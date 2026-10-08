use serde_json::{Value, json};

use crate::support::{FakeHost, Peer, Reply, account, credit, credits};

const CREDITS: &str = "GET /api/admin/accounts/reset-credits";
const CONSUME: &str = "POST /api/admin/accounts/reset-credits";
const QUOTA_REFRESH: &str = "POST /api/admin/accounts/quota/refresh";

async fn reset(peer: &mut Peer, host: &mut FakeHost, resume: bool) -> (u64, Value) {
    peer.api(
        host,
        "POST",
        "api/reset",
        Some(json!({"accountId": "a1", "resume": resume})),
    )
    .await
}

fn two_credits() -> Reply {
    credits(
        2,
        json!([
            credit("late", "available", Some("2026-11-02T00:00:00+00:00")),
            credit("redeemed", "redeemed", Some("2026-10-01T00:00:00+00:00")),
            credit("early", "available", Some("2026-10-20T00:00:00.5+00:00")),
        ]),
    )
}

fn pending(host: &FakeHost) -> &Value {
    &host.record("a1").unwrap()["pending"]
}

fn is_uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && value
            .chars()
            .all(|character| character == '-' || matches!(character, '0'..='9' | 'a'..='f'))
}

#[tokio::test]
async fn account_without_credits_is_not_consumed() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, credits(0, json!([])));

    let (status, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(status, 200);
    assert_eq!(body["outcome"], "no_credit");
    assert_eq!(body["credits"]["availableCount"], 0);
    assert_eq!(host.routes(), [CREDITS]);
    assert_eq!(pending(&host), &json!(null));
    assert_eq!(host.record("a1").unwrap()["last"]["outcome"], "no_credit");
}

#[tokio::test]
async fn reset_targets_earliest_available_credit_and_reads_back_state() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, two_credits())
        .reply(
            CONSUME,
            Reply::Data(json!({"code": "reset", "credit": null})),
        )
        .reply(
            CREDITS,
            credits(
                1,
                json!([credit(
                    "late",
                    "available",
                    Some("2026-11-02T00:00:00+00:00")
                )]),
            ),
        )
        .reply(
            QUOTA_REFRESH,
            Reply::Data(json!({"account": account("a1", true)})),
        );

    let (status, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(status, 200);
    assert_eq!(body["outcome"], "reset");
    assert_eq!(body["warnings"], json!([]));
    assert_eq!(body["credits"]["availableCount"], 1);
    assert_eq!(body["account"]["id"], "a1");
    assert_eq!(host.routes(), [CREDITS, CONSUME, CREDITS, QUOTA_REFRESH]);
    let consume = host.consume_bodies()[0];
    assert_eq!(consume["accountId"], "a1");
    assert_eq!(consume["creditId"], "early");
    assert!(is_uuid_v4(consume["redeemRequestId"].as_str().unwrap()));
    assert_eq!(pending(&host), &json!(null));
    assert_eq!(host.record("a1").unwrap()["last"]["outcome"], "reset");
    assert_eq!(host.record("a1").unwrap()["last"]["code"], "reset");
}

#[tokio::test]
async fn listed_count_without_details_lets_upstream_choose() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, credits(1, json!([])))
        .reply(CONSUME, Reply::Data(json!({"code": "reset"})))
        .reply(CREDITS, credits(0, json!([])))
        .reply(
            QUOTA_REFRESH,
            Reply::Data(json!({"account": account("a1", true)})),
        );

    let (_, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(body["outcome"], "reset");
    assert!(host.consume_bodies()[0].get("creditId").is_none());
}

#[tokio::test]
async fn unknown_result_keeps_pending_and_resume_reuses_the_same_request() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, two_credits()).reply(
        CONSUME,
        Reply::Error {
            status: 502,
            code: 50_202,
        },
    );

    let (status, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(status, 200);
    assert_eq!(body["outcome"], "unknown");
    let first = host.consume_bodies()[0].clone();
    assert_eq!(pending(&host)["redeemRequestId"], first["redeemRequestId"]);
    assert_eq!(pending(&host)["creditId"], "early");

    // 未确认前不能发起新的一笔消费。
    let (status, body) = reset(&mut peer, &mut host, false).await;
    assert_eq!(status, 409);
    assert_eq!(body["error"]["code"], "pending_exists");

    host.reply(CONSUME, Reply::Data(json!({"code": "already_redeemed"})))
        .reply(CREDITS, credits(1, json!([])))
        .reply(
            QUOTA_REFRESH,
            Reply::Data(json!({"account": account("a1", true)})),
        );
    let (status, body) = reset(&mut peer, &mut host, true).await;

    assert_eq!(status, 200);
    assert_eq!(body["outcome"], "reset");
    assert_eq!(body["message"], "上次重置已完成");
    assert_eq!(host.consume_bodies()[1], &first);
    assert_eq!(pending(&host), &json!(null));
}

