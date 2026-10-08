use serde_json::{Value, json};

use crate::support::{
    FakeHost, Peer, Reply, SHORT_TERM_KEY, WEEKLY_KEY, account_page, credit, credits, quota_facts,
    usage_account,
};

const ACCOUNTS: &str = "GET /api/admin/accounts";
const CREDITS: &str = "GET /api/admin/accounts/reset-credits";
const CONSUME: &str = "POST /api/admin/accounts/reset-credits";
const QUOTA_REFRESH: &str = "POST /api/admin/accounts/quota/refresh";
const CYCLE: i64 = 1_760_600_000_000;

fn enable(host: &mut FakeHost, short_term_threshold: Option<u8>) {
    host.put(
        "settings",
        "auto",
        json!({"accounts": {"a1": {"weeklyThreshold": 100, "shortTermThreshold": short_term_threshold}}}),
    );
}

fn exhausted_weekly(host: &mut FakeHost, reset_at: i64) {
    host.reply(
        ACCOUNTS,
        account_page(vec![usage_account("a1", 100.0, 20.0)]),
    );
    host.quota_refreshes.push_back(Some(quota_facts(
        "a1",
        &[
            (WEEKLY_KEY, 100.0, Some(reset_at)),
            (SHORT_TERM_KEY, 20.0, Some(reset_at - 1)),
        ],
    )));
}

fn successful_consume(host: &mut FakeHost) {
    host.reply(
        CREDITS,
        credits(
            1,
            json!([credit("c1", "available", Some("2026-11-01T00:00:00+00:00"))]),
        ),
    )
    .reply(CONSUME, Reply::Data(json!({"code": "reset"})))
    .reply(CREDITS, credits(0, json!([])))
    .reply(
        QUOTA_REFRESH,
        Reply::Data(json!({"account": usage_account("a1", 0.0, 0.0)})),
    );
}

fn automation(host: &FakeHost) -> &Value {
    host.automation("a1").unwrap()
}

/// 让退避立即到期，模拟下一轮之后的时间点。
fn expire_backoff(host: &mut FakeHost) {
    let mut record = automation(host).clone();
    record["retryAtMs"] = json!(0);
    host.put("automation", "a1", record);
}

#[tokio::test]
async fn without_enabled_accounts_nothing_reaches_the_host_api() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();

    peer.reconcile(&mut host).await;

    assert!(host.requests.is_empty());
    assert!(host.refreshed.is_empty());
}

#[tokio::test]
async fn cached_usage_below_threshold_does_not_query_upstream() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    // Spark 周窗口已触顶，但不是 Codex 主额度。
    host.reply(
        ACCOUNTS,
        account_page(vec![usage_account("a1", 80.0, 100.0)]),
    );

    peer.reconcile(&mut host).await;

    assert_eq!(host.routes(), [ACCOUNTS]);
    assert!(host.refreshed.is_empty());
    assert!(host.automation("a1").is_none());
}

#[tokio::test]
async fn exhausted_weekly_quota_consumes_one_card_and_claims_the_cycle() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    exhausted_weekly(&mut host, CYCLE);
    successful_consume(&mut host);

    peer.reconcile(&mut host).await;

    assert_eq!(host.refreshed, ["a1"]);
    assert_eq!(
        host.routes(),
        [ACCOUNTS, CREDITS, CONSUME, CREDITS, QUOTA_REFRESH]
    );
    assert_eq!(host.consume_bodies()[0]["creditId"], "c1");
    let record = automation(&host);
    assert_eq!(record["cycleResetAtMs"], CYCLE);
    assert_eq!(record["last"]["outcome"], "reset");
    assert_eq!(record["last"]["trigger"], "weekly");
    assert_eq!(record["retryAtMs"], json!(null));
    assert_eq!(host.record("a1").unwrap()["last"]["outcome"], "reset");
}

#[tokio::test]
async fn the_same_cycle_never_gets_a_second_card() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    host.put(
        "automation",
        "a1",
        json!({"cycleResetAtMs": CYCLE, "last": null, "retryAtMs": null}),
    );
    // 上游仍报告同一周期触顶，例如重置刚生效前的观测。
    exhausted_weekly(&mut host, CYCLE + 1_000);

    peer.reconcile(&mut host).await;

    assert_eq!(host.routes(), [ACCOUNTS]);
    assert_eq!(automation(&host)["last"]["outcome"], "cycle_used");
    assert_eq!(automation(&host)["cycleResetAtMs"], CYCLE);
    assert!(automation(&host)["retryAtMs"].is_u64());

    // 退避期内不再访问上游。
    host.reply(
        ACCOUNTS,
        account_page(vec![usage_account("a1", 100.0, 20.0)]),
    );
    peer.reconcile(&mut host).await;
    assert_eq!(host.refreshed, ["a1"]);
}

#[tokio::test]
async fn a_new_weekly_cycle_can_use_another_card() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    host.put(
        "automation",
        "a1",
        json!({"cycleResetAtMs": CYCLE, "last": null, "retryAtMs": null}),
    );
    let next_cycle = CYCLE + 7 * 86_400_000;
    exhausted_weekly(&mut host, next_cycle);
    successful_consume(&mut host);

    peer.reconcile(&mut host).await;

    assert_eq!(host.consume_bodies().len(), 1);
    assert_eq!(automation(&host)["cycleResetAtMs"], next_cycle);
}

