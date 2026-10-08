use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

use codex_proxy_plugin_reset_credits::{PLUGIN_ID, manifest, plugin};
use gateway_plugin_sdk::{
    CallContext, ErrorCode, Frame, Handshake, Message, PROTOCOL_VERSION, PluginFault, Stage,
    client::{PluginSession, SessionConfig, read_frame, write_frame},
};
use serde_json::{Value, json};
use tokio::{
    io::{DuplexStream, ReadHalf, WriteHalf},
    task::JoinHandle,
};

const PIPE_BUFFER_BYTES: usize = 1024 * 1024;
pub type CallbackReply = Result<(Value, Vec<u8>), PluginFault>;

pub const WEEKLY_KEY: &str = "codex:604800s";
pub const SHORT_TERM_KEY: &str = "codex:18000s";
const SPARK_WEEKLY_KEY: &str = "codex_bengalfox:604800s";

// 从公开会话入口验证真实处理器；只替换宿主资源，不复制生产模块或扩大可见性。
pub struct Peer {
    reader: ReadHalf<DuplexStream>,
    writer: WriteHalf<DuplexStream>,
    task: JoinHandle<()>,
    next_id: u64,
}

impl Peer {
    pub async fn start() -> Self {
        let (host, transport) = tokio::io::duplex(PIPE_BUFFER_BYTES * 2);
        let (reader, writer) = tokio::io::split(transport);
        let task = tokio::spawn(async move {
            let session = PluginSession::accept(reader, writer, SessionConfig::default())
                .await
                .unwrap();
            session.run(plugin().unwrap()).await.unwrap();
        });
        let (reader, writer) = tokio::io::split(host);
        let mut peer = Self {
            reader,
            writer,
            task,
            next_id: 0,
        };
        let manifest = manifest().unwrap();
        peer.send(Frame::control(Message::Hello {
            handshake: Handshake {
                protocol_version: PROTOCOL_VERSION,
                artifact_sha256: "a".repeat(64),
                plugin_id: PLUGIN_ID.to_owned(),
                instance_id: "test-instance".to_owned(),
                generation: 1,
                incarnation: "test-session".to_owned(),
                configuration: json!({}),
                contributes: manifest.contributes,
            },
        }))
        .await;
        assert!(matches!(
            peer.receive().await.message,
            Message::Ready { .. }
        ));
        peer
    }

    pub async fn call_with(
        &mut self,
        method: &str,
        stage: Stage,
        params: Value,
        payload: Vec<u8>,
        mut callback: impl FnMut(&str, &Value, &[u8]) -> CallbackReply,
    ) -> Frame {
        self.next_id += 1;
        let id = self.next_id * 2 - 1;
        self.send(Frame {
            message: Message::Call {
                id,
                method: method.to_owned(),
                context: CallContext {
                    call_id: id,
                    instance_id: "test-instance".to_owned(),
                    generation: 1,
                    incarnation: "test-session".to_owned(),
                    stage,
                    timeout_ms: 10_000,
                    resource_stream: false,
                    resource_scope_id: "test-scope".to_owned(),
                    request_id: None,
                    attempt_id: None,
                    account_id: None,
                    credential_revision: None,
                },
                params,
            },
            payload,
        })
        .await;
        loop {
            let frame = self.receive().await;
            match frame.message {
                Message::Callback {
                    id: callback_id,
                    parent_id,
                    method,
                    params,
                } => {
                    assert_eq!(parent_id, id);
                    let response = match callback(&method, &params, &frame.payload) {
                        Ok((result, payload)) => Frame {
                            message: Message::Result {
                                id: callback_id,
                                result,
                            },
                            payload,
                        },
                        Err(error) => Frame::control(Message::Error {
                            id: callback_id,
                            error,
                        }),
                    };
                    self.send(response).await;
                }
                Message::Result { id: reply_id, .. } | Message::Error { id: reply_id, .. } => {
                    assert_eq!(reply_id, id);
                    return frame;
                }
                _ => panic!("出现未预期的插件消息：{frame:?}"),
            }
        }
    }

