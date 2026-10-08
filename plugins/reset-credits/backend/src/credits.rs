//! 重置卡查询与选卡；每次实时查询上游，宿主不缓存卡片列表。

use crate::host_admin::{self, AdminError, ResetCredit, ResetCredits};
use gateway_plugin_sdk::client::HostClient;
use serde::Serialize;

/// 页面只需要张数和最早到期时间；卡片 ID 留在插件内部，不返回给页面。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreditsSummary {
    pub available_count: u64,
    pub earliest_expires_at_display: Option<String>,
}

pub(crate) async fn fetch(host: &HostClient, account_id: &str) -> Result<ResetCredits, AdminError> {
    host_admin::get(
        host,
        &format!(
            "/api/admin/accounts/reset-credits?accountId={}",
            host_admin::query_value(account_id)
        ),
    )
    .await
}

pub(crate) fn summary(credits: &ResetCredits) -> CreditsSummary {
    CreditsSummary {
        available_count: credits.available_count,
        earliest_expires_at_display: available(credits)
            .next()
            .and_then(|credit| credit.expires_at_display.clone()),
    }
}

/// 选择最早到期的可用卡；列表为空但张数大于 0 时返回 `None`，由上游选择。
/// 与宿主账号页一致：只有状态为 `available` 的卡可被指定。
pub(crate) fn pick(credits: &ResetCredits) -> Option<&ResetCredit> {
    available(credits).next()
}

fn available(credits: &ResetCredits) -> impl Iterator<Item = &ResetCredit> {
    let mut available: Vec<&ResetCredit> = credits
        .credits
        .iter()
        .filter(|credit| credit.status.as_deref() == Some("available") && !credit.id.is_empty())
        .collect();
    // 宿主统一输出 UTC 的 RFC 3339 时间，字符串顺序即时间顺序；缺少到期时间的卡排在最后。
    available.sort_by(|left, right| match (&left.expires_at, &right.expires_at) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    available.into_iter()
}
