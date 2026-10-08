use super::{
    accounts::{self, AccountView},
    response::{ApiError, ApiResult, json_reply},
    validation::{decode_json, require_account_id},
};
use crate::{
    automation::settings,
    credits::CreditsSummary,
    host_admin::Account,
    operations::AccountRecord,
    reset::{self, Origin, ResetError, ResetOutcome},
};
use gateway_plugin_sdk::{
    call::management::ManagementRequest,
    client::{HostClient, TypedCall},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResetRequest {
    account_id: String,
    /// 确认上次未决的消费：原样复用已保存的幂等键和卡片 ID。
    #[serde(default)]
    resume: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccountRequest {
    account_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResetResponse {
    outcome: ResetOutcome,
    message: String,
    warnings: Vec<String>,
    credits: Option<CreditsSummary>,
    account: Option<AccountView>,
}

pub(super) async fn consume(call: TypedCall<ManagementRequest>) -> ApiResult {
    let request: ResetRequest = decode_json(&call)?;
    require_account_id(&request.account_id)?;
    let mut execution = reset::execute(
        &call.host,
        &request.account_id,
        request.resume,
        Origin::Manual,
    )
    .await
    .map_err(api_error)?;
    // 扣卡已确认，账号行回读失败只作为提醒，不能报告为错误。
    let account = match execution.account.take() {
        Some(account) => refreshed_view(&call.host, account, &execution.record).await,
        None => None,
    };
    json_reply(&ResetResponse {
        outcome: execution.outcome,
        message: execution.message,
        warnings: execution.warnings,
        credits: execution.credits,
        account,
    })
}

pub(super) async fn discard(call: TypedCall<ManagementRequest>) -> ApiResult {
    let request: AccountRequest = decode_json(&call)?;
    require_account_id(&request.account_id)?;
    reset::discard(&call.host, &request.account_id)
        .await
        .map_err(api_error)?;
    json_reply(&json!({ "discarded": true }))
}

async fn refreshed_view(
    host: &HostClient,
    account: Account,
    record: &AccountRecord,
) -> Option<AccountView> {
    let settings = settings::load(host).await.ok()?;
    accounts::view(host, account, record, &settings.value)
        .await
        .ok()
}

fn api_error(error: ResetError) -> ApiError {
    match error {
        ResetError::NoPending => ApiError::conflict("no_pending", "该账号没有待确认的重置操作"),
        ResetError::PendingExists => ApiError::conflict(
            "pending_exists",
            "该账号上次重置的结果尚未确认，请先确认上次重置",
        ),
        ResetError::Busy => ApiError::conflict("busy", "该账号正在进行另一笔重置，请稍后刷新"),
        ResetError::Host(error) => ApiError::from(error),
        ResetError::State(error) => ApiError::from(error),
    }
}
