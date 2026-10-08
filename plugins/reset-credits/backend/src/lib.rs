//! 额度重置卡插件：通过宿主原生管理接口查询和消费 OpenAI 主动重置卡，
//! 支持页面批量操作和按额度阈值自动用卡。

mod accounts;
mod app;
mod automation;
mod credits;
mod host_admin;
mod management;
mod manifest;
mod operations;
mod reset;
mod state;

pub use app::plugin;
pub use manifest::{PLUGIN_ID, manifest};