    pub async fn api(
        &mut self,
        host: &mut FakeHost,
        method: &str,
        path: &str,
        data: Option<Value>,
    ) -> (u64, Value) {
        let params = json!({
            "method": method, "path": path, "query": "",
            "content_type": data.as_ref().map(|_| "application/json"),
        });
        let payload = data.map_or_else(Vec::new, |value| serde_json::to_vec(&value).unwrap());
        let frame = self
            .call_with(
                "management.handle",
                Stage::Management,
                params,
                payload,
                |method, params, payload| host.callback(method, params, payload),
            )
            .await;
        let result = result(&frame);
        (
            result["status"].as_u64().unwrap(),
            serde_json::from_slice(&frame.payload).unwrap(),
        )
    }

    /// 维护入口始终返回空结果；单个账号的失败只写入自动用卡记录。
    pub async fn reconcile(&mut self, host: &mut FakeHost) {
        let frame = self
            .call_with(
                "plugin.reconcile",
                Stage::Maintenance,
                json!({}),
                Vec::new(),
                |method, params, payload| host.callback(method, params, payload),
            )
            .await;
        assert_eq!(result(&frame), &json!({}));
    }

    pub async fn send(&mut self, frame: Frame) {
        write_frame(&mut self.writer, &frame).await.unwrap();
    }

