use super::response::JSON_CONTENT_TYPE;
use gateway_plugin_sdk::call::management::{
    ManagementPage, ManagementRegistration, ManagementResource, ManagementRoute,
};

pub(crate) fn registration() -> ManagementRegistration {
    let get = |path: &str| ManagementRoute {
        method: "GET".to_owned(),
        path: path.to_owned(),
        request_content_types: Vec::new(),
        response_content_types: vec![JSON_CONTENT_TYPE.to_owned()],
    };
    let post = |path: &str| ManagementRoute {
        method: "POST".to_owned(),
        path: path.to_owned(),
        request_content_types: vec![JSON_CONTENT_TYPE.to_owned()],
        response_content_types: vec![JSON_CONTENT_TYPE.to_owned()],
    };
    ManagementRegistration {
        routes: vec![
            get("api/accounts"),
            post("api/credits"),
            post("api/reset"),
            post("api/discard"),
            post("api/auto"),
        ],
        resources: ["web/index.html", "web/app.js", "web/app.css"]
            .into_iter()
            .map(|path| ManagementResource {
                path: path.to_owned(),
                public: false,
            })
            .collect(),
        pages: vec![ManagementPage {
            id: "reset-credits".to_owned(),
            title: "额度重置卡".to_owned(),
            description: Some("批量或按额度阈值自动使用 OpenAI 账号的主动重置卡".to_owned()),
            entry: "web/index.html".to_owned(),
            icon: None,
        }],
        callbacks: Vec::new(),
    }
}
