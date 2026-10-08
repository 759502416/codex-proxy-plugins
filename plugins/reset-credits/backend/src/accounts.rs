//! 读取宿主账号目录中支持重置卡的 OpenAI 账号；额度为宿主已有观测，不访问上游。

use crate::host_admin::{self, Account, AccountPage, AdminError};
use gateway_plugin_sdk::client::HostClient;

const PAGE_SIZE: u32 = 200;
/// 防止宿主分页异常时无限翻页；10000 个账号已远超单实例常见规模。
const MAXIMUM_PAGES: u32 = 50;

pub(crate) enum ListError {
    Host(AdminError),
    TooManyAccounts,
}

pub(crate) async fn fetch_all(host: &HostClient) -> Result<Vec<Account>, ListError> {
    let mut accounts = Vec::new();
    let mut page = 1;
    loop {
        let result: AccountPage = host_admin::get(
            host,
            &format!("/api/admin/accounts?provider=openai&page={page}&pageSize={PAGE_SIZE}"),
        )
        .await
        .map_err(ListError::Host)?;
        // API Key 账号与不支持查询的账号不提供重置卡能力，直接排除。
        accounts.extend(
            result
                .items
                .into_iter()
                .filter(|account| account.capabilities.reset_credits),
        );
        if result.page.page >= result.page.total_pages {
            return Ok(accounts);
        }
        if page >= MAXIMUM_PAGES {
            return Err(ListError::TooManyAccounts);
        }
        page += 1;
    }
}
