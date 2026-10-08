//! 通过宿主内部分派调用原生管理接口；身份由宿主按插件实例签发，插件不保存管理员凭据。
//! 请求与响应字段沿用宿主管理 API 合同，解析时忽略新增字段。

use gateway_plugin_sdk::{
    call::middleware::{MiddlewareHeader, http::Version},
    client::{HostClient, HttpBody, HttpFrame, HttpRequest},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// 账号目录单页最多 200 个账号，完整视图远小于该上限。
const MAXIMUM_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
/// 低于管理调用 120 秒的总期限，给结果处理和状态写入留出时间。
const DISPATCH_TIMEOUT_MS: u64 = 60_000;
const JSON_CONTENT_TYPE: &[u8] = b"application/json";
const ADMIN_OK_CODE: u32 = 200;
/// 宿主无法确认不可逆消费是否完成。
pub(crate) const UPSTREAM_RESULT_UNKNOWN: u32 = 50_202;

pub(crate) enum AdminError {
    /// 分派、等待或读取响应失败；写操作的执行结果未知。
    Dispatch,
    /// 宿主返回了管理 API 的错误信封。
    Rejected {
        status: u16,
        code: u32,
        message: String,
    },
    /// 响应不符合管理 API 合同。
    Invalid,
    /// 请求正文未能编码，未发出请求。
    Encoding,
}

#[derive(Deserialize)]
struct Envelope {
    code: u32,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: serde_json::Value,
}

pub(crate) async fn get<T: DeserializeOwned>(
    host: &HostClient,
    uri: &str,
) -> Result<T, AdminError> {
    send(host, "GET", uri, None).await
}

pub(crate) async fn post<T: DeserializeOwned>(
    host: &HostClient,
    uri: &str,
    body: &impl Serialize,
) -> Result<T, AdminError> {
    let body = serde_json::to_vec(body).map_err(|_| AdminError::Encoding)?;
    send(host, "POST", uri, Some(body)).await
}

async fn send<T: DeserializeOwned>(
    host: &HostClient,
    method: &str,
    uri: &str,
    body: Option<Vec<u8>>,
) -> Result<T, AdminError> {
    let mut headers = vec![MiddlewareHeader {
        name: "accept".to_owned(),
        value: JSON_CONTENT_TYPE.to_vec(),
    }];
    if body.is_some() {
        headers.push(MiddlewareHeader {
            name: "content-type".to_owned(),
            value: JSON_CONTENT_TYPE.to_vec(),
        });
    }
    let mut response = host
        .dispatch_http(HttpRequest {
            settings: serde_json::Value::Null,
            method: method.to_owned(),
            uri: uri.to_owned(),
            version: Version::Http11,
            headers,
            timeout_ms: Some(DISPATCH_TIMEOUT_MS),
            body: body.map_or_else(HttpBody::empty, HttpBody::from_bytes),
        })
        .await
        .map_err(|_| AdminError::Dispatch)?;
    let status = response.status;
    let mut bytes = Vec::new();
    while let Some(frame) = response
        .body
        .read()
        .await
        .map_err(|_| AdminError::Dispatch)?
    {
        if let HttpFrame::Data(chunk) = frame {
            if bytes.len() + chunk.len() > MAXIMUM_RESPONSE_BYTES {
                // 超限只放弃读取，关闭失败不改变“响应无效”的结论。
                let _ = response.body.close().await;
                return Err(AdminError::Invalid);
            }
            bytes.extend(chunk);
        }
    }
    let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| AdminError::Invalid)?;
    if !(200..300).contains(&status) || envelope.code != ADMIN_OK_CODE {
        return Err(AdminError::Rejected {
            status,
            code: envelope.code,
            message: envelope.message,
        });
    }
    serde_json::from_value(envelope.data).map_err(|_| AdminError::Invalid)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountPage {
    pub items: Vec<Account>,
    pub page: PageMeta,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageMeta {
    pub page: u32,
    pub total_pages: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub plan_type_display: String,
    pub status: String,
    pub enabled: bool,
    pub capabilities: Capabilities,
    pub quota: Quota,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Capabilities {
    pub reset_credits: bool,
    pub consume_reset_credit: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Quota {
    pub refreshed_at_display: String,
    pub rate_limit_recovery_display: Option<String>,
    pub windows: Vec<QuotaWindow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaWindow {
    pub key: String,
    pub group: String,
    /// 上游限流桶 ID；Codex 主额度为 `codex`，Spark 等附加额度使用其他 ID。
    pub limit_id: Option<String>,
    pub window_seconds: Option<u64>,
    pub label_display: String,
    pub used_percent: Option<f64>,
    pub used_percent_display: String,
    pub reset_at_display: String,
    pub limit_reached: bool,
}

/// 查询参数值按 RFC 3986 非保留字符以外全部转义。
pub(crate) fn query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[derive(Deserialize)]
pub(crate) struct QuotaRefresh {
    pub account: Account,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResetCredits {
    pub available_count: u64,
    pub credits: Vec<ResetCredit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResetCredit {
    pub id: String,
    pub status: Option<String>,
    pub expires_at: Option<String>,
    pub expires_at_display: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConsumeRequest<'a> {
    pub account_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credit_id: Option<&'a str>,
    pub redeem_request_id: &'a str,
}

#[derive(Deserialize)]
pub(crate) struct ConsumeResult {
    pub code: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountIdBody<'a> {
    pub account_id: &'a str,
}
