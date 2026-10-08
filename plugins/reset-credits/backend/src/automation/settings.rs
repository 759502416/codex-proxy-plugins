//! 全部账号的自动用卡设置保存在一条记录中；未出现在记录中的账号视为关闭。

use crate::state::{self, Stored};
use gateway_plugin_sdk::{ErrorCode, PluginFault, client::HostClient};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const KEY: &str = "auto";
/// 与状态 schema 的 `maxProperties` 一致，单条记录保持在值大小上限内。
const MAXIMUM_ACCOUNTS: usize = 3000;
const UPDATE_ATTEMPTS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AutoSettings {
    /// 周额度已用百分比达到该值时用卡。
    pub weekly_threshold: u8,
    /// 5 小时额度阈值；为空表示不因 5 小时额度用卡。
    pub short_term_threshold: Option<u8>,
}

impl AutoSettings {
    pub(crate) fn valid(&self) -> bool {
        (1..=100).contains(&self.weekly_threshold)
            && self
                .short_term_threshold
                .is_none_or(|threshold| (1..=100).contains(&threshold))
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SettingsDocument {
    pub accounts: BTreeMap<String, AutoSettings>,
}

pub(crate) enum UpdateError {
    TooManyAccounts,
    State(PluginFault),
}

pub(crate) async fn load(host: &HostClient) -> Result<Stored<SettingsDocument>, PluginFault> {
    state::load(host, state::SETTINGS, KEY).await
}

/// 增量修改所选账号；`None` 表示关闭。记录被并发修改时重新读取再应用。
pub(crate) async fn update(
    host: &HostClient,
    account_ids: &[String],
    settings: Option<AutoSettings>,
) -> Result<(), UpdateError> {
    for _ in 0..UPDATE_ATTEMPTS {
        let mut stored = load(host).await.map_err(UpdateError::State)?;
        for account_id in account_ids {
            match settings {
                Some(settings) => {
                    stored.value.accounts.insert(account_id.clone(), settings);
                }
                None => {
                    stored.value.accounts.remove(account_id);
                }
            }
        }
        if stored.value.accounts.len() > MAXIMUM_ACCOUNTS {
            return Err(UpdateError::TooManyAccounts);
        }
        match state::save(host, state::SETTINGS, KEY, stored.version, &stored.value).await {
            Ok(_) => return Ok(()),
            Err(error) if error.code == ErrorCode::Conflict => {}
            Err(error) => return Err(UpdateError::State(error)),
        }
    }
    Err(UpdateError::State(PluginFault::new(
        ErrorCode::Conflict,
        "自动用卡设置正在被修改",
    )))
}
