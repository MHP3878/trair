// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Bots on the server (M9, step 4a): a bot writes only once its owner
//! vouched for it, the certificate goes out with its account, and nobody
//! else can vouch, take over or fake it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use trair_core::bots::{BotCert, request_code, vouch};

/// A certificate with the secret it came with.
type Voucher = (BotCert, Vec<u8>);
use trair_core::proofs::{sign_account_deletion, sign_login};
use trair_core::{Account, DeviceIdentity, SignedDeviceList};
use trair_server::{Db, Server, router};

const START: u64 = 1_800_000_000;

struct World {
    server: Arc<Server>,
    clock: Arc<AtomicU64>,
}

impl World {
    fn new() -> World {
        let clock = Arc::new(AtomicU64::new(START));
        let c = clock.clone();
        let server = Server::new(
            Db::in_memory().unwrap(),
            Arc::new(move || c.load(Ordering::SeqCst)),
        );
        World { server, clock }
    }

    fn now(&self) -> u64 {
        self.clock.load(Ordering::SeqCst)
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let req = req
            .body(body.map_or_else(Body::empty, |b| Body::from(serde_json::to_vec(&b).unwrap())))
            .unwrap();
        let res = router(self.server.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

struct Person {
    account: Account,
    device: DeviceIdentity,
    token: String,
}

impl Person {
    fn user(&self) -> &str {
        self.account.user()
    }

    async fn new(w: &World, user: &str, bot: bool) -> Person {
        let account = Account::generate(user).unwrap();
        let device = DeviceIdentity::generate().unwrap();
        let list = SignedDeviceList::first(&account, device.entry()).unwrap();
        let (status, body) = w
            .call(
                "POST",
                "/v1/accounts",
                None,
                Some(
                    json!({"user": user, "account_key": hex::encode(account.public_key()),
                    "device_list": list, "bot": bot}),
                ),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (_, c) = w
            .call(
                "POST",
                "/v1/login/challenge",
                None,
                Some(json!({"user": user, "device": device.id})),
            )
            .await;
        let challenge = c["challenge"].as_str().unwrap().to_string();
        let sig = sign_login(&device, user, &hex::decode(&challenge).unwrap()).unwrap();
        let (_, l) = w
            .call(
                "POST",
                "/v1/login",
                None,
                Some(json!({"user": user, "device": device.id, "challenge": challenge, "signature": hex::encode(sig)})),
            )
            .await;
        Person {
            account,
            device,
            token: l["token"].as_str().unwrap().to_string(),
        }
    }

    async fn vouch_for(&self, w: &World, (cert, secret): &Voucher) -> (StatusCode, Value) {
        w.call(
            "POST",
            "/v1/bots/vouch",
            Some(&self.token),
            Some(json!({"cert": cert, "secret": hex::encode(secret)})),
        )
        .await
    }

    /// As a bot: a new request code; the server learns the secret's hash.
    async fn ask(&self, w: &World) -> String {
        let (code, hash) = request_code(&self.account).unwrap();
        let (status, body) = w
            .call(
                "POST",
                "/v1/bots/request",
                Some(&self.token),
                Some(json!({"hash": hash})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        code
    }

    async fn write_to(&self, w: &World, to: &Person) -> (StatusCode, Value) {
        let body = base64::engine::general_purpose::STANDARD.encode(b"hallo");
        w.call(
            "POST",
            "/v1/messages",
            Some(&self.token),
            Some(json!({"to": [{"user": to.user(), "device": to.device.id}], "body": body})),
        )
        .await
    }

    async fn change_a_chat(&self, w: &World) -> StatusCode {
        let commit = base64::engine::general_purpose::STANDARD.encode(b"change");
        w.call(
            "POST",
            "/v1/commits",
            Some(&self.token),
            Some(
                json!({"group": "5a".repeat(16), "epoch": 0, "commit": commit, "to": [],
                "committers": [self.user()]}),
            ),
        )
        .await
        .0
    }

    async fn bot_info(&self, w: &World, of: &str) -> Value {
        let (status, body) = w
            .call(
                "GET",
                &format!("/v1/accounts/{of}"),
                Some(&self.token),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["bot"].clone()
    }
}

#[tokio::test]
async fn b01_a_bot_writes_once_its_owner_vouched_and_nobody_can_fake_that() {
    let w = World::new();
    let julian = Person::new(&w, "julian", false).await;
    let mallory = Person::new(&w, "mallory", false).await;
    let bot = Person::new(&w, "wetterbot", true).await;
    let robo = Person::new(&w, "robo", true).await;

    // Unconfirmed: logged in, but no messages, no chat changes, no
    // communities. Everyone sees it is a bot nobody vouched for yet.
    assert_eq!(
        bot.write_to(&w, &julian).await,
        (
            StatusCode::FORBIDDEN,
            json!({"ok": false, "error": "bot_unconfirmed"})
        )
    );
    assert_eq!(bot.change_a_chat(&w).await, StatusCode::FORBIDDEN);
    let (status, body) = w
        .call(
            "POST",
            "/v1/community",
            Some(&bot.token),
            Some(json!({"op": "create", "name": "Bots"})),
        )
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("bot"))
    );
    assert_eq!(
        julian.bot_info(&w, "wetterbot").await,
        json!({"cert": null})
    );
    assert_eq!(julian.bot_info(&w, "mallory").await, Value::Null);
    assert_eq!(julian.write_to(&w, &mallory).await.0, StatusCode::OK);

    let code = bot.ask(&w).await;
    let cert = vouch(&julian.account, &code, w.now()).unwrap();
    // Mallory cannot hand in Julian's certificate, nor one she signed in
    // Julian's name, nor a changed one of her own.
    assert_eq!(mallory.vouch_for(&w, &cert).await.1["error"], "not_owner");
    let mut claimed = vouch(&mallory.account, &code, w.now()).unwrap();
    claimed.0.owner = "julian".into();
    assert_eq!(julian.vouch_for(&w, &claimed).await.1["error"], "bad_cert");
    let mut changed = vouch(&mallory.account, &code, w.now()).unwrap();
    changed.0.made += 1;
    assert_eq!(mallory.vouch_for(&w, &changed).await.1["error"], "bad_cert");
    // Name and key of the bot are public: without the secret from the code
    // its operator handed out, nobody can vouch.
    let (guessed, _) = request_code(&bot.account).unwrap();
    let guessed = vouch(&mallory.account, &guessed, w.now()).unwrap();
    assert_eq!(
        mallory.vouch_for(&w, &guessed).await.1["error"],
        "bad_secret"
    );
    // A person is no bot; a code with another key does not fit the bot.
    let (person, _) = request_code(&mallory.account).unwrap();
    let person = vouch(&julian.account, &person, w.now()).unwrap();
    assert_eq!(
        julian.vouch_for(&w, &person).await.1["error"],
        "unknown_bot"
    );
    let other_key = Account::generate("wetterbot").unwrap();
    let (wrong, _) = request_code(&other_key).unwrap();
    let wrong = vouch(&julian.account, &wrong, w.now()).unwrap();
    assert_eq!(julian.vouch_for(&w, &wrong).await.1["error"], "bad_cert");
    // A person cannot ask like a bot; a bot does not own bots.
    let (status, body) = w
        .call(
            "POST",
            "/v1/bots/request",
            Some(&mallory.token),
            Some(json!({"hash": "ab".repeat(32)})),
        )
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("not_a_bot"))
    );
    let robo_code = robo.ask(&w).await;
    let by_bot = vouch(&bot.account, &robo_code, w.now()).unwrap();
    assert_eq!(bot.vouch_for(&w, &by_bot).await.1["error"], "bot_owner");

    // Julian vouches: the bot writes, and its certificate goes out.
    assert_eq!(julian.vouch_for(&w, &cert).await.0, StatusCode::OK);
    assert_eq!(bot.write_to(&w, &julian).await.0, StatusCode::OK);
    assert_ne!(bot.change_a_chat(&w).await, StatusCode::FORBIDDEN);
    assert_eq!(
        mallory.bot_info(&w, "wetterbot").await,
        json!({"cert": serde_json::to_value(&cert.0).unwrap()})
    );
    // The secret works once: Mallory, who saw the code later, cannot take
    // the bot over with it.
    let hers = vouch(&mallory.account, &code, w.now()).unwrap();
    assert_eq!(mallory.vouch_for(&w, &hers).await.1["error"], "bad_secret");
    // Still no communities in this step.
    let (status, _) = w
        .call(
            "POST",
            "/v1/community",
            Some(&bot.token),
            Some(json!({"op": "join", "code": "trairc:00"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Julian's account goes: the bot loses its certificate and falls silent.
    let now = w.now();
    let sig = hex::encode(sign_account_deletion(&julian.account, now).unwrap());
    let (status, _) = w
        .call(
            "POST",
            "/v1/accounts/delete",
            Some(&julian.token),
            Some(json!({"now": now, "signature": sig})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        mallory.bot_info(&w, "wetterbot").await,
        json!({"cert": null})
    );
    assert_eq!(bot.write_to(&w, &mallory).await.0, StatusCode::FORBIDDEN);
    // Only a new code from the bot's operator gives it a new owner.
    assert_eq!(mallory.vouch_for(&w, &hers).await.1["error"], "bad_secret");
    let fresh = bot.ask(&w).await;
    let hers = vouch(&mallory.account, &fresh, w.now()).unwrap();
    assert_eq!(mallory.vouch_for(&w, &hers).await.0, StatusCode::OK);
    assert_eq!(bot.write_to(&w, &mallory).await.0, StatusCode::OK);
}

#[tokio::test]
async fn b02_at_most_ten_bots_per_owner_and_deleted_bots_are_gone() {
    let w = World::new();
    let julian = Person::new(&w, "julian", false).await;
    let mut bots = Vec::new();
    for i in 0..11 {
        bots.push(Person::new(&w, &format!("bot-{i}"), true).await);
    }
    for (i, b) in bots.iter().enumerate() {
        let code = b.ask(&w).await;
        let cert = vouch(&julian.account, &code, w.now()).unwrap();
        let (status, body) = julian.vouch_for(&w, &cert).await;
        if i < 10 {
            assert_eq!(status, StatusCode::OK, "{body}");
        } else {
            assert_eq!(body["error"], "too_many_bots");
        }
    }
    // A bot that deletes itself makes room.
    let now = w.now();
    let sig = hex::encode(sign_account_deletion(&bots[0].account, now).unwrap());
    let (status, _) = w
        .call(
            "POST",
            "/v1/accounts/delete",
            Some(&bots[0].token),
            Some(json!({"now": now, "signature": sig})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let code = bots[10].ask(&w).await;
    let cert = vouch(&julian.account, &code, w.now()).unwrap();
    assert_eq!(julian.vouch_for(&w, &cert).await.0, StatusCode::OK);
}
