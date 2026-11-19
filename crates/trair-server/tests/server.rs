// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The server end to end, with real Trair clients built from the core:
//! register, log in with the device key, chat with MLS through the
//! queue, pair a second device through the relay, withdraw it, clean up.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use trair_core::pairing::{Code, PairingGuest, PairingHost};
use trair_core::proofs::{sign_account_deletion, sign_login};
use trair_core::vault::{KdfParams, Vault};
use trair_core::{
    Account, Device, DeviceIdentity, Event, Incoming, KeyPackageOffer, Messenger, PendingCommit,
    SignedDeviceList,
};
use trair_server::{
    Db, IDLE_LIMIT, MAX_KEYPACKAGE, MAX_MESSAGE, MESSAGE_LIFETIME, PAIRING_LIFETIME,
    REGISTRATIONS_PER_HOUR, SESSION_LIFETIME, Server, router,
};

const START: u64 = 1_800_000_000;
const CHEAP: KdfParams = KdfParams {
    m_kib: 8,
    t: 1,
    p: 1,
};

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

    fn later(&self, seconds: u64) {
        self.clock.fetch_add(seconds, Ordering::SeqCst);
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
        self.call_from(None, method, path, token, body).await
    }

    async fn call_from(
        &self,
        ip: Option<[u8; 4]>,
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
        let mut req = req
            .body(body.map_or_else(Body::empty, |b| Body::from(serde_json::to_vec(&b).unwrap())))
            .unwrap();
        if let Some(ip) = ip {
            req.extensions_mut()
                .insert(ConnectInfo(SocketAddr::from((ip, 40000))));
        }
        let res = router(self.server.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn raw(&self, method: &str, path: &str, token: &str, body: &str) -> StatusCode {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        router(self.server.clone())
            .oneshot(req)
            .await
            .unwrap()
            .status()
    }
}

/// A Trair device as the app will be: account, device key, session.
struct Client {
    account: Account,
    device: DeviceIdentity,
    list: SignedDeviceList,
    token: String,
}

impl Client {
    fn user(&self) -> &str {
        self.account.user()
    }

    async fn register(w: &World, user: &str) -> Client {
        let account = Account::generate(user).unwrap();
        let device = DeviceIdentity::generate().unwrap();
        let list = SignedDeviceList::first(&account, device.entry()).unwrap();
        let (status, body) = w
            .call(
                "POST",
                "/v1/accounts",
                None,
                Some(json!({"user": user, "account_key": hex::encode(account.public_key()), "device_list": list})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let token = login(w, user, &device).await.expect("login");
        Client {
            account,
            device,
            list,
            token,
        }
    }

    fn mls(&self) -> Device {
        Device::from_identity(&self.account, &self.device).unwrap()
    }
}

async fn login(w: &World, user: &str, device: &DeviceIdentity) -> Result<String, StatusCode> {
    let (_, c) = w
        .call(
            "POST",
            "/v1/login/challenge",
            None,
            Some(json!({"user": user, "device": device.id})),
        )
        .await;
    let challenge = c["challenge"].as_str().unwrap().to_string();
    let signature = sign_login(device, user, &hex::decode(&challenge).unwrap()).unwrap();
    let (status, body) = w
        .call(
            "POST",
            "/v1/login",
            None,
            Some(json!({"user": user, "device": device.id, "challenge": challenge, "signature": hex::encode(signature)})),
        )
        .await;
    if status == StatusCode::OK {
        Ok(body["token"].as_str().unwrap().to_string())
    } else {
        Err(status)
    }
}

async fn send(w: &World, from: &str, to_user: &str, to_device: &str, bytes: &[u8]) -> StatusCode {
    w.call(
        "POST",
        "/v1/messages",
        Some(from),
        Some(json!({"to": [{"user": to_user, "device": to_device}], "body": B64.encode(bytes)})),
    )
    .await
    .0
}

async fn inbox(w: &World, token: &str) -> Vec<(i64, Vec<u8>)> {
    let (status, body) = w.call("GET", "/v1/messages", Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["id"].as_i64().unwrap(),
                B64.decode(m["body"].as_str().unwrap()).unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn s01_register_and_log_in_with_the_device_key() {
    let w = World::new();
    let alice = Client::register(&w, "local-alice").await;

    // The same name again, a list signed by someone else, a bad name.
    let (status, body) = w
        .call(
            "POST",
            "/v1/accounts",
            None,
            Some(json!({"user": "local-alice", "account_key": hex::encode(alice.account.public_key()), "device_list": alice.list})),
        )
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::CONFLICT, Some("user_taken"))
    );
    let mallory = Account::generate("local-mallory").unwrap();
    let (status, _) = w
        .call(
            "POST",
            "/v1/accounts",
            None,
            Some(json!({"user": "local-alice2", "account_key": hex::encode(mallory.public_key()), "device_list": alice.list})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = w
        .call(
            "POST",
            "/v1/accounts",
            None,
            Some(json!({"user": "Alice!", "account_key": "00", "device_list": alice.list})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Logged in: the account is readable with the token, not without.
    let (status, body) = w
        .call("GET", "/v1/accounts/local-alice", Some(&alice.token), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["account_key"], hex::encode(alice.account.public_key()));
    assert_eq!(
        w.call("GET", "/v1/accounts/local-alice", None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        w.call("GET", "/v1/accounts/local-alice", Some("abc"), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );

    // A stranger's device cannot log in as Alice.
    let stranger = DeviceIdentity::generate().unwrap();
    assert_eq!(
        login(&w, "local-alice", &stranger).await,
        Err(StatusCode::FORBIDDEN)
    );
}

#[tokio::test]
async fn s02_challenges_work_once_briefly_and_only_for_their_device() {
    let w = World::new();
    let alice = Client::register(&w, "local-alice").await;
    let user = "local-alice";
    let ask = |device: String| {
        let w = &w;
        async move {
            let (_, c) = w
                .call(
                    "POST",
                    "/v1/login/challenge",
                    None,
                    Some(json!({"user": user, "device": device})),
                )
                .await;
            c["challenge"].as_str().unwrap().to_string()
        }
    };
    let try_login = |challenge: String| {
        let w = &w;
        let sig = hex::encode(
            sign_login(&alice.device, user, &hex::decode(&challenge).unwrap()).unwrap(),
        );
        let id = alice.device.id.clone();
        async move {
            w.call(
                "POST",
                "/v1/login",
                None,
                Some(json!({"user": user, "device": id, "challenge": challenge, "signature": sig})),
            )
            .await
            .0
        }
    };
    let c = ask(alice.device.id.clone()).await;
    assert_eq!(try_login(c.clone()).await, StatusCode::OK);
    assert_eq!(try_login(c).await, StatusCode::FORBIDDEN, "used twice");
    let other = ask("d-00000000".into()).await;
    assert_eq!(
        try_login(other).await,
        StatusCode::FORBIDDEN,
        "made for another device"
    );
    let late = ask(alice.device.id.clone()).await;
    w.later(61);
    assert_eq!(try_login(late).await, StatusCode::FORBIDDEN, "expired");
    assert_eq!(
        try_login(hex::encode([7u8; 32])).await,
        StatusCode::FORBIDDEN,
        "made up"
    );
    // A real challenge for Alice's device, signed by another key.
    let fresh = ask(alice.device.id.clone()).await;
    let thief = DeviceIdentity::generate().unwrap();
    let forged = hex::encode(sign_login(&thief, user, &hex::decode(&fresh).unwrap()).unwrap());
    let (status, _) = w
        .call(
            "POST",
            "/v1/login",
            None,
            Some(json!({"user": user, "device": alice.device.id, "challenge": fresh, "signature": forged})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "signed by the wrong key");

    // Absurdly long names do not even get a challenge.
    let (status, _) = w
        .call(
            "POST",
            "/v1/login/challenge",
            None,
            Some(json!({"user": "x".repeat(65), "device": "d"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Sessions run out.
    w.later(SESSION_LIFETIME);
    assert_eq!(
        w.call("GET", "/v1/messages", Some(&alice.token), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn s03_two_accounts_chat_through_the_server_and_it_learns_nothing() {
    let w = World::new();
    let alice = Client::register(&w, "local-alice").await;
    let bob = Client::register(&w, "local-bob").await;
    let mut a = alice.mls();
    let mut b = bob.mls();
    // Strict mode stays on: both compared fingerprints in person.
    a.verify(bob.user(), bob.account.fingerprint());
    b.verify(alice.user(), alice.account.fingerprint());

    let packages: Vec<String> = b
        .key_packages(3)
        .unwrap()
        .iter()
        .map(|p| B64.encode(p))
        .collect();
    let (status, body) = w
        .call(
            "POST",
            "/v1/keypackages",
            Some(&bob.token),
            Some(json!({"packages": packages})),
        )
        .await;
    assert_eq!((status, body["stored"].as_u64()), (StatusCode::OK, Some(3)));

    let (_, claimed) = w
        .call(
            "POST",
            "/v1/keypackages/claim",
            Some(&alice.token),
            Some(json!({"user": "local-bob"})),
        )
        .await;
    let entry = &claimed["packages"][0];
    let offer = KeyPackageOffer {
        claimed_device: format!("local-bob:{}", entry["device"].as_str().unwrap()),
        bytes: B64.decode(entry["package"].as_str().unwrap()).unwrap(),
    };
    a.create_group("g").unwrap();
    let out = a.add_members("g", &[offer]).unwrap();
    a.confirm("g").unwrap();
    assert_eq!(
        send(
            &w,
            &alice.token,
            "local-bob",
            &bob.device.id,
            out.welcome.as_ref().unwrap()
        )
        .await,
        StatusCode::OK
    );

    let welcome = inbox(&w, &bob.token).await;
    assert_eq!(welcome.len(), 1);
    assert!(matches!(
        b.receive(&welcome[0].1).unwrap(),
        Event::Joined { .. }
    ));
    let (_, acked) = w
        .call(
            "POST",
            "/v1/messages/ack",
            Some(&bob.token),
            Some(json!({"ids": [welcome[0].0]})),
        )
        .await;
    assert_eq!(acked["deleted"], 1);
    assert!(
        inbox(&w, &bob.token).await.is_empty(),
        "fetched and acknowledged means gone"
    );

    let chat_text = "Treffen um acht am Brunnen";
    let sealed = b.encrypt("g", chat_text).unwrap();
    assert_eq!(
        send(&w, &bob.token, "local-alice", &alice.device.id, &sealed).await,
        StatusCode::OK
    );
    let got = inbox(&w, &alice.token).await;
    match a.receive(&got[0].1).unwrap() {
        Event::Message { text, from, .. } => {
            assert_eq!(text, chat_text);
            assert!(from.starts_with("local-bob:"));
        }
        other => panic!("{other:?}"),
    }
    // The server holds no readable text and no sender.
    assert!(!String::from_utf8_lossy(&got[0].1).contains("Brunnen"));
    assert_eq!(
        w.server.message_columns(),
        ["id", "user", "device", "body", "at"]
    );

    // Key packages are handed out once each.
    for expected in [1, 1, 0] {
        let (_, c) = w
            .call(
                "POST",
                "/v1/keypackages/claim",
                Some(&alice.token),
                Some(json!({"user": "local-bob"})),
            )
            .await;
        assert_eq!(c["packages"].as_array().unwrap().len(), expected);
    }
    // Bob cannot delete Alice's waiting messages.
    send(&w, &bob.token, "local-alice", &alice.device.id, b"x").await;
    let id = inbox(&w, &alice.token).await.last().unwrap().0;
    let (_, acked) = w
        .call(
            "POST",
            "/v1/messages/ack",
            Some(&bob.token),
            Some(json!({"ids": [id]})),
        )
        .await;
    assert_eq!(acked["deleted"], 0);
}

#[tokio::test]
async fn s04_long_poll_wakes_up_when_a_message_arrives() {
    let w = Arc::new(World::new());
    let alice = Client::register(&w, "local-alice").await;
    let bob = Client::register(&w, "local-bob").await;
    let waiting = {
        let w = w.clone();
        let token = bob.token.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            let (_, body) = w
                .call("GET", "/v1/messages?wait=20", Some(&token), None)
                .await;
            (
                started.elapsed(),
                body["messages"].as_array().unwrap().len(),
            )
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    send(&w, &alice.token, "local-bob", &bob.device.id, b"wach auf").await;
    let (took, count) = waiting.await.unwrap();
    assert_eq!(count, 1);
    assert!(
        took.as_secs() < 5,
        "woke up at once, not after 20 s: {took:?}"
    );
    // Nothing there: waits about the asked time and returns empty.
    let started = Instant::now();
    let (_, body) = w
        .call("GET", "/v1/messages?wait=1", Some(&alice.token), None)
        .await;
    assert!(body["messages"].as_array().unwrap().is_empty());
    assert!(started.elapsed().as_millis() >= 900);
}

/// Alice with a real vault, registered and logged in.
async fn alice_with_vault(w: &World) -> (Vault, String) {
    let pw = secret();
    let (vault, _) = Vault::create_with(&pw, json!({"name": "Alice"}), CHEAP).unwrap();
    let user = vault.account().user().to_string();
    let (status, body) = w
        .call(
            "POST",
            "/v1/accounts",
            None,
            Some(json!({"user": user, "account_key": hex::encode(vault.account().public_key()), "device_list": vault.device_list()})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = login(w, &user, vault.device()).await.unwrap();
    (vault, token)
}

fn secret() -> String {
    let mut b = [0u8; 20];
    getrandom::fill(&mut b).unwrap();
    b.iter().map(|x| char::from(b'a' + x % 26)).collect()
}

/// Pair a new device through the relay, as the app will. Returns the new
/// device's vault.
async fn pair_through_relay(w: &World, host: &mut Vault, token: &str) -> Vault {
    let (_, opened) = w.call("POST", "/v1/pairing", Some(token), None).await;
    let channel = opened["channel"].as_u64().unwrap() as u16;
    let (mut h, code, host_hello) = PairingHost::start(channel, Instant::now()).unwrap();
    let path = format!("/v1/pairing/{channel}");
    let (status, _) = w
        .call(
            "PUT",
            &format!("{path}/host"),
            Some(token),
            Some(json!({"hello": B64.encode(&host_hello)})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // The new device: no account, no session, only the typed code.
    let typed = Code::parse(&code.text()).unwrap();
    let (_, got) = w.call("GET", &format!("{path}/host"), None, None).await;
    let (mut g, guest_hello) = PairingGuest::start(&typed).unwrap();
    let offer = g
        .offer(&B64.decode(got["hello"].as_str().unwrap()).unwrap())
        .unwrap();
    let (status, _) = w
        .call(
            "PUT",
            &format!("{path}/guest"),
            None,
            Some(json!({"hello": B64.encode(&guest_hello), "offer": B64.encode(&offer)})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // The host finishes, sends the bundle and the new device list.
    let (_, got) = w
        .call("GET", &format!("{path}/guest"), Some(token), None)
        .await;
    let bundle = h
        .finish(
            &B64.decode(got["hello"].as_str().unwrap()).unwrap(),
            &B64.decode(got["offer"].as_str().unwrap()).unwrap(),
            host,
            Instant::now(),
        )
        .unwrap();
    w.call(
        "PUT",
        &format!("{path}/bundle"),
        Some(token),
        Some(json!({"bundle": B64.encode(&bundle)})),
    )
    .await;
    let (status, body) = w
        .call(
            "PUT",
            "/v1/devices",
            Some(token),
            Some(json!({"device_list": host.device_list()})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, got) = w.call("GET", &format!("{path}/bundle"), None, None).await;
    let joined = g
        .finish(&B64.decode(got["bundle"].as_str().unwrap()).unwrap())
        .unwrap();
    Vault::create_joined_with(&secret(), joined, CHEAP)
        .unwrap()
        .0
}

#[tokio::test]
async fn s05_pairing_a_second_device_through_the_relay() {
    let w = World::new();
    let (mut host, token) = alice_with_vault(&w).await;
    let laptop = pair_through_relay(&w, &mut host, &token).await;
    assert_eq!(laptop.account().fingerprint(), host.account().fingerprint());
    let user = host.account().user().to_string();
    let laptop_token = login(&w, &user, laptop.device())
        .await
        .expect("new device logs in");
    // The new device receives messages addressed to it.
    assert_eq!(
        send(&w, &token, &user, &laptop.device().id, b"hallo laptop").await,
        StatusCode::OK
    );
    assert_eq!(inbox(&w, &laptop_token).await.len(), 1);

    // One guest attempt per channel; strangers cannot read the host's side.
    let bob = Client::register(&w, "local-bob").await;
    let (_, opened) = w.call("POST", "/v1/pairing", Some(&token), None).await;
    let ch = opened["channel"].as_u64().unwrap();
    w.call(
        "PUT",
        &format!("/v1/pairing/{ch}/host"),
        Some(&token),
        Some(json!({"hello": B64.encode(b"h")})),
    )
    .await;
    let guess = json!({"hello": B64.encode(b"g"), "offer": B64.encode(b"o")});
    assert_eq!(
        w.call(
            "PUT",
            &format!("/v1/pairing/{ch}/guest"),
            None,
            Some(guess.clone())
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, body) = w
        .call("PUT", &format!("/v1/pairing/{ch}/guest"), None, Some(guess))
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::CONFLICT, Some("used"))
    );
    assert_eq!(
        w.call(
            "GET",
            &format!("/v1/pairing/{ch}/guest"),
            Some(&bob.token),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Channels expire with the code.
    w.later(PAIRING_LIFETIME + 1);
    assert_eq!(
        w.call("GET", &format!("/v1/pairing/{ch}/host"), None, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        w.call("GET", "/v1/pairing/9999/bundle", None, None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn s06_a_withdrawn_device_loses_everything_at_once() {
    let w = World::new();
    let (mut host, token) = alice_with_vault(&w).await;
    let laptop = pair_through_relay(&w, &mut host, &token).await;
    let user = host.account().user().to_string();
    let laptop_token = login(&w, &user, laptop.device()).await.unwrap();
    send(&w, &token, &user, &laptop.device().id, b"wartet noch").await;
    let old_list = host.device_list().clone();

    host.withdraw_device(&laptop.device().id).unwrap();
    let (status, body) = w
        .call(
            "PUT",
            "/v1/devices",
            Some(&token),
            Some(json!({"device_list": host.device_list()})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], json!([laptop.device().id]));
    assert_eq!(
        w.call("GET", "/v1/messages", Some(&laptop_token), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&w, &user, laptop.device()).await,
        Err(StatusCode::FORBIDDEN)
    );
    assert_eq!(
        send(&w, &token, &user, &laptop.device().id, b"x").await,
        StatusCode::NOT_FOUND
    );

    // The server cannot be talked into the old list again, or a forged one.
    let (status, _) = w
        .call(
            "PUT",
            "/v1/devices",
            Some(&token),
            Some(json!({"device_list": old_list})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let bob = Client::register(&w, "local-bob").await;
    let (status, _) = w
        .call(
            "PUT",
            "/v1/devices",
            Some(&token),
            Some(json!({"device_list": bob.list})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn s07_old_messages_idle_accounts_and_deleted_accounts_are_gone() {
    let w = World::new();
    let alice = Client::register(&w, "local-alice").await;
    let bob = Client::register(&w, "local-bob").await;
    send(&w, &alice.token, "local-bob", &bob.device.id, b"alt").await;
    w.later(MESSAGE_LIFETIME + 1);
    let cleaned = w.server.cleanup().unwrap();
    assert_eq!(cleaned.messages, 1);
    assert_eq!(cleaned.accounts, 0, "30 days are not idle yet");

    // Deleting needs the account key, a session alone is not enough.
    let bob_token = login(&w, "local-bob", &bob.device).await.unwrap();
    let alice_token = login(&w, "local-alice", &alice.device).await.unwrap();
    let now = w.now();
    let forged = hex::encode(sign_account_deletion(&alice.account, now).unwrap());
    let (status, _) = w
        .call(
            "POST",
            "/v1/accounts/delete",
            Some(&bob_token),
            Some(json!({"now": now, "signature": forged})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let stale = hex::encode(sign_account_deletion(&bob.account, now - 3600).unwrap());
    let (status, _) = w
        .call(
            "POST",
            "/v1/accounts/delete",
            Some(&bob_token),
            Some(json!({"now": now - 3600, "signature": stale})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let real = hex::encode(sign_account_deletion(&bob.account, now).unwrap());
    let (status, _) = w
        .call(
            "POST",
            "/v1/accounts/delete",
            Some(&bob_token),
            Some(json!({"now": now, "signature": real})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        w.call("GET", "/v1/accounts/local-bob", Some(&alice_token), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        login(&w, "local-bob", &bob.device).await,
        Err(StatusCode::FORBIDDEN)
    );

    // Six months without login: the account goes with all its data.
    w.later(IDLE_LIMIT + 1);
    assert_eq!(w.server.cleanup().unwrap().accounts, 1);
    assert_eq!(
        login(&w, "local-alice", &alice.device).await,
        Err(StatusCode::FORBIDDEN)
    );
}

#[tokio::test]
async fn s08_limits_and_strange_requests() {
    let w = World::new();
    let alice = Client::register(&w, "local-alice").await;
    let bob = Client::register(&w, "local-bob").await;
    let t = &alice.token;
    assert_eq!(
        send(
            &w,
            t,
            "local-bob",
            &bob.device.id,
            &vec![1u8; MAX_MESSAGE + 1]
        )
        .await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        send(&w, t, "local-nobody", "d-00000000", b"x").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&w, t, "local-bob", "d-00000000", b"x").await,
        StatusCode::NOT_FOUND
    );
    let (status, _) = w
        .call(
            "POST",
            "/v1/messages",
            Some(t),
            Some(json!({"to": [], "body": "eA=="})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = w
        .call(
            "POST",
            "/v1/keypackages",
            Some(t),
            Some(json!({"packages": [B64.encode(vec![0u8; MAX_KEYPACKAGE + 1])]})),
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, _) = w
        .call(
            "POST",
            "/v1/keypackages",
            Some(t),
            Some(json!({"packages": vec!["eA=="; 101]})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        w.raw("POST", "/v1/messages", t, "{kaputt")
            .await
            .is_client_error()
    );
    assert!(
        w.raw("POST", "/v1/messages/ack", t, r#"{"ids": [], "extra": 1}"#)
            .await
            .is_client_error()
    );
    assert!(
        w.raw("POST", "/v1/messages", t, &"x".repeat(2 * 1024 * 1024))
            .await
            .is_client_error(),
        "too big"
    );
}

#[tokio::test]
async fn s09_too_many_registrations_from_one_address_are_slowed_down() {
    let w = World::new();
    let mut statuses = Vec::new();
    for i in 0..REGISTRATIONS_PER_HOUR + 1 {
        let account = Account::generate(&format!("local-u{i}")).unwrap();
        let device = DeviceIdentity::generate().unwrap();
        let list = SignedDeviceList::first(&account, device.entry()).unwrap();
        let (status, _) = w
            .call_from(
                Some([203, 0, 113, 7]),
                "POST",
                "/v1/accounts",
                None,
                Some(json!({"user": account.user(), "account_key": hex::encode(account.public_key()), "device_list": list})),
            )
            .await;
        statuses.push(status);
    }
    assert!(
        statuses[..REGISTRATIONS_PER_HOUR]
            .iter()
            .all(|s| *s == StatusCode::OK)
    );
    assert_eq!(
        statuses[REGISTRATIONS_PER_HOUR],
        StatusCode::TOO_MANY_REQUESTS
    );
    // Another address, or an hour later, is fine again.
    w.later(3601);
    let account = Account::generate("local-spaeter").unwrap();
    let device = DeviceIdentity::generate().unwrap();
    let list = SignedDeviceList::first(&account, device.entry()).unwrap();
    let (status, _) = w
        .call_from(
            Some([203, 0, 113, 7]),
            "POST",
            "/v1/accounts",
            None,
            Some(json!({"user": account.user(), "account_key": hex::encode(account.public_key()), "device_list": list})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

// ------------------------------------------------- M4: chats and changes

/// A folder for a chat database that is removed when the test ends.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("trair-srv-{}-{n}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    /// A key that differs per folder (tests only; the app takes 32 random
    /// bytes from the vault).
    fn key(&self) -> [u8; 32] {
        use sha2::Digest;
        sha2::Sha256::digest(self.0.to_string_lossy().as_bytes()).into()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A Trair device with its chat database, as the app will use it.
struct ChatClient {
    c: Client,
    m: Messenger,
    _dir: TempDir,
}

impl ChatClient {
    async fn register(w: &World, user: &str) -> ChatClient {
        let c = Client::register(w, user).await;
        let dir = TempDir::new();
        let m = Messenger::open(
            &dir.0.join(trair_core::chats::CHATS_FILE),
            &dir.key(),
            &c.account,
            &c.device,
            &c.list.list,
            w.now(),
        )
        .unwrap();
        ChatClient { c, m, _dir: dir }
    }

    /// One-time key packages and a last-resort package to the server.
    async fn upload(&mut self, w: &World, n: usize) -> Value {
        let packages: Vec<String> = self
            .m
            .key_packages(n)
            .unwrap()
            .iter()
            .map(|p| B64.encode(p))
            .collect();
        let last = B64.encode(self.m.last_resort_key_package().unwrap());
        let (status, body) = w
            .call(
                "POST",
                "/v1/keypackages",
                Some(&self.c.token),
                Some(json!({"packages": packages, "last_resort": last})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn claim(&self, w: &World, user: &str) -> Vec<KeyPackageOffer> {
        let (status, body) = w
            .call(
                "POST",
                "/v1/keypackages/claim",
                Some(&self.c.token),
                Some(json!({"user": user})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["packages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| KeyPackageOffer {
                claimed_device: format!("{user}:{}", p["device"].as_str().unwrap()),
                bytes: B64.decode(p["package"].as_str().unwrap()).unwrap(),
            })
            .collect()
    }

    /// Send a change and apply the server's answer, as the app does.
    async fn push(&mut self, w: &World, p: &PendingCommit) -> (StatusCode, bool) {
        let (status, body) = w
            .call(
                "POST",
                "/v1/commits",
                Some(&self.c.token),
                Some(commit_body(p)),
            )
            .await;
        let applied = self
            .m
            .commit_answer(
                &p.chat,
                status == StatusCode::OK,
                body["taken"].as_str(),
                w.now(),
            )
            .unwrap();
        (status, applied)
    }

    async fn say(&mut self, w: &World, chat: &str, text: &str) {
        let to = targets(&self.m.recipients(chat).unwrap());
        let (bytes, _) = self.m.send(chat, text, w.now()).unwrap();
        let (status, body) = w
            .call(
                "POST",
                "/v1/messages",
                Some(&self.c.token),
                Some(json!({"to": to, "body": B64.encode(bytes)})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    /// Fetch, process and acknowledge everything waiting.
    async fn read(&mut self, w: &World) -> Vec<Incoming> {
        let mut out = Vec::new();
        let mut ids = Vec::new();
        for (id, bytes) in inbox(w, &self.c.token).await {
            out.push(self.m.receive(&bytes, w.now()).unwrap());
            ids.push(id);
        }
        let (status, _) = w
            .call(
                "POST",
                "/v1/messages/ack",
                Some(&self.c.token),
                Some(json!({"ids": ids})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        out
    }
}

/// `user:device` ids as the server wants them.
fn targets(ids: &[String]) -> Vec<Value> {
    ids.iter()
        .map(|id| {
            let (user, device) = id.split_once(':').unwrap();
            json!({"user": user, "device": device})
        })
        .collect()
}

fn commit_body(p: &PendingCommit) -> Value {
    json!({
        "group": p.chat,
        "epoch": p.epoch,
        "commit": B64.encode(&p.commit),
        "to": targets(&p.to),
        "welcome": p.welcome.as_ref().map(|w| B64.encode(w)),
        "welcome_to": targets(&p.welcome_to),
    })
}

#[tokio::test]
async fn s10_the_server_takes_one_change_per_chat_and_epoch() {
    let w = World::new();
    let alice = Client::register(&w, "local-alice").await;
    let bob = Client::register(&w, "local-bob").await;
    // A chat id in the core's format: 32 lowercase hex characters.
    let group = "5a".repeat(16);
    let group = group.as_str();
    let bob_dev = json!([{"user": bob.user(), "device": bob.device.id}]);
    let body =
        |epoch: u64, commit: &[u8], to: &Value, welcome: Option<&[u8]>, welcome_to: &Value| {
            json!({"group": group, "epoch": epoch, "commit": B64.encode(commit), "to": to,
               "welcome": welcome.map(|w| B64.encode(w)), "welcome_to": welcome_to})
        };
    let post = |b: Value| w.call("POST", "/v1/commits", Some(&alice.token), Some(b));

    // The first change of a new chat: the welcome goes to Bob.
    let (status, answer) = post(body(
        0,
        b"change-0",
        &json!([]),
        Some(b"welcome-0"),
        &bob_dev,
    ))
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["epoch"], 1);
    assert_eq!(inbox(&w, &bob.token).await.last().unwrap().1, b"welcome-0");

    // A second change for epoch 0: refused, with what was taken.
    let (status, answer) = post(body(0, b"other-0", &bob_dev, None, &json!([]))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(answer["error"], "wrong_epoch");
    assert_eq!(answer["epoch"], 1);
    assert_eq!(
        answer["taken"],
        hex::encode(<sha2::Sha256 as sha2::Digest>::digest(b"change-0"))
    );
    assert_eq!(inbox(&w, &bob.token).await.len(), 1, "nothing was queued");

    // A change for a device that does not exist: nothing at all happens.
    let ghost = json!([{"user": bob.user(), "device": "ghost"}]);
    let (status, _) = post(body(1, b"change-1", &ghost, None, &json!([]))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, answer) = post(body(1, b"change-1", &bob_dev, None, &json!([]))).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["epoch"], 2);
    assert_eq!(inbox(&w, &bob.token).await.last().unwrap().1, b"change-1");

    // Strange requests.
    for bad in [
        json!({"group": "G", "epoch": 2, "commit": B64.encode(b"x"), "to": []}),
        json!({"group": group.to_uppercase(), "epoch": 2, "commit": B64.encode(b"x"), "to": []}),
        body(2, b"x", &json!([]), Some(b"w"), &json!([])),
        body(2, b"x", &json!([]), None, &bob_dev),
        json!({"group": group, "epoch": u64::MAX, "commit": B64.encode(b"x"), "to": []}),
        json!({"group": group, "epoch": 2, "commit": "", "to": []}),
        json!({"group": group, "epoch": 2, "commit": B64.encode(b"x"), "to": [], "sender": "alice"}),
    ] {
        let (status, _) = post(bad.clone()).await;
        assert!(status.is_client_error(), "{bad} gave {status}");
    }
    let (status, _) = w
        .call(
            "POST",
            "/v1/commits",
            None,
            Some(body(2, b"x", &json!([]), None, &json!([]))),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // The server never stores who is in a chat, and forgets idle chats.
    let tables = w.server.db_tables();
    assert!(tables.iter().all(|t| !t.contains("member")), "{tables:?}");
    w.later(IDLE_LIMIT + 1);
    assert!(w.server.cleanup().unwrap().groups >= 1);
}

#[tokio::test]
async fn s11_two_people_chat_through_the_server_with_the_real_database() {
    let w = World::new();
    let mut alice = ChatClient::register(&w, "local-alice").await;
    let mut bob = ChatClient::register(&w, "local-bob").await;
    let up = bob.upload(&w, 3).await;
    assert_eq!(up["count"], 3);
    let (_, count) = w
        .call("GET", "/v1/keypackages", Some(&bob.c.token), None)
        .await;
    assert_eq!(count["count"], 3);

    // Codes exchanged; Alice starts the chat.
    let a_code = alice.m.contact_code();
    let b_code = bob.m.contact_code();
    alice.m.add_contact(&b_code, "Bob", w.now()).unwrap();
    bob.m.add_contact(&a_code, "Alice", w.now()).unwrap();
    let offers = alice.claim(&w, bob.c.user()).await;
    let (chat, p) = alice.m.start_chat(bob.c.user(), &offers, w.now()).unwrap();
    assert_eq!(alice.push(&w, &p).await, (StatusCode::OK, true));
    let got = bob.read(&w).await;
    assert!(matches!(&got[..], [Incoming::Chat { chat: c, .. }] if c.id == chat.id));

    alice.say(&w, &chat.id, "Hallo über den Server").await;
    bob.say(&w, &chat.id, "angekommen").await;
    assert!(
        matches!(&bob.read(&w).await[..], [Incoming::Message(m)] if m.text == "Hallo über den Server")
    );
    assert!(matches!(&alice.read(&w).await[..], [Incoming::Message(m)] if m.text == "angekommen"));

    // Both change keys at once: the server takes Alice's, Bob's is
    // thrown away and he tries again after reading hers.
    let pa = alice.m.update_keys(&chat.id).unwrap();
    let pb = bob.m.update_keys(&chat.id).unwrap();
    assert_eq!(alice.push(&w, &pa).await, (StatusCode::OK, true));
    assert_eq!(bob.push(&w, &pb).await, (StatusCode::CONFLICT, false));
    bob.read(&w).await;
    let pb = bob.m.update_keys(&chat.id).unwrap();
    assert_eq!(bob.push(&w, &pb).await, (StatusCode::OK, true));
    alice.read(&w).await;
    assert_eq!(
        alice.m.epoch(&chat.id).unwrap(),
        bob.m.epoch(&chat.id).unwrap()
    );

    // Alice sends a change, the server takes it, Trair dies before the
    // answer. Sent again later, the server's hash shows it was hers.
    let pa = alice.m.update_keys(&chat.id).unwrap();
    let (status, _) = w
        .call(
            "POST",
            "/v1/commits",
            Some(&alice.c.token),
            Some(commit_body(&pa)),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let again = alice.m.pending_commits().unwrap();
    assert_eq!(again, std::slice::from_ref(&pa));
    assert_eq!(
        alice.push(&w, &again[0]).await,
        (StatusCode::CONFLICT, true)
    );
    bob.read(&w).await;
    alice.say(&w, &chat.id, "noch synchron?").await;
    assert!(
        matches!(&bob.read(&w).await[..], [Incoming::Message(m)] if m.text == "noch synchron?")
    );

    // The server only ever saw ciphertext.
    let db = w.server.db_dump();
    for needle in [
        "Hallo über den Server",
        "angekommen",
        "noch synchron?",
        "Alice",
        "Bob",
    ] {
        assert!(
            !db.windows(needle.len()).any(|x| x == needle.as_bytes()),
            "{needle} on the server"
        );
    }
}

#[tokio::test]
async fn s12_the_last_resort_package_is_handed_out_but_kept() {
    let w = World::new();
    let mut bob = ChatClient::register(&w, "local-bob").await;
    let alice = ChatClient::register(&w, "local-alice").await;
    bob.upload(&w, 1).await;
    let first = alice.claim(&w, bob.c.user()).await;
    let second = alice.claim(&w, bob.c.user()).await;
    let third = alice.claim(&w, bob.c.user()).await;
    assert_eq!((first.len(), second.len(), third.len()), (1, 1, 1));
    assert_ne!(first[0].bytes, second[0].bytes, "one-time package first");
    assert_eq!(
        second[0].bytes, third[0].bytes,
        "then the last-resort one, again and again"
    );
    let (_, count) = w
        .call("GET", "/v1/keypackages", Some(&bob.c.token), None)
        .await;
    assert_eq!(count["count"], 0, "time to upload more");
}

#[test]
fn s13_a_database_from_m3_gets_the_new_column() {
    let dir = TempDir::new();
    let path = dir.0.join("old.db");
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE keypackages (id INTEGER PRIMARY KEY, user TEXT NOT NULL,
                 device TEXT NOT NULL, data BLOB NOT NULL);
             INSERT INTO keypackages (user, device, data) VALUES ('local-bob', 'pc', x'01');",
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    assert!(
        db.tables()
            .unwrap()
            .contains(&"keypackages.last_resort".to_string())
    );
    assert_eq!(
        db.keypackage_count("local-bob", "pc").unwrap(),
        1,
        "old packages stay usable"
    );
    assert_eq!(
        db.claim_keypackage("local-bob", "pc").unwrap(),
        Some(vec![1])
    );
    // Opening again does not try to add it twice.
    drop(db);
    Db::open(&path).unwrap();
}