    pub async fn receive(&mut self) -> Frame {
        tokio::time::timeout(Duration::from_secs(10), read_frame(&mut self.reader))
            .await
            .expect("插件响应超时")
            .unwrap()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn result(frame: &Frame) -> &Value {
    let Message::Result { result, .. } = &frame.message else {
        panic!("预期成功响应，实际收到：{frame:?}");
    };
    result
}

/// 管理接口的一次预置回复。
pub enum Reply {
    Data(Value),
    Error {
        status: u16,
        code: u32,
    },
    /// 分派本身失败，模拟连接中断或取消。
    Fault,
}

/// 内存中的宿主：私有状态按命名空间与键保存并校验版本，管理接口按路由依次回放预置回复。
#[derive(Default)]
pub struct FakeHost {
    pub state: BTreeMap<(String, String), (u64, Value)>,
    pub replies: BTreeMap<String, VecDeque<Reply>>,
    /// 依次回放的额度刷新结果；`None` 表示刷新失败。
    pub quota_refreshes: VecDeque<Option<Value>>,
    /// 实际收到的管理请求：`方法 路径?查询` 与 JSON 正文。
    pub requests: Vec<(String, Value)>,
    /// 实际刷新额度的账号。
    pub refreshed: Vec<String>,
    /// 下一次写入状态时强制返回版本冲突。
    pub conflict_next_put: bool,
    /// 读取自动用卡设置超过该次数后返回空设置，模拟管理员中途关闭。
    pub settings_disabled_after_reads: Option<usize>,
    settings_reads: usize,
    bodies: BTreeMap<String, Option<Vec<u8>>>,
}

impl FakeHost {
    pub fn reply(&mut self, route: &str, reply: Reply) -> &mut Self {
        self.replies
            .entry(route.to_owned())
            .or_default()
            .push_back(reply);
        self
    }

    pub fn put(&mut self, namespace: &str, key: &str, value: Value) {
        let entry = (namespace.to_owned(), key.to_owned());
        let version = self.state.get(&entry).map_or(1, |(version, _)| version + 1);
        self.state.insert(entry, (version, value));
    }

    pub fn get(&self, namespace: &str, key: &str) -> Option<&Value> {
        self.state
            .get(&(namespace.to_owned(), key.to_owned()))
            .map(|(_, value)| value)
    }

    pub fn record(&self, account_id: &str) -> Option<&Value> {
        self.get("accounts", account_id)
    }

    pub fn automation(&self, account_id: &str) -> Option<&Value> {
        self.get("automation", account_id)
    }

    pub fn routes(&self) -> Vec<&str> {
        self.requests
            .iter()
            .map(|(route, _)| route.split('?').next().unwrap())
            .collect()
    }

    pub fn consume_bodies(&self) -> Vec<&Value> {
        self.requests
            .iter()
            .filter(|(route, _)| route == "POST /api/admin/accounts/reset-credits")
            .map(|(_, body)| body)
            .collect()
    }

    pub fn callback(&mut self, method: &str, params: &Value, payload: &[u8]) -> CallbackReply {
        match method {
            "host.state.get" => {
                let namespace = params["namespace"].as_str().unwrap().to_owned();
                let key = params["key"].as_str().unwrap().to_owned();
                if namespace == "settings" {
                    self.settings_reads += 1;
                    if self
                        .settings_disabled_after_reads
                        .is_some_and(|reads| self.settings_reads > reads)
                    {
                        return Ok((json!({ "record": null }), Vec::new()));
                    }
                }
                let record = self.state.get(&(namespace, key)).map(|(version, value)| {
                    json!({"value": value, "version": version, "schema_version": 1})
                });
                Ok((json!({ "record": record }), Vec::new()))
            }
            "host.state.put" => {
                let entry = (
                    params["namespace"].as_str().unwrap().to_owned(),
                    params["key"].as_str().unwrap().to_owned(),
                );
                let current = self.state.get(&entry).map(|(version, _)| *version);
                let expected = params["expected_version"].as_u64();
                if std::mem::take(&mut self.conflict_next_put) || current != expected {
                    return Err(PluginFault::new(ErrorCode::Conflict, "version changed"));
                }
                let version = current.unwrap_or_default() + 1;
                self.state.insert(entry, (version, params["value"].clone()));
                Ok((json!({ "version": version }), Vec::new()))
            }
            "host.quota_observations.refresh" => {
                assert_eq!(params, &json!({}));
                let query: Value = serde_json::from_slice(payload).unwrap();
                self.refreshed
                    .push(query["account_id"].as_str().unwrap().to_owned());
                match self
                    .quota_refreshes
                    .pop_front()
                    .expect("缺少预置额度刷新结果")
                {
                    Some(facts) => Ok((json!({}), serde_json::to_vec(&facts).unwrap())),
                    None => Err(PluginFault::new(ErrorCode::Upstream, "refresh failed")),
                }
            }
            "host.log" => Ok((json!({ "recorded": true }), Vec::new())),
            "host.http.dispatch" => self.dispatch(params, payload),
            "host.http.body_read" => {
                let body = self
                    .bodies
                    .get_mut(params["handle"].as_str().unwrap())
                    .unwrap();
                Ok(match body.take() {
                    Some(bytes) => (json!({"eof": false, "trailers": null}), bytes),
                    None => (json!({"eof": true, "trailers": null}), Vec::new()),
                })
            }
            _ => panic!("出现未预期的宿主回调：{method}"),
        }
    }

    fn dispatch(&mut self, params: &Value, payload: &[u8]) -> CallbackReply {
        let uri = params["uri"].as_str().unwrap();
        let route = format!("{} {uri}", params["method"].as_str().unwrap());
        let body = if payload.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(payload).unwrap()
        };
        self.requests.push((route.clone(), body));
        let key = route.split('?').next().unwrap();
        let reply = self
            .replies
            .get_mut(key)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| panic!("缺少预置回复：{route}"));
        let (status, envelope) = match reply {
            Reply::Data(data) => (200, json!({"code": 200, "message": "ok", "data": data})),
            Reply::Error { status, code } => (
                status,
                json!({"code": code, "message": "宿主错误", "data": null}),
            ),
            Reply::Fault => return Err(PluginFault::new(ErrorCode::Upstream, "dispatch failed")),
        };
        let handle = format!("body-{}", self.bodies.len());
        self.bodies
            .insert(handle.clone(), Some(serde_json::to_vec(&envelope).unwrap()));
        Ok((
            json!({
                "status": status, "version": "HTTP/1.1", "headers": [],
                "body": {"kind": "handle", "handle": handle},
                "response": null, "session": false,
            }),
            Vec::new(),
        ))
    }
}

