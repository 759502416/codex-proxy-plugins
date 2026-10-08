use super::{
    response::{ApiResult, json_reply},
    validation::{decode_json, require_account_id},
};
use crate::credits;
use gateway_plugin_sdk::{call::management::ManagementRequest, client::TypedCall};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccountRequest {
    account_id: String,
}

pub(super) async fn query(call: TypedCall<ManagementRequest>) -> ApiResult {
    let request: AccountRequest = decode_json(&call)?;
    require_account_id(&request.account_id)?;
    let credits = credits::fetch(&call.host, &request.account_id).await?;
    json_reply(&credits::summary(&credits))
}
