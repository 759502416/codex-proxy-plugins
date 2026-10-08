use super::{
    response::{ApiError, ApiResult, json_reply},
    validation::require_empty_body,
};
use crate::{
    accounts::{self, ListError},
    automation::{
        records::{self, AutoResult},
        settings::{self, AutoSettings, SettingsDocument},
    },
    host_admin::{Account, QuotaWindow},
    operations::{self, AccountRecord, LastResult},
};
use gateway_plugin_sdk::{
    PluginFault,
    call::management::ManagementRequest,
    client::{HostClient, TypedCall},
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccountView {
    id: String,
    name: String,
    email: Option<String>,
    plan_type_display: String,
    status: String,
    enabled: bool,
    consumable: bool,
    quota_refreshed_at_display: String,
    rate_limit_recovery_display: Option<String>,
    windows: Vec<WindowView>,
    pending: Option<PendingView>,
    last: Option<LastResult>,
    auto: Option<AutoSettings>,
    auto_last: Option<AutoResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowView {
    key: String,
    label: String,
    used_percent: Option<f64>,
    used_percent_display: String,
    reset_at_display: String,
    limit_reached: bool,
}

/// 只暴露开始时间；幂等键和卡片 ID 仅用于插件内部重试。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingView {
    started_at_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountsResponse {
    accounts: Vec<AccountView>,
}

pub(super) async fn list(call: TypedCall<ManagementRequest>) -> ApiResult {
    require_empty_body(&call.request, &call.payload)?;
    let accounts = accounts::fetch_all(&call.host)
        .await
        .map_err(|error| match error {
            ListError::Host(error) => ApiError::from(error),
            ListError::TooManyAccounts => {
                ApiError::new(502, "too_many_accounts", "账号数量超过插件单次列出上限")
            }
        })?;
    let settings = settings::load(&call.host).await?.value;
    let mut views = Vec::with_capacity(accounts.len());
    for account in accounts {
        let record = operations::load(&call.host, &account.id).await?.value;
        views.push(view(&call.host, account, &record, &settings).await?);
    }
    json_reply(&AccountsResponse { accounts: views })
}

pub(super) async fn view(
    host: &HostClient,
    account: Account,
    record: &AccountRecord,
    settings: &SettingsDocument,
) -> Result<AccountView, PluginFault> {
    let automation = records::load(host, &account.id).await?.value;
    Ok(AccountView {
        consumable: account.capabilities.consume_reset_credit,
        windows: summary_windows(account.quota.windows),
        auto: settings.accounts.get(&account.id).copied(),
        auto_last: automation.last,
        id: account.id,
        name: account.name,
        email: account.email,
        plan_type_display: account.plan_type_display,
        status: account.status,
        enabled: account.enabled,
        quota_refreshed_at_display: account.quota.refreshed_at_display,
        rate_limit_recovery_display: account.quota.rate_limit_recovery_display,
        pending: record.pending.as_ref().map(|pending| PendingView {
            started_at_ms: pending.started_at_ms,
        }),
        last: record.last.clone(),
    })
}

/// 与宿主账号表摘要一致：优先展示已知分组的窗口，全是其他分组时才展示全部；
/// 分组按短期、月度排序，同组保留 Provider 的投影顺序。
fn summary_windows(mut windows: Vec<QuotaWindow>) -> Vec<WindowView> {
    let known = windows.iter().any(|window| window.group != "other");
    windows.sort_by_key(|window| match window.group.as_str() {
        "shortTerm" => 0,
        "monthly" => 1,
        _ => 2,
    });
    windows
        .into_iter()
        .filter(|window| !known || window.group != "other")
        .map(|window| WindowView {
            key: window.key,
            label: window.label_display,
            used_percent: window.used_percent,
            used_percent_display: window.used_percent_display,
            reset_at_display: window.reset_at_display,
            limit_reached: window.limit_reached,
        })
        .collect()
}
