//! 每个账号一条自动用卡记录，只由维护任务写入。

use crate::state::{self, Stored};
use gateway_plugin_sdk::{PluginFault, client::HostClient};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AutomationRecord {
    /// 已占用自动用卡的周额度周期，记录该周期的重置时间。
    pub cycle_reset_at_ms: Option<i64>,
    pub last: Option<AutoResult>,
    /// 退避截止时间，之前不再为该账号访问上游。
    pub retry_at_ms: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AutoResult {
    pub outcome: AutoOutcome,
    /// 上游结果码或插件内部原因。
    pub code: Option<String>,
    pub trigger: Option<Trigger>,
    pub at_ms: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutoOutcome {
    /// 已占用周期、正在消费；长时间停留表示上次处理中断。
    Resetting,
    Reset,
    NoCredit,
    NotExecuted,
    Failed,
    Unknown,
    CycleUsed,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Trigger {
    Weekly,
    ShortTerm,
    Both,
}

pub(crate) async fn load(
    host: &HostClient,
    account_id: &str,
) -> Result<Stored<AutomationRecord>, PluginFault> {
    state::load(host, state::AUTOMATION, account_id).await
}

pub(crate) async fn save(
    host: &HostClient,
    account_id: &str,
    expected_version: Option<u64>,
    record: &AutomationRecord,
) -> Result<u64, PluginFault> {
    state::save(
        host,
        state::AUTOMATION,
        account_id,
        expected_version,
        record,
    )
    .await
}