#[tokio::test]
async fn fresh_upstream_usage_below_threshold_skips_consumption() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    host.reply(
        ACCOUNTS,
        account_page(vec![usage_account("a1", 100.0, 20.0)]),
    );
    host.quota_refreshes
        .push_back(Some(quota_facts("a1", &[(WEEKLY_KEY, 3.0, Some(CYCLE))])));

    peer.reconcile(&mut host).await;

    assert_eq!(host.routes(), [ACCOUNTS]);
    assert!(host.automation("a1").is_none());
}

#[tokio::test]
async fn no_credit_releases_the_cycle_and_backs_off() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    exhausted_weekly(&mut host, CYCLE);
    host.reply(CREDITS, credits(0, json!([])));

    peer.reconcile(&mut host).await;

    assert!(host.consume_bodies().is_empty());
    let record = automation(&host);
    assert_eq!(record["last"]["outcome"], "no_credit");
    assert_eq!(record["cycleResetAtMs"], json!(null));
    assert!(record["retryAtMs"].is_u64());
}

#[tokio::test]
async fn unknown_result_is_confirmed_next_round_with_the_same_request() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    exhausted_weekly(&mut host, CYCLE);
    host.reply(
        CREDITS,
        credits(
            1,
            json!([credit("c1", "available", Some("2026-11-01T00:00:00+00:00"))]),
        ),
    )
    .reply(CONSUME, Reply::Fault);

    peer.reconcile(&mut host).await;

    assert_eq!(automation(&host)["last"]["outcome"], "unknown");
    assert_eq!(automation(&host)["cycleResetAtMs"], CYCLE);
    assert!(host.record("a1").unwrap()["pending"].is_object());
    let first = host.consume_bodies()[0].clone();

    expire_backoff(&mut host);
    exhausted_weekly(&mut host, CYCLE);
    host.reply(CONSUME, Reply::Data(json!({"code": "already_redeemed"})))
        .reply(CREDITS, credits(0, json!([])))
        .reply(
            QUOTA_REFRESH,
            Reply::Data(json!({"account": usage_account("a1", 0.0, 0.0)})),
        );
    peer.reconcile(&mut host).await;

    // 重试不重新查卡，原样复用幂等键和卡片 ID。
    assert_eq!(host.consume_bodies(), [&first, &first]);
    assert_eq!(automation(&host)["last"]["outcome"], "reset");
    assert_eq!(host.record("a1").unwrap()["pending"], json!(null));
}

#[tokio::test]
async fn switching_off_during_the_check_cancels_consumption() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    host.settings_disabled_after_reads = Some(1);
    exhausted_weekly(&mut host, CYCLE);

    peer.reconcile(&mut host).await;

    assert_eq!(host.routes(), [ACCOUNTS]);
    assert!(host.automation("a1").is_none());
}

#[tokio::test]
async fn short_term_quota_only_triggers_when_configured() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    host.reply(
        ACCOUNTS,
        account_page(vec![usage_account("a1", 40.0, 100.0)]),
    );

    peer.reconcile(&mut host).await;
    assert!(host.refreshed.is_empty());

    enable(&mut host, Some(100));
    host.reply(
        ACCOUNTS,
        account_page(vec![usage_account("a1", 40.0, 100.0)]),
    );
    host.quota_refreshes.push_back(Some(quota_facts(
        "a1",
        &[
            (WEEKLY_KEY, 40.0, Some(CYCLE)),
            (SHORT_TERM_KEY, 100.0, Some(CYCLE - 1)),
        ],
    )));
    successful_consume(&mut host);
    peer.reconcile(&mut host).await;

    assert_eq!(host.consume_bodies().len(), 1);
    assert_eq!(automation(&host)["last"]["trigger"], "short_term");
    // 周期仍以周额度重置时间标识。
    assert_eq!(automation(&host)["cycleResetAtMs"], CYCLE);
}

#[tokio::test]
async fn refresh_failure_is_recorded_without_consuming() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    enable(&mut host, None);
    host.reply(
        ACCOUNTS,
        account_page(vec![usage_account("a1", 100.0, 20.0)]),
    );
    host.quota_refreshes.push_back(None);

    peer.reconcile(&mut host).await;

    assert_eq!(host.routes(), [ACCOUNTS]);
    assert_eq!(automation(&host)["last"]["outcome"], "failed");
    assert_eq!(automation(&host)["last"]["code"], "quota_refresh_failed");
}

#[tokio::test]
async fn auto_settings_can_be_enabled_and_disabled_in_batches() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();

    let (status, body) = peer
        .api(
            &mut host,
            "POST",
            "api/auto",
            Some(json!({
                "accountIds": ["a1", "a2", "a1"],
                "settings": {"weeklyThreshold": 95, "shortTermThreshold": null},
            })),
        )
        .await;
    assert_eq!((status, body), (200, json!({"updated": 2})));
    assert_eq!(
        host.get("settings", "auto").unwrap()["accounts"]["a2"],
        json!({"weeklyThreshold": 95, "shortTermThreshold": null})
    );

    let (status, _) = peer
        .api(
            &mut host,
            "POST",
            "api/auto",
            Some(json!({"accountIds": ["a1"], "settings": null})),
        )
        .await;
    assert_eq!(status, 200);
    let accounts = &host.get("settings", "auto").unwrap()["accounts"];
    assert!(accounts.get("a1").is_none());
    assert!(accounts.get("a2").is_some());

    let (status, _) = peer
        .api(
            &mut host,
            "POST",
            "api/auto",
            Some(json!({
                "accountIds": ["a1"],
                "settings": {"weeklyThreshold": 0, "shortTermThreshold": null},
            })),
        )
        .await;
    assert_eq!(status, 400);
}
