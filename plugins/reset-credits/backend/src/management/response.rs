use crate::{host_admin::AdminError, reset};
use gateway_plugin_sdk::{
    ErrorCode, PluginFault, call::management::ManagementResponse, client::TypedReply,
};
use serde::Serialize;
use serde_json::json;

pub(super) const JSON_CONTENT_TYPE: &str = "application/json";
pub(super) type ApiResult = Result<TypedReply<ManagementResponse>, ApiError>;

#[derive(Serialize)]
pub(super) struct ApiError {
    #[serde(skip)]
    status: u16,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(400, "invalid_request", message)
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(409, code, message)
    }

    pub fn into_reply(self) -> Result<TypedReply<ManagementResponse>, PluginFault> {
        encode(self.status, &json!({ "error": self }))
    }
}

impl From<PluginFault> for ApiError {
    fn from(error: PluginFault) -> Self {
        if error.code == ErrorCode::Conflict {
            Self::conflict("conflict", "账号记录已被另一个操作更新，请刷新后重试")
        } else {
            Self::new(502, "host_callback", "宿主操作未完成")
        }
    }
}

impl From<AdminError> for ApiError {
    fn from(error: AdminError) -> Self {
        // 状态只保留调用方错误与网关错误两类，文案与扣卡结果共用同一套描述。
        let (status, code) = match &error {
            AdminError::Rejected { status, .. } if (400..500).contains(status) => {
                (*status, "host_rejected")
            }
            AdminError::Rejected { .. } => (502, "host_rejected"),
            AdminError::Dispatch => (502, "host_dispatch"),
            AdminError::Invalid => (502, "host_invalid"),
            AdminError::Encoding => (500, "encoding"),
        };
        Self::new(status, code, reset::rejection_message(error))
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(_: serde_json::Error) -> Self {
        Self::new(500, "encoding", "管理接口响应编码失败")
    }
}

pub(super) fn json_reply(body: &impl Serialize) -> ApiResult {
    encode(200, body).map_err(|_| ApiError::new(500, "encoding", "管理接口响应编码失败"))
}

fn encode(
    status: u16,
    body: &impl Serialize,
) -> Result<TypedReply<ManagementResponse>, PluginFault> {
    let payload = serde_json::to_vec(body)
        .map_err(|_| PluginFault::new(ErrorCode::Fault, "管理接口响应编码失败"))?;
    Ok(TypedReply::new(ManagementResponse {
        headers: Vec::new(),
        status,
        content_type: JSON_CONTENT_TYPE.to_owned(),
    })
    .with_payload(payload))
}
