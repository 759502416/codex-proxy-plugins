use super::response::{ApiError, JSON_CONTENT_TYPE};
use gateway_plugin_sdk::{call::management::ManagementRequest, client::TypedCall};
use serde::Deserialize;

pub(super) fn require_empty_body(
    request: &ManagementRequest,
    payload: &[u8],
) -> Result<(), ApiError> {
    if request.content_type.is_none() && payload.is_empty() {
        Ok(())
    } else {
        Err(ApiError::invalid("此接口不接受请求正文"))
    }
}

pub(super) fn decode_json<T: for<'de> Deserialize<'de>>(
    call: &TypedCall<ManagementRequest>,
) -> Result<T, ApiError> {
    if !call
        .request
        .content_type
        .as_deref()
        .and_then(|content_type| content_type.split(';').next())
        .is_some_and(|content_type| content_type.trim().eq_ignore_ascii_case(JSON_CONTENT_TYPE))
    {
        return Err(ApiError::new(
            400,
            "invalid_content_type",
            "此接口需要 application/json 内容类型",
        ));
    }
    serde_json::from_slice(&call.payload).map_err(|_| ApiError::invalid("请求正文不是有效 JSON"))
}

/// 账号 ID 同时用作宿主查询参数和私有状态键，沿用状态键的长度与字符限制。
pub(super) fn require_account_id(value: &str) -> Result<(), ApiError> {
    if !value.is_empty()
        && value.chars().count() <= 256
        && value.len() <= 512
        && !value.chars().any(char::is_control)
    {
        Ok(())
    } else {
        Err(ApiError::invalid("账号 ID 无效"))
    }
}
