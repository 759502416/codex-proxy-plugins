use codex_proxy_plugin_reset_credits::{PLUGIN_ID, manifest};
use gateway_plugin_sdk::Stage;
use serde_json::{Value, json};

use crate::support::Peer;

#[test]
fn manifest_derives_plugin_id_and_declares_management_with_maintenance() {
    let manifest = manifest().unwrap();
    assert_eq!(manifest.plugin_id().unwrap(), PLUGIN_ID);
    let contributes = serde_json::to_value(&manifest.contributes).unwrap();
    assert_eq!(
        contributes
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["maintenance", "management"]
    );
}

#[tokio::test]
async fn registration_exposes_routes_and_private_resources() {
    let mut peer = Peer::start().await;
    let frame = peer
        .call_with(
            "management.register",
            Stage::Registration,
            json!({}),
            Vec::new(),
            |method, _, _| panic!("出现未预期的宿主回调：{method}"),
        )
        .await;
    let registration: Value = serde_json::from_slice(&frame.payload).unwrap();
    let routes: Vec<_> = registration["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|route| {
            format!(
                "{} {}",
                route["method"].as_str().unwrap(),
                route["path"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        routes,
        [
            "GET api/accounts",
            "POST api/credits",
            "POST api/reset",
            "POST api/discard",
            "POST api/auto"
        ]
    );
    assert!(
        registration["resources"]
            .as_array()
            .unwrap()
            .iter()
            .all(|resource| resource["public"] == false)
    );
    assert_eq!(registration["pages"][0]["entry"], "web/index.html");
}
