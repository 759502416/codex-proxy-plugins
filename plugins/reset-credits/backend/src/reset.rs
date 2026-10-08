//! 单账号消费一张重置卡，手动页面与自动用卡共用。
//!
//! 扣卡不可逆：未决记录在发出前写入，结果未知时保留；只有拿到明确结果才清除，
//! 确认扣卡后的回读失败只作为提醒，避免误以为未扣卡而重复操作。

use crate::{
    credits::{self, CreditsSummary},
    host_admin::{
        self, Account, AccountIdBody, AdminError, ConsumeRequest, ConsumeResult, QuotaRefresh,
        UPSTREAM_RESULT_UNKNOWN,
    },
    operations::{self, AccountRecord, LastResult, Outcome, PendingConsume},
};
use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::host::{LogLevel, LogRequest},
    client::HostClient,
};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

/// 状态 schema 中结果码的长度上限。
const MAXIMUM_CODE_CHARS: usize = 64;
const RECORD_SAVE_FAILED: &str = "操作记录保存失败，刷新后该账号可能显示为待确认";

#[derive(Clone, Copy)]
pub(crate) enum Origin {
    Manual,
    Auto,
}

/// 消费发出前的拒绝；这些情况都没有调用消费接口。
pub(crate) enum ResetError {
    NoPending,
    PendingExists,
    /// 写入未决记录时版本已变化，另一笔操作已经开始。
    Busy,
    Host(AdminError),
    State(PluginFault),
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResetOutcome {
    Reset,
    NoCredit,
    NotExecuted,
    Failed,
    Unknown,
}

pub(crate) struct Execution {
    pub outcome: ResetOutcome,
    /// 上游结果码；没有拿到上游结果时为空。
    pub code: Option<String>,
    pub message: String,
    pub warnings: Vec<String>,
    pub credits: Option<CreditsSummary>,
    /// 确认扣卡后由宿主刷新额度得到的账号。
    pub account: Option<Account>,
    /// 操作结束后的账号记录，保存失败时仍保留未决消费。
    pub record: AccountRecord,
}

impl Execution {
    fn new(outcome: ResetOutcome, message: impl Into<String>, record: AccountRecord) -> Self {
        Self {
            outcome,
            code: None,
            message: message.into(),
            warnings: Vec::new(),
            credits: None,
            account: None,
            record,
        }
    }
}

/// `resume` 为真时原样重试已保存的未决消费，否则查卡并发起新的一笔。
pub(crate) async fn execute(
    host: &HostClient,
    account_id: &str,
    resume: bool,
    origin: Origin,
) -> Result<Execution, ResetError> {
    let stored = operations::load(host, account_id)
        .await
        .map_err(ResetError::State)?;
    let (pending, version) = if resume {
        let pending = stored.value.pending.clone().ok_or(ResetError::NoPending)?;
        (pending, stored.version)
    } else {
        if stored.value.pending.is_some() {
            return Err(ResetError::PendingExists);
        }
        let available = credits::fetch(host, account_id)
            .await
            .map_err(ResetError::Host)?;
        if available.available_count == 0 {
            // 没有卡时不调用消费接口；记录仅用于展示，写入失败不影响结论。
            let record = finish(host, account_id, stored.version, Outcome::NoCredit, None)
                .await
                .unwrap_or(stored.value);
            let mut execution =
                Execution::new(ResetOutcome::NoCredit, "当前没有可用的主动重置卡", record);
            execution.credits = Some(credits::summary(&available));
            return Ok(execution);
        }
        let pending = PendingConsume {
            redeem_request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            credit_id: credits::pick(&available).map(|credit| credit.id.clone()),
            started_at_ms: now_ms(),
        };
        let record = AccountRecord {
            pending: Some(pending.clone()),
            last: stored.value.last.clone(),
        };
        let version = operations::save(host, account_id, stored.version, &record)
            .await
            .map_err(|error| {
                if error.code == ErrorCode::Conflict {
                    ResetError::Busy
                } else {
                    ResetError::State(error)
                }
            })?;
        (pending, Some(version))
    };
    let pending_record = AccountRecord {
        pending: Some(pending.clone()),
        last: stored.value.last,
    };

    let result = host_admin::post::<ConsumeResult>(
        host,
        "/api/admin/accounts/reset-credits",
        &ConsumeRequest {
            account_id,
            credit_id: pending.credit_id.as_deref(),
            redeem_request_id: &pending.redeem_request_id,
        },
    )
    .await;
    let execution = match result {
        Ok(result) => confirmed(host, account_id, version, resume, pending_record, result).await,
        Err(error) if ambiguous(&error) => Execution::new(
            ResetOutcome::Unknown,
            "消费结果暂不确定，确认上次重置时会复用同一个请求标识",
            pending_record,
        ),
        Err(error) if resume => {
            // 本次重试被拒不能证明首次发出的消费未执行，未决记录继续保留。
            let mut execution = Execution::new(
                ResetOutcome::Failed,
                rejection_message(error),
                pending_record,
            );
            execution.warnings.push("上次重置的结果仍待确认".to_owned());
            execution
        }
        Err(error) => {
            let message = rejection_message(error);
            match finish(host, account_id, version, Outcome::Failed, None).await {
                Ok(record) => Execution::new(ResetOutcome::Failed, message, record),
                Err(_) => {
                    let mut execution =
                        Execution::new(ResetOutcome::Failed, message, pending_record);
                    execution.warnings.push(RECORD_SAVE_FAILED.to_owned());
                    execution
                }
            }
        }
    };
    log(host, account_id, &execution, resume, origin, &pending).await;
    Ok(execution)
}

/// 删除未决记录；上次消费可能已经完成，由调用方确认后果。
pub(crate) async fn discard(host: &HostClient, account_id: &str) -> Result<(), ResetError> {
    let stored = operations::load(host, account_id)
        .await
        .map_err(ResetError::State)?;
    if stored.value.pending.is_none() {
        return Err(ResetError::NoPending);
    }
    finish(host, account_id, stored.version, Outcome::Discarded, None)
        .await
        .map(drop)
        .map_err(|error| {
            if error.code == ErrorCode::Conflict {
                ResetError::Busy
            } else {
                ResetError::State(error)
            }
        })
}

/// 宿主返回了上游结果码，这次幂等键的执行结果已经明确。
async fn confirmed(
    host: &HostClient,
    account_id: &str,
    version: Option<u64>,
    resumed: bool,
    pending_record: AccountRecord,
    result: ConsumeResult,
) -> Execution {
    // 首次发出时上游不应报告已处理；仅在重试同一幂等键时把它视为上次已成功。
    let reset = result.code == "reset" || (resumed && result.code == "already_redeemed");
    let (outcome, saved_outcome, message) = if reset {
        let message = if result.code == "already_redeemed" {
            "上次重置已完成"
        } else {
            "额度已重置"
        };
        (ResetOutcome::Reset, Outcome::Reset, message.to_owned())
    } else if result.code == "no_credit" {
        (
            ResetOutcome::NoCredit,
            Outcome::NoCredit,
            "当前没有可用的主动重置卡".to_owned(),
        )
    } else {
        (
            ResetOutcome::NotExecuted,
            Outcome::NotExecuted,
            not_executed_message(&result.code),
        )
    };
    let code = bounded_code(&result.code);
    let mut execution = match finish(host, account_id, version, saved_outcome, code.clone()).await {
        Ok(record) => Execution::new(outcome, message, record),
        Err(_) => {
            let mut execution = Execution::new(outcome, message, pending_record);
            execution.warnings.push(RECORD_SAVE_FAILED.to_owned());
            execution
        }
    };
    execution.code = code;
    if !reset {
        return execution;
    }

    match credits::fetch(host, account_id).await {
        Ok(credits) => execution.credits = Some(credits::summary(&credits)),
        Err(_) => execution.warnings.push("最新重置卡数量加载失败".to_owned()),
    }
    // 由宿主按上游真实额度恢复账号状态，插件不直接解除冻结或改写重置时间。
    match host_admin::post::<QuotaRefresh>(
        host,
        "/api/admin/accounts/quota/refresh",
        &AccountIdBody { account_id },
    )
    .await
    {
        Ok(refreshed) => execution.account = Some(refreshed.account),
        Err(_) => execution
            .warnings
            .push("最新额度加载失败，请稍后手动刷新额度".to_owned()),
    }
    execution
}

async fn finish(
    host: &HostClient,
    account_id: &str,
    version: Option<u64>,
    outcome: Outcome,
    code: Option<String>,
) -> Result<AccountRecord, PluginFault> {
    let record = AccountRecord {
        pending: None,
        last: Some(LastResult {
            outcome,
            code,
            at_ms: now_ms(),
        }),
    };
    operations::save(host, account_id, version, &record).await?;
    Ok(record)
}

/// 宿主合同中结果未知的情形；超时与网关超时同样无法证明消费未发出。
fn ambiguous(error: &AdminError) -> bool {
    match error {
        AdminError::Dispatch | AdminError::Invalid => true,
        AdminError::Rejected { status, code, .. } => {
            *code == UPSTREAM_RESULT_UNKNOWN || matches!(status, 408 | 504)
        }
        AdminError::Encoding => false,
    }
}

/// 宿主管理端的错误文案本身面向管理员，原样展示。
pub(crate) fn rejection_message(error: AdminError) -> String {
    match error {
        AdminError::Rejected { message, .. } if !message.trim().is_empty() => message,
        AdminError::Rejected { .. } => "宿主拒绝了请求".to_owned(),
        AdminError::Dispatch => "宿主管理接口调用失败".to_owned(),
        AdminError::Invalid => "宿主管理接口返回了无法识别的响应".to_owned(),
        AdminError::Encoding => "管理接口请求编码失败".to_owned(),
    }
}

fn not_executed_message(code: &str) -> String {
    match code {
        "already_redeemed" => "该重置操作已被处理，请先刷新重置卡数量".to_owned(),
        "nothing_to_reset" => "当前额度窗口不需要重置".to_owned(),
        _ => format!(
            "上游未执行额度重置：{}",
            bounded_code(code).unwrap_or_default()
        ),
    }
}

pub(crate) fn bounded_code(code: &str) -> Option<String> {
    let code: String = code
        .chars()
        .filter(|character| !character.is_control())
        .take(MAXIMUM_CODE_CHARS)
        .collect();
    (!code.is_empty()).then_some(code)
}

/// 日志只记录账号与结果，不包含幂等键、卡片 ID 或上游正文。
async fn log(
    host: &HostClient,
    account_id: &str,
    execution: &Execution,
    resumed: bool,
    origin: Origin,
    pending: &PendingConsume,
) {
    let outcome = serde_json::to_value(execution.outcome).unwrap_or_default();
    let fields = BTreeMap::from([
        ("account_id".to_owned(), json!(account_id)),
        ("outcome".to_owned(), outcome),
        ("code".to_owned(), json!(execution.code)),
        ("resumed".to_owned(), json!(resumed)),
        ("targeted".to_owned(), json!(pending.credit_id.is_some())),
        (
            "origin".to_owned(),
            json!(match origin {
                Origin::Manual => "manual",
                Origin::Auto => "auto",
            }),
        ),
    ]);
    let request = LogRequest {
        event: "reset_credits.consume".to_owned(),
        level: if execution.outcome == ResetOutcome::Reset {
            LogLevel::Info
        } else {
            LogLevel::Warn
        },
        fields,
    };
    // 日志超出宿主预算会被丢弃，不影响消费结论，也不重试。
    if let Ok(params) = serde_json::to_value(&request) {
        let _ = host.call("host.log", params, Vec::new()).await;
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}
