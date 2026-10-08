//! 自动用卡：宿主每 30 秒调用一次维护入口，插件按账号设置判断是否消费重置卡。
//! 判定先用宿主已有额度观测，只有达到阈值的账号才访问上游。

pub(crate) mod records;
mod runner;
pub(crate) mod settings;
mod windows;

pub(crate) use runner::reconcile;
