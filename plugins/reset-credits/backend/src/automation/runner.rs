//! 维护入口的一轮处理。宿主每次调用限时 30 秒，同一实例串行执行；
//! 本轮来不及处理的账号由下一轮从中断处继续。

use super::{
    records::{self, AutoOutcome, AutoResult, AutomationRecord, Trigger},
    settings::{self, AutoSettings},
    windows,
};
use crate::{
    accounts,
    host_admin::Account,
    operations,
    reset::{self, Origin, ResetError, ResetOutcome},
    state::Stored,
};
use gateway_plugin_sdk::{
    PluginFault,
    call::{
        data::QuotaFactsQuery,
        host::{LogLevel, LogRequest},
    },
    client::{Empty, HostClient, TypedCall, TypedReply},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// 超过该时长不再开始新账号，给进行中的消费留出时间。
const ROUND_BUDGET: Duration = Duration::from_secs(12);
const FAILURE_BACKOFF_MS: u64 = 2 * 60_000;
const IDLE_BACKOFF_MS: u64 = 10 * 60_000;
const UNKNOWN_RETRY_MS: u64 = 60_000;
/// 同一周期的重置时间来自上游固定时间戳，容差只吸收不同观测来源的取整差异。
const SAME_CYCLE_TOLERANCE_MS: i64 = 5 * 60_000;

pub(crate) async fn reconcile(
    cursor: Arc<AtomicUsize>,
    call: TypedCall<Empty>,
) -> Result<TypedReply<Empty>, PluginFault> {
    // 单个账号或本轮失败只记录并等待下一轮，不返回错误触发宿主的快速重试。
    run(&call.host, &cursor).await;
    Ok(TypedReply::new(Empty {}))
}

async fn run(host: &HostClient, cursor: &AtomicUsize) {
    let started = Instant::now();
    let Ok(settings) = settings::load(host).await else {
        log(host, "settings_unavailable", None).await;
        return;
    };
    if settings.value.accounts.is_empty() {
        return;
    }
    let Ok(accounts) = accounts::fetch_all(host).await else {
        log(host, "accounts_unavailable", None).await;
        return;
    };
    let candidates: Vec<(Account, AutoSettings)> = accounts
        .into_iter()
        .filter(|account| {
            account.enabled
                && account.capabilities.consume_reset_credit
                && !matches!(account.status.as_str(), "disabled" | "error")
        })
        .filter_map(|account| {
            let settings = *settings.value.accounts.get(&account.id)?;
            Some((account, settings))
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    let start = cursor.load(Ordering::Relaxed) % candidates.len();
    for offset in 0..candidates.len() {
        let index = (start + offset) % candidates.len();
        if started.elapsed() >= ROUND_BUDGET {
            cursor.store(index, Ordering::Relaxed);
            return;
        }
        let (account, settings) = &candidates[index];
        process(host, account, settings).await;
    }
}

async fn process(host: &HostClient, account: &Account, settings: &AutoSettings) {
    let main = windows::main_windows(&account.quota.windows);
    // 本地观测未达到阈值时不访问上游。
    if windows::cached_trigger(&main, settings).is_none() {
        return;
    }
    let account_id = account.id.as_str();
    let (Ok(operation), Ok(stored)) = (
        operations::load(host, account_id).await,
        records::load(host, account_id).await,
    ) else {
        return;
    };
    let resume = operation.value.pending.is_some();
    if stored
        .value
        .retry_at_ms
        .is_some_and(|retry_at| reset::now_ms() < retry_at)
    {
        return;
    }
    let previous_cycle = stored.value.cycle_reset_at_ms;
    let facts = match host
        .refresh_account_quota(QuotaFactsQuery {
            account_id: account_id.to_owned(),
        })
        .await
    {
        Ok(facts) => facts,
        Err(_) => {
            let result = auto_result(AutoOutcome::Failed, Some("quota_refresh_failed"), None);
            settle(
                host,
                account_id,
                &stored,
                previous_cycle,
                result,
                FAILURE_BACKOFF_MS,
            )
            .await;
            return;
        }
    };
    let fresh = windows::fresh_state(&facts, &main, settings);
    // 上游已恢复或仍低于阈值。
    let Some(trigger) = fresh.trigger else {
        return;
    };
    let Some(cycle) = fresh.weekly_reset_at_ms else {
        let result = auto_result(
            AutoOutcome::Failed,
            Some("weekly_reset_unknown"),
            Some(trigger),
        );
        settle(
            host,
            account_id,
            &stored,
            previous_cycle,
            result,
            IDLE_BACKOFF_MS,
        )
        .await;
        return;
    };
    // 未决消费的重试复用同一张卡，不受周期限制。
    if !resume && previous_cycle.is_some_and(|used| (used - cycle).abs() <= SAME_CYCLE_TOLERANCE_MS)
    {
        let result = auto_result(AutoOutcome::CycleUsed, None, Some(trigger));
        settle(
            host,
            account_id,
            &stored,
            previous_cycle,
            result,
            IDLE_BACKOFF_MS,
        )
        .await;
        return;
    }
    // 查询期间管理员可能刚关闭开关，发出前再读一次。
    if !settings::load(host)
        .await
        .is_ok_and(|current| current.value.accounts.contains_key(account_id))
    {
        return;
    }
    // 先占用周期再发出消费：中断时宁可本周期不再自动用卡，也不扣第二张。
    let claim = AutomationRecord {
        cycle_reset_at_ms: Some(cycle),
        last: Some(auto_result(AutoOutcome::Resetting, None, Some(trigger))),
        retry_at_ms: None,
    };
    let Ok(version) = records::save(host, account_id, stored.version, &claim).await else {
        return;
    };
    let claimed = Stored {
        version: Some(version),
        value: claim,
    };

    let (cycle_after, outcome, code, backoff) =
        match reset::execute(host, account_id, resume, Origin::Auto).await {
            Ok(execution) => match execution.outcome {
                ResetOutcome::Reset => (Some(cycle), AutoOutcome::Reset, execution.code, 0),
                // 结果未知时保留周期占用，下一轮用同一请求标识确认。
                ResetOutcome::Unknown => {
                    (Some(cycle), AutoOutcome::Unknown, None, UNKNOWN_RETRY_MS)
                }
                ResetOutcome::NoCredit => (
                    previous_cycle,
                    AutoOutcome::NoCredit,
                    execution.code,
                    IDLE_BACKOFF_MS,
                ),
                ResetOutcome::NotExecuted => (
                    previous_cycle,
                    AutoOutcome::NotExecuted,
                    execution.code,
                    IDLE_BACKOFF_MS,
                ),
                ResetOutcome::Failed => (
                    previous_cycle,
                    AutoOutcome::Failed,
                    None,
                    FAILURE_BACKOFF_MS,
                ),
            },
            // 以下情况都没有发出消费，释放周期占用。
            Err(ResetError::Host(_)) => (
                previous_cycle,
                AutoOutcome::Failed,
                Some("credits_query_failed".to_owned()),
                FAILURE_BACKOFF_MS,
            ),
            Err(ResetError::PendingExists | ResetError::Busy | ResetError::NoPending) => (
                previous_cycle,
                AutoOutcome::Failed,
                Some("account_busy".to_owned()),
                UNKNOWN_RETRY_MS,
            ),
            Err(ResetError::State(_)) => (
                previous_cycle,
                AutoOutcome::Failed,
                Some("state_unavailable".to_owned()),
                FAILURE_BACKOFF_MS,
            ),
        };
    let result = AutoResult {
        outcome,
        code,
        trigger: Some(trigger),
        at_ms: reset::now_ms(),
    };
    settle(host, account_id, &claimed, cycle_after, result, backoff).await;
}

fn auto_result(outcome: AutoOutcome, code: Option<&str>, trigger: Option<Trigger>) -> AutoResult {
    AutoResult {
        outcome,
        code: code.map(str::to_owned),
        trigger,
        at_ms: reset::now_ms(),
    }
}

/// 写入本次结果；失败只影响展示和退避，下一轮按记录重新判断。
async fn settle(
    host: &HostClient,
    account_id: &str,
    stored: &Stored<AutomationRecord>,
    cycle: Option<i64>,
    result: AutoResult,
    backoff_ms: u64,
) {
    let record = AutomationRecord {
        cycle_reset_at_ms: cycle,
        retry_at_ms: (backoff_ms > 0).then(|| reset::now_ms() + backoff_ms),
        last: Some(result),
    };
    if records::save(host, account_id, stored.version, &record)
        .await
        .is_err()
    {
        log(host, "record_save_failed", Some(account_id)).await;
    }
}

async fn log(host: &HostClient, reason: &str, account_id: Option<&str>) {
    let mut fields = BTreeMap::from([("reason".to_owned(), json!(reason))]);
    if let Some(account_id) = account_id {
        fields.insert("account_id".to_owned(), json!(account_id));
    }
    let request = LogRequest {
        event: "reset_credits.auto".to_owned(),
        level: LogLevel::Warn,
        fields,
    };
    if let Ok(params) = serde_json::to_value(&request) {
        let _ = host.call("host.log", params, Vec::new()).await;
    }
}