#[tokio::test]
async fn interrupted_dispatch_is_treated_as_unknown() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, two_credits())
        .reply(CONSUME, Reply::Fault);

    let (_, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(body["outcome"], "unknown");
    assert!(pending(&host).is_object());
}

#[tokio::test]
async fn explicit_rejection_of_first_attempt_clears_pending() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, two_credits()).reply(
        CONSUME,
        Reply::Error {
            status: 502,
            code: 50_201,
        },
    );

    let (status, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(status, 200);
    assert_eq!(body["outcome"], "failed");
    assert_eq!(pending(&host), &json!(null));
    assert_eq!(host.record("a1").unwrap()["last"]["outcome"], "failed");
}

#[tokio::test]
async fn rejected_retry_keeps_the_earlier_attempt_pending() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, two_credits())
        .reply(CONSUME, Reply::Fault)
        .reply(
            CONSUME,
            Reply::Error {
                status: 502,
                code: 50_201,
            },
        );
    reset(&mut peer, &mut host, false).await;

    let (_, body) = reset(&mut peer, &mut host, true).await;

    assert_eq!(body["outcome"], "failed");
    assert_eq!(body["warnings"], json!(["上次重置的结果仍待确认"]));
    assert!(pending(&host).is_object());
}

#[tokio::test]
async fn upstream_refusal_codes_are_not_counted_as_reset() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, two_credits())
        .reply(CONSUME, Reply::Data(json!({"code": "nothing_to_reset"})));

    let (_, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(body["outcome"], "not_executed");
    assert_eq!(body["message"], "当前额度窗口不需要重置");
    assert_eq!(host.routes(), [CREDITS, CONSUME]);
    assert_eq!(pending(&host), &json!(null));
    assert_eq!(
        host.record("a1").unwrap()["last"]["code"],
        "nothing_to_reset"
    );
}

#[tokio::test]
async fn already_redeemed_on_first_attempt_is_not_confirmation() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, two_credits())
        .reply(CONSUME, Reply::Data(json!({"code": "already_redeemed"})));

    let (_, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(body["outcome"], "not_executed");
}

#[tokio::test]
async fn concurrent_claim_does_not_send_consume() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, two_credits());
    host.conflict_next_put = true;

    let (status, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(status, 409);
    assert_eq!(body["error"]["code"], "busy");
    assert_eq!(host.routes(), [CREDITS]);
}

#[tokio::test]
async fn confirmed_reset_reports_read_back_failures_as_warnings() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();
    host.reply(CREDITS, two_credits())
        .reply(CONSUME, Reply::Data(json!({"code": "reset"})))
        .reply(CREDITS, Reply::Fault)
        .reply(
            QUOTA_REFRESH,
            Reply::Error {
                status: 502,
                code: 50_201,
            },
        );

    let (status, body) = reset(&mut peer, &mut host, false).await;

    assert_eq!(status, 200);
    assert_eq!(body["outcome"], "reset");
    assert_eq!(
        body["warnings"],
        json!([
            "最新重置卡数量加载失败",
            "最新额度加载失败，请稍后手动刷新额度"
        ])
    );
}

#[tokio::test]
async fn resume_and_discard_require_a_pending_operation() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();

    let (status, body) = reset(&mut peer, &mut host, true).await;
    assert_eq!(status, 409);
    assert_eq!(body["error"]["code"], "no_pending");

    host.reply(CREDITS, two_credits())
        .reply(CONSUME, Reply::Fault);
    reset(&mut peer, &mut host, false).await;
    let (status, body) = peer
        .api(
            &mut host,
            "POST",
            "api/discard",
            Some(json!({"accountId": "a1"})),
        )
        .await;

    assert_eq!(status, 200);
    assert_eq!(body, json!({"discarded": true}));
    assert_eq!(pending(&host), &json!(null));
    assert_eq!(host.record("a1").unwrap()["last"]["outcome"], "discarded");
}

#[tokio::test]
async fn invalid_account_id_is_rejected_before_any_host_call() {
    let mut peer = Peer::start().await;
    let mut host = FakeHost::default();

    let (status, _) = peer
        .api(
            &mut host,
            "POST",
            "api/reset",
            Some(json!({"accountId": ""})),
        )
        .await;

    assert_eq!(status, 400);
    assert!(host.requests.is_empty());
}