pub fn account(id: &str, reset_credits: bool) -> Value {
    account_with_windows(
        id,
        reset_credits,
        json!([
            window("other-window", "other", None, 3_600, 10.0),
            window(WEEKLY_KEY, "shortTerm", Some("codex"), 604_800, 100.0),
            window("monthly", "monthly", None, 2_592_000, 20.0),
        ]),
    )
}

/// Codex 主额度的周与 5 小时窗口，另附一个已触顶的 Spark 周窗口用于验证不参与判定。
pub fn usage_account(id: &str, weekly_used: f64, short_term_used: f64) -> Value {
    account_with_windows(
        id,
        true,
        json!([
            window(
                SHORT_TERM_KEY,
                "shortTerm",
                Some("codex"),
                18_000,
                short_term_used
            ),
            window(WEEKLY_KEY, "shortTerm", Some("codex"), 604_800, weekly_used),
            window(
                SPARK_WEEKLY_KEY,
                "shortTerm",
                Some("codex_bengalfox"),
                604_800,
                100.0
            ),
        ]),
    )
}

fn account_with_windows(id: &str, reset_credits: bool, windows: Value) -> Value {
    json!({
        "id": id,
        "name": format!("账号 {id}"),
        "email": format!("{id}@example.com"),
        "planTypeDisplay": "Plus",
        "status": "quota_exhausted",
        "enabled": true,
        "capabilities": {
            "quota": true, "quotaRefresh": reset_credits, "profile": true, "subscription": true,
            "avatar": true, "resetCredits": reset_credits, "consumeResetCredit": reset_credits,
        },
        "quota": {
            "refreshedAtDisplay": "2026-10-08 12:00",
            "limitReached": true,
            "rateLimitedUntil": null,
            "rateLimitRecoveryDisplay": null,
            "rateLimitReason": null,
            "recoveryProbeRequired": false,
            "windows": windows,
            "credits": null,
        },
    })
}

fn window(key: &str, group: &str, limit_id: Option<&str>, seconds: u64, used: f64) -> Value {
    json!({
        "key": key, "group": group, "limitId": limit_id, "limitName": null, "role": "primary",
        "windowSeconds": seconds, "labelDisplay": key, "windowLabelDisplay": key,
        "usedPercent": used, "usedPercentDisplay": format!("{used}%"),
        "limitReached": used >= 100.0, "resetAtDisplay": "10-10 08:00",
    })
}

pub fn account_page(accounts: Vec<Value>) -> Reply {
    Reply::Data(json!({
        "items": accounts,
        "page": {"page": 1, "pageSize": 200, "total": 1, "totalPages": 1},
        "summary": {},
    }))
}

/// 刷新后的额度观测；未列出的窗口视为缺失。
pub fn quota_facts(account_id: &str, windows: &[(&str, f64, Option<i64>)]) -> Value {
    json!({
        "schema_version": 1,
        "account_id": account_id,
        "observed_at_ms": 1_760_000_000_000_i64,
        "windows": windows.iter().map(|(key, used, reset_at)| json!({
            "key": key, "window_seconds": null, "used_percent": used, "reset_at_ms": reset_at,
        })).collect::<Vec<_>>(),
    })
}

pub fn credits(available_count: u64, credits: Value) -> Reply {
    Reply::Data(json!({ "availableCount": available_count, "credits": credits }))
}

pub fn credit(id: &str, status: &str, expires_at: Option<&str>) -> Value {
    json!({
        "id": id, "status": status, "title": null, "resetType": "codex_rate_limits",
        "expiresAt": expires_at,
        "expiresAtDisplay": expires_at.map(|value| format!("展示 {value}")),
    })
}
