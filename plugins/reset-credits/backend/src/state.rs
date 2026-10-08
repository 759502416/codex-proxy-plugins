//! 插件私有状态的通用读写；创建要求键不存在，更新必须提交读取到的版本。

use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::host::{StateGetRequest, StateGetResult, StatePutRequest, StatePutResult},
    client::{HostClient, SessionError},
};
use serde::{Serialize, de::DeserializeOwned};

/// 每个账号一条操作记录，手动与自动共用，保证同一账号同时只有一笔消费。
pub(crate) const ACCOUNTS: &str = "accounts";
/// 每个账号一条自动用卡记录：周期占用、最近结果与退避时间。
pub(crate) const AUTOMATION: &str = "automation";
/// 全部账号的自动用卡设置，单条记录便于每轮一次读取。
pub(crate) const SETTINGS: &str = "settings";
const SCHEMA_VERSION: u32 = 1;

pub(crate) struct Stored<T> {
    /// 记录不存在时为空，写入时按“仅创建”处理。
    pub version: Option<u64>,
    pub value: T,
}

pub(crate) async fn load<T: DeserializeOwned + Default>(
    host: &HostClient,
    namespace: &str,
    key: &str,
) -> Result<Stored<T>, PluginFault> {
    let result: StateGetResult = call(
        host,
        "host.state.get",
        &StateGetRequest {
            namespace: namespace.to_owned(),
            key: key.to_owned(),
        },
    )
    .await?;
    let Some(record) = result.record else {
        return Ok(Stored {
            version: None,
            value: T::default(),
        });
    };
    if record.schema_version != SCHEMA_VERSION {
        return Err(invalid_state());
    }
    Ok(Stored {
        version: Some(record.version),
        value: serde_json::from_value(record.value).map_err(|_| invalid_state())?,
    })
}

/// 版本已变化时返回 `Conflict`，调用方据此判断另一笔操作已经写入。
pub(crate) async fn save<T: Serialize>(
    host: &HostClient,
    namespace: &str,
    key: &str,
    expected_version: Option<u64>,
    value: &T,
) -> Result<u64, PluginFault> {
    let result: StatePutResult = call(
        host,
        "host.state.put",
        &StatePutRequest {
            namespace: namespace.to_owned(),
            key: key.to_owned(),
            value: serde_json::to_value(value).map_err(|_| invalid_state())?,
            expected_version,
        },
    )
    .await?;
    Ok(result.version)
}

async fn call<I: Serialize, O: DeserializeOwned>(
    host: &HostClient,
    method: &str,
    input: &I,
) -> Result<O, PluginFault> {
    let reply = host
        .call(
            method,
            serde_json::to_value(input).map_err(|_| invalid_callback())?,
            Vec::new(),
        )
        .await
        .map_err(SessionError::into_plugin_fault)?;
    serde_json::from_value(reply.result).map_err(|_| invalid_callback())
}

fn invalid_state() -> PluginFault {
    PluginFault::new(ErrorCode::Fault, "已保存的插件记录无效")
}

fn invalid_callback() -> PluginFault {
    PluginFault::new(ErrorCode::Fault, "宿主回调返回了无效响应")
}
