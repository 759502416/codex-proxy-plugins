use super::{
    response::{ApiError, ApiResult, json_reply},
    validation::{decode_json, require_account_id},
};
use crate::automation::settings::{self, AutoSettings, UpdateError};
use gateway_plugin_sdk::{call::management::ManagementRequest, client::TypedCall};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;

const MAXIMUM_ACCOUNTS_PER_REQUEST: usize = 200;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutoRequest {
    account_ids: Vec<String>,
    /// 为空表示关闭所选账号的自动用卡。
    settings: Option<AutoSettings>,
}

pub(super) async fn update(call: TypedCall<ManagementRequest>) -> ApiResult {
    let request: AutoRequest = decode_json(&call)?;
    if request.account_ids.is_empty() || request.account_ids.len() > MAXIMUM_ACCOUNTS_PER_REQUEST {
        return Err(ApiError::invalid("每次需选择 1–200 个账号"));
    }
    for account_id in &request.account_ids {
        require_account_id(account_id)?;
    }
    if request.settings.is_some_and(|settings| !settings.valid()) {
        return Err(ApiError::invalid("阈值须为 1–100 的整数"));
    }
    let account_ids: Vec<String> = request
        .account_ids
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    settings::update(&call.host, &account_ids, request.settings)
        .await
        .map_err(|error| match error {
            UpdateError::TooManyAccounts => ApiError::invalid("开启自动用卡的账号数量超过插件上限"),
            UpdateError::State(error) => ApiError::from(error),
        })?;
    json_reply(&json!({ "updated": account_ids.len() }))
}
