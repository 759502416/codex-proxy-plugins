use serde_json::json;

use crate::support::{FakeHost, Peer, Reply, account, credit, credits};

#[tokio::test]
async fn list_pages_through_openai_accounts_and_keeps_only_reset_capable_ones() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(
        "GET /api/admin/accounts",
        Reply::Data(json!({
            "items": [account("a1", true), account("key-1", false)],
            "page": {"page": 1, "pageSize": 200, "total": 3, "totalPages": 2},
            "summary": {},
        })),
    )
    .reply(
        "GET /api/admin/accounts",
        Reply::Data(json!({
            "items": [account("a2", true)],
            "page": {"page": 2, "pageSize": 200, "total": 3, "totalPages": 2},
            "summary": {},
        })),
    );
    host.put(
        "accounts",
        "a2",
        json!({
            "pending": {"redeemRequestId": "8fbf302d-11df-4bd5-82e4-08e4b3df7874", "creditId": "c1", "startedAtMs": 42},
            "last": null,
        }),
    );
    host.put(
        "settings",
        "auto",
        json!({"accounts": {"a1": {"weeklyThreshold": 100, "shortTermThreshold": null}}}),
    );

    let (status, body) = peer.api(&mut host, "GET", "api/accounts", None).await;

    assert_eq!(status, 200);
    let accounts = body["accounts"].as_array().unwrap();
    assert_eq!(
        accounts
            .iter()
            .map(|value| value["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["a1", "a2"]
    );
    assert_eq!(
        host.requests
            .iter()
            .map(|(route, _)| route.as_str())
            .collect::<Vec<_>>(),
        [
            "GET /api/admin/accounts?provider=openai&page=1&pageSize=200",
            "GET /api/admin/accounts?provider=openai&page=2&pageSize=200",
        ]
    );
    // 与宿主摘要一致：去掉“其他”分组并按短期、月度排序。
    assert_eq!(
        accounts[0]["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|window| window["key"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["codex:604800s", "monthly"]
    );
    assert_eq!(accounts[0]["pending"], json!(null));
    assert_eq!(
        accounts[0]["auto"],
        json!({"weeklyThreshold": 100, "shortTermThreshold": null})
    );
    assert_eq!(accounts[1]["auto"], json!(null));
    // 页面只看到开始时间，幂等键和卡片 ID 不离开插件。
    assert_eq!(accounts[1]["pending"], json!({"startedAtMs": 42}));
}

#[tokio::test]
async fn credits_summary_reports_count_and_earliest_available_expiry_without_ids() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(
        "GET /api/admin/accounts/reset-credits",
        credits(
            2,
            json!([
                credit("late", "available", Some("2026-11-02T00:00:00+00:00")),
                credit("used", "redeemed", Some("2026-10-01T00:00:00+00:00")),
                credit("early", "available", Some("2026-10-20T00:00:00+00:00")),
            ]),
        ),
    );

    let (status, body) = peer
        .api(
            &mut host,
            "POST",
            "api/credits",
            Some(json!({"accountId": "acct a/1"})),
        )
        .await;

    assert_eq!(status, 200);
    assert_eq!(
        body,
        json!({"availableCount": 2, "earliestExpiresAtDisplay": "展示 2026-10-20T00:00:00+00:00"})
    );
    assert_eq!(
        host.requests[0].0,
        "GET /api/admin/accounts/reset-credits?accountId=acct%20a%2F1"
    );
}

#[tokio::test]
async fn credits_query_failure_keeps_host_status_class() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(
        "GET /api/admin/accounts/reset-credits",
        Reply::Error {
            status: 502,
            code: 50_201,
        },
    );

    let (status, body) = peer
        .api(
            &mut host,
            "POST",
            "api/credits",
            Some(json!({"accountId": "a1"})),
        )
        .await;

    assert_eq!(status, 502);
    assert_eq!(body["error"]["code"], "host_rejected");
}
