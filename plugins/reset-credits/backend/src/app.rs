use crate::{automation, management};
use gateway_plugin_sdk::client::{AuthorError, ComposedPlugin, PluginBuilder, methods};
use std::sync::{Arc, atomic::AtomicUsize};

/// 组装重置卡插件的处理器。
///
/// # 错误
///
/// 清单与处理器声明不一致时返回错误。
pub fn plugin() -> Result<ComposedPlugin, AuthorError> {
    // 维护轮次的起点在进程内保留，预算用完时下一轮从未处理的账号继续。
    let cursor = Arc::new(AtomicUsize::new(0));
    PluginBuilder::from_json(include_bytes!("../../plugin.json"))?
        .management(management::registration(), management::handle)?
        .on(methods::RECONCILE, move |call| {
            automation::reconcile(Arc::clone(&cursor), call)
        })?
        .build()
}
