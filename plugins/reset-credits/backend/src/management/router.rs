use super::{
    accounts, auto, credits, reset,
    response::{ApiError, ApiResult},
};
use gateway_plugin_sdk::{
    PluginFault,
    call::management::{ManagementRequest, ManagementResponse},
    client::{TypedCall, TypedReply},
};

/// 批量设置最多 200 个账号 ID，每个最多 512 字节。
const MAXIMUM_BODY_BYTES: usize = 128 * 1024;

pub(crate) async fn handle(
    call: TypedCall<ManagementRequest>,
) -> Result<TypedReply<ManagementResponse>, PluginFault> {
    route(call).await.or_else(ApiError::into_reply)
}

async fn route(call: TypedCall<ManagementRequest>) -> ApiResult {
    if !call.request.query.is_empty() {
        return Err(ApiError::invalid("此接口不支持查询参数"));
    }
    if call.payload.len() > MAXIMUM_BODY_BYTES {
        return Err(ApiError::invalid("请求正文过大"));
    }
    match (call.request.method.as_str(), call.request.path.as_str()) {
        ("GET", "api/accounts") => accounts::list(call).await,
        ("POST", "api/credits") => credits::query(call).await,
        ("POST", "api/reset") => reset::consume(call).await,
        ("POST", "api/discard") => reset::discard(call).await,
        ("POST", "api/auto") => auto::update(call).await,
        _ => Err(ApiError::new(404, "not_found", "未找到插件管理接口")),
    }
}
