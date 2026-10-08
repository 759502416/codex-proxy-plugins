//! 账号操作记录。未决消费在发出前写入，记录存在即表示结果未确认，
//! 重试必须原样复用其中的幂等键和卡片 ID；版本校验保证同一账号同时只有一笔消费。

use crate::state::{self, Stored};
use gateway_plugin_sdk::{PluginFault, client::HostClient};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountRecord {
    pub pending: Option<PendingConsume>,
    pub last: Option<LastResult>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PendingConsume {
    pub redeem_request_id: String,
    pub credit_id: Option<String>,
    pub started_at_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LastResult {
    pub outcome: Outcome,
    /// 上游结果码；本地判定或宿主拒绝时为空。
    pub code: Option<String>,
    pub at_ms: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Reset,
    NoCredit,
    NotExecuted,
    Failed,
    Discarded,
}

pub(crate) async fn load(
    host: &HostClient,
    account_id: &str,
) -> Result<Stored<AccountRecord>, PluginFault> {
    state::load(host, state::ACCOUNTS, account_id).await
}

pub(crate) async fn save(
    host: &HostClient,
    account_id: &str,
    expected_version: Option<u64>,
    record: &AccountRecord,
) -> Result<u64, PluginFault> {
    state::save(host, state::ACCOUNTS, account_id, expected_version, record).await
}
