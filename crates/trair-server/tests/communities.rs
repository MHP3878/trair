// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! M8, step 3a: communities on the real server (in memory), with real
//! accounts, device keys and signatures. Every right is tried from the
//! side that must not have it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use trair_core::proofs::{OpenMessage, sign_login, sign_open_message};
use trair_core::teamkey::TeamKey;
use trair_core::{Account, DeviceIdentity, SignedDeviceList};
use trair_server::{Db, IDLE_LIMIT, INVITE_PREFIX, Server, perm, router};

const START: u64 = 1_800_000_000;

struct World {
    server: Arc<Server>,
    clock: Arc<AtomicU64>,
    team: TeamKey,
}

impl World {
    fn new() -> World {
        let clock = Arc::new(AtomicU64::new(START));
        let c = clock.clone();
        let team = TeamKey::generate().unwrap();
        let server = Server::with_team_keys(
            Db::in_memory().unwrap(),
            Arc::new(move || c.load(Ordering::SeqCst)),
            vec![team.public_key_hex()],
        );
        World {
            server,
            clock,
            team,
        }
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

    async fn new(w: &World, user: &str) -> Person {
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

    /// One op; the status and the answer.
    async fn op(&self, w: &World, body: Value) -> (StatusCode, Value) {
        w.call("POST", "/v1/community", Some(&self.token), Some(body))
            .await
    }

    /// One op that must work.
    async fn ok(&self, w: &World, body: Value) -> Value {
        let (status, answer) = self.op(w, body.clone()).await;
        assert_eq!(status, StatusCode::OK, "{body} → {answer}");
        answer
    }

    /// One op that the server must refuse with `code`.
    async fn refused(&self, w: &World, body: Value, code: &str) {
        let (status, answer) = self.op(w, body.clone()).await;
        assert!(!status.is_success(), "{} could do {body}", self.user());
        assert_eq!(answer["error"], code, "{body} → {status} {answer}");
    }

    fn signed(&self, community: &str, channel: &str, text: &str, sent: u64) -> Value {
        let m = OpenMessage {
            community,
            channel,
            user: self.user(),
            device: &self.device.id,
            sent,
            text,
        };
        let sig = sign_open_message(&self.device, &m).unwrap();
        json!({"op": "post", "community": community, "channel": channel, "text": text,
            "sent": sent, "sig": hex::encode(sig)})
    }

    async fn post(
        &self,
        w: &World,
        community: &str,
        channel: &str,
        text: &str,
    ) -> (StatusCode, Value) {
        self.op(w, self.signed(community, channel, text, w.now()))
            .await
    }

    /// Log in again (counts as being active).
    async fn relogin(&mut self, w: &World) {
        let user = self.user().to_string();
        let (_, c) = w
            .call(
                "POST",
                "/v1/login/challenge",
                None,
                Some(json!({"user": user, "device": self.device.id})),
            )
            .await;
        let challenge = c["challenge"].as_str().unwrap().to_string();
        let sig = sign_login(&self.device, &user, &hex::decode(&challenge).unwrap()).unwrap();
        let (status, l) = w
            .call(
                "POST",
                "/v1/login",
                None,
                Some(json!({"user": user, "device": self.device.id, "challenge": challenge, "signature": hex::encode(sig)})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{l}");
        self.token = l["token"].as_str().unwrap().to_string();
    }

    async fn sync(&self, w: &World, since: i64) -> Value {
        let (status, body) = w
            .call(
                "GET",
                &format!("/v1/communities/sync?since={since}"),
                Some(&self.token),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

/// Olga's community with Ada and Max joined by invitation.
async fn community(w: &World, owner: &Person, members: &[&Person]) -> String {
    let made = owner
        .ok(w, json!({"op": "create", "name": "Abi 2026"}))
        .await;
    let id = made["community"].as_str().unwrap().to_string();
    let inv = owner
        .ok(
            w,
            json!({"op": "create_invite", "community": id, "minutes": 60, "uses": null}),
        )
        .await;
    let code = inv["code"].as_str().unwrap();
    assert!(code.starts_with(INVITE_PREFIX));
    for m in members {
        let j = m.ok(w, json!({"op": "join", "code": code})).await;
        assert_eq!(j["community"], id.as_str());
    }
    id
}

/// Is this text anywhere in the server's database?
fn stored(w: &World, text: &str) -> bool {
    let db = w.server.db_dump();
    db.windows(text.len()).any(|x| x == text.as_bytes())
}

async fn info(w: &World, p: &Person, id: &str) -> Value {
    p.ok(w, json!({"op": "info", "community": id})).await
}

fn messages_of(v: &Value) -> Vec<String> {
    v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn k01_a_community_with_an_invitation_and_signed_messages() {
    let w = World::new();
    let olga = Person::new(&w, "olga").await;
    let ada = Person::new(&w, "ada").await;
    let id = community(&w, &olga, &[&ada]).await;
    let i = info(&w, &ada, &id).await;
    assert_eq!(i["community"]["name"], "Abi 2026");
    assert_eq!(i["community"]["owner"], "olga");
    let roles: Vec<&str> = i["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["Admin", "Moderator", "@alle"]);
    let chans: Vec<(&str, bool)> = i["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap(), c["can_write"].as_bool().unwrap()))
        .collect();
    assert_eq!(chans, [("regeln", false), ("allgemein", true)]);
    assert_eq!(i["me"]["perms"], 0);
    assert_eq!(info(&w, &olga, &id).await["me"]["perms"], perm::ALL);

    let (status, _) = ada.post(&w, &id, "allgemein", "Hallo zusammen").await;
    assert_eq!(status, StatusCode::OK);
    // The rules channel is for those who manage channels.
    ada.refused(
        &w,
        ada.signed(&id, "regeln", "meins", w.now()),
        "not_allowed",
    )
    .await;
    olga.ok(&w, olga.signed(&id, "regeln", "Seid nett.", w.now()))
        .await;
    let list = olga
        .ok(
            &w,
            json!({"op": "messages", "community": id, "channel": "allgemein"}),
        )
        .await;
    assert_eq!(messages_of(&list), ["Hallo zusammen"]);
    assert_eq!(list["messages"][0]["user"], "ada");
    assert_eq!(list["messages"][0]["device"], ada.device.id.as_str());

    // A signature over other words, from another device, or far from now.
    let mut forged = ada.signed(&id, "allgemein", "harmlos", w.now());
    forged["text"] = json!("ganz anders");
    ada.refused(&w, forged, "bad_signature").await;
    let mut borrowed = olga.signed(&id, "allgemein", "von Olga", w.now());
    borrowed["op"] = json!("post");
    ada.refused(&w, borrowed, "bad_signature").await;
    ada.refused(
        &w,
        ada.signed(&id, "allgemein", "alt", w.now() - 3600),
        "clock",
    )
    .await;
    ada.refused(&w, ada.signed(&id, "allgemein", "   ", w.now()), "bad_text")
        .await;
    ada.refused(
        &w,
        ada.signed(&id, "allgemein", &"x".repeat(4001), w.now()),
        "bad_text",
    )
    .await;
    // Strangers see nothing.
    let eve = Person::new(&w, "eve").await;
    eve.refused(&w, json!({"op": "info", "community": id}), "not_allowed")
        .await;
    eve.refused(
        &w,
        json!({"op": "messages", "community": id, "channel": "allgemein"}),
        "not_allowed",
    )
    .await;
    eve.refused(
        &w,
        eve.signed(&id, "allgemein", "spam", w.now()),
        "not_allowed",
    )
    .await;
    eve.refused(
        &w,
        json!({"op": "join", "code": "trairc:0000"}),
        "bad_invite",
    )
    .await;
}

#[tokio::test]
async fn k02_members_write_only_after_accepting_the_rules() {
    let w = World::new();
    let olga = Person::new(&w, "olga").await;
    let ada = Person::new(&w, "ada").await;
    let id = community(&w, &olga, &[&ada]).await;
    let edit = |rules: &str| json!({"op": "edit", "community": id, "name": "Abi 2026", "about": "Unser Jahrgang", "rules": rules});
    ada.refused(&w, edit("meine Regeln"), "not_allowed").await;
    let rev = olga.ok(&w, edit("1. Freundlich bleiben")).await["rules_rev"]
        .as_i64()
        .unwrap();
    assert_eq!(rev, 1);
    let i = info(&w, &ada, &id).await;
    assert_eq!(i["me"]["rules_ok"], false);
    assert_eq!(i["channels"][1]["can_write"], false);
    let (status, body) = ada.post(&w, &id, "allgemein", "darf ich?").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("accept_rules"))
    );
    ada.refused(
        &w,
        json!({"op": "accept_rules", "community": id, "rev": 0}),
        "rules_changed",
    )
    .await;
    ada.ok(&w, json!({"op": "accept_rules", "community": id, "rev": 1}))
        .await;
    assert_eq!(
        ada.post(&w, &id, "allgemein", "jetzt ja").await.0,
        StatusCode::OK
    );
    // Same rules, other description: nothing to accept again.
    olga.ok(&w, json!({"op": "edit", "community": id, "name": "Abi 2026", "about": "neu", "rules": "1. Freundlich bleiben"}))
        .await;
    assert_eq!(
        ada.post(&w, &id, "allgemein", "noch da").await.0,
        StatusCode::OK
    );
    // New rules: accept again.
    olga.ok(&w, edit("1. Freundlich bleiben\n2. Kein Spam"))
        .await;
    assert_eq!(
        ada.post(&w, &id, "allgemein", "und jetzt?").await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn k03_roles_rights_and_who_stands_above_whom() {
    let w = World::new();
    let olga = Person::new(&w, "olga").await;
    let bea = Person::new(&w, "bea").await;
    let ada = Person::new(&w, "ada").await;
    let max = Person::new(&w, "max").await;
    let id = community(&w, &olga, &[&bea, &ada, &max]).await;
    let set = |user: &str, roles: Vec<i64>| json!({"op": "set_roles", "community": id, "user": user, "roles": roles});
    // Members cannot hand out roles; the owner can.
    ada.refused(&w, set("ada", vec![1]), "not_allowed").await;
    olga.ok(&w, set("bea", vec![1])).await;
    olga.ok(&w, set("ada", vec![2])).await;
    let i = info(&w, &ada, &id).await;
    assert_eq!(
        i["me"]["perms"],
        perm::KICK | perm::DELETE_MESSAGES | perm::REPORTS | perm::LOG
    );

    // The moderator removes a member, not the admin, not the owner, and
    // hands out no roles.
    let kick = |user: &str| json!({"op": "kick", "community": id, "user": user, "reason": "Spam"});
    ada.refused(&w, kick("bea"), "not_allowed").await;
    ada.refused(&w, kick("olga"), "not_allowed").await;
    ada.refused(&w, set("max", vec![2]), "not_allowed").await;
    ada.ok(&w, kick("max")).await;
    assert!(
        !info(&w, &olga, &id).await["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["user"] == "max")
    );
    max.refused(&w, json!({"op": "info", "community": id}), "not_allowed")
        .await;

    // The admin shapes roles below itself only, with rights it has.
    let r = bea
        .ok(&w, json!({"op": "create_role", "community": id, "name": "Helfer", "color": 0xFF00FF00_u32, "perms": perm::DELETE_MESSAGES}))
        .await["role"]
        .as_i64()
        .unwrap();
    let edit = |role: i64, position: i64, perms: u32| json!({"op": "edit_role", "community": id, "role": role, "name": "x", "color": 0, "position": position, "perms": perms});
    bea.refused(&w, edit(r, 20, perm::KICK), "not_allowed")
        .await;
    bea.refused(&w, edit(1, 15, perm::KICK), "not_allowed")
        .await;
    bea.ok(&w, edit(r, 12, perm::KICK)).await;
    bea.refused(&w, set("ada", vec![1]), "not_allowed").await;
    bea.ok(&w, set("ada", vec![2, r])).await;
    bea.refused(
        &w,
        json!({"op": "delete_role", "community": id, "role": 1}),
        "not_allowed",
    )
    .await;
    bea.refused(
        &w,
        json!({"op": "delete_role", "community": id, "role": 0}),
        "not_allowed",
    )
    .await;
    // A moderator who may manage roles still cannot give rights it lacks.
    olga.ok(&w, edit(2, 10, perm::KICK | perm::MANAGE_ROLES))
        .await;
    ada.refused(
        &w,
        json!({"op": "create_role", "community": id, "name": "Bannhammer", "color": 0, "perms": perm::BAN}),
        "not_allowed",
    )
    .await;
    // Nobody takes away rights they could not give.
    let banners = olga
        .ok(&w, json!({"op": "create_role", "community": id, "name": "Bann", "color": 0, "perms": perm::BAN}))
        .await["role"]
        .as_i64()
        .unwrap();
    olga.ok(&w, edit(banners, 5, perm::BAN)).await;
    ada.refused(&w, edit(banners, 5, 0), "not_allowed").await;
    // Everyone gets the rights of "@alle".
    let invite = json!({"op": "create_invite", "community": id, "minutes": 10, "uses": 1});
    ada.refused(&w, invite.clone(), "not_allowed").await;
    olga.ok(&w, edit(0, 0, perm::INVITE)).await;
    ada.ok(&w, invite).await;
    assert_eq!(info(&w, &bea, &id).await["me"]["perms"], perm::ALL);
    let log = olga.ok(&w, json!({"op": "log", "community": id})).await;
    let actions: Vec<&str> = log["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(
        actions.contains(&"kick") && actions.contains(&"role_create") && actions.contains(&"roles"),
        "{actions:?}"
    );
    max.refused(&w, json!({"op": "log", "community": id}), "not_allowed")
        .await;
}

#[tokio::test]
async fn k04_channels_limit_reading_and_writing() {
    let w = World::new();
    let olga = Person::new(&w, "olga").await;
    let ada = Person::new(&w, "ada").await;
    let max = Person::new(&w, "max").await;
    let id = community(&w, &olga, &[&ada, &max]).await;
    olga.ok(
        &w,
        json!({"op": "set_roles", "community": id, "user": "ada", "roles": [2]}),
    )
    .await;
    let staff = olga
        .ok(&w, json!({"op": "create_channel", "community": id, "name": "team", "category": "Intern", "read_roles": [2], "write_roles": null}))
        .await["channel"]
        .as_str()
        .unwrap()
        .to_string();
    max.refused(
        &w,
        json!({"op": "create_channel", "community": id, "name": "meins", "category": "x", "read_roles": null, "write_roles": null}),
        "not_allowed",
    )
    .await;
    olga.refused(
        &w,
        json!({"op": "create_channel", "community": id, "name": "x", "category": "x", "read_roles": [99], "write_roles": null}),
        "unknown_role",
    )
    .await;
    let since = max.sync(&w, -1).await["seq"].as_i64().unwrap();
    assert_eq!(
        ada.post(&w, &id, &staff, "nur fürs Team").await.0,
        StatusCode::OK
    );
    assert_eq!(
        max.post(&w, &id, &staff, "ich auch").await.0,
        StatusCode::FORBIDDEN
    );
    max.refused(
        &w,
        json!({"op": "messages", "community": id, "channel": staff}),
        "not_allowed",
    )
    .await;
    let seen: Vec<String> = info(&w, &max, &id).await["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert!(!seen.contains(&staff));
    // The long poll does not hand it over either.
    let s = max.sync(&w, since).await;
    assert!(
        s["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["kind"] != "message"),
        "{s}"
    );
    let s = ada.sync(&w, since).await;
    assert!(
        s["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["message"]["text"] == "nur fürs Team")
    );
    // Read-only: everyone reads, only managers write.
    olga.ok(&w, json!({"op": "edit_channel", "community": id, "channel": staff, "name": "news", "category": "Info", "topic": "", "position": 5, "read_roles": null, "write_roles": []}))
        .await;
    assert_eq!(
        ada.post(&w, &id, &staff, "geht nicht mehr").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        messages_of(
            &max.ok(
                &w,
                json!({"op": "messages", "community": id, "channel": staff})
            )
            .await
        ),
        ["nur fürs Team"]
    );
    // Deleting a channel deletes its messages.
    assert!(stored(&w, "nur fürs Team"));
    olga.ok(
        &w,
        json!({"op": "delete_channel", "community": id, "channel": staff}),
    )
    .await;
    assert!(!stored(&w, "nur fürs Team"));
}

#[tokio::test]
async fn k05_kicked_can_come_back_banned_cannot_invites_run_out() {
    let w = World::new();
    let olga = Person::new(&w, "olga").await;
    let max = Person::new(&w, "max").await;
    let id = community(&w, &olga, &[&max]).await;
    let once = olga
        .ok(
            &w,
            json!({"op": "create_invite", "community": id, "minutes": 60, "uses": 2}),
        )
        .await;
    let code = once["code"].as_str().unwrap().to_string();
    olga.ok(&w, json!({"op": "kick", "community": id, "user": "max"}))
        .await;
    max.ok(&w, json!({"op": "join", "code": code})).await;
    olga.ok(
        &w,
        json!({"op": "ban", "community": id, "user": "max", "reason": "Beleidigungen"}),
    )
    .await;
    max.refused(&w, json!({"op": "join", "code": code}), "banned")
        .await;
    let bans = olga.ok(&w, json!({"op": "bans", "community": id})).await;
    assert_eq!(bans["bans"][0]["reason"], "Beleidigungen");
    olga.ok(&w, json!({"op": "unban", "community": id, "user": "max"}))
        .await;
    // Two uses: Max used one; the next one takes the last.
    let ida = Person::new(&w, "ida").await;
    ida.ok(&w, json!({"op": "join", "code": code})).await;
    max.refused(&w, json!({"op": "join", "code": code}), "invite_expired")
        .await;
    // Expired and taken back.
    let short = olga
        .ok(
            &w,
            json!({"op": "create_invite", "community": id, "minutes": 1, "uses": null}),
        )
        .await;
    w.later(61);
    max.refused(
        &w,
        json!({"op": "join", "code": short["code"]}),
        "invite_expired",
    )
    .await;
    let other = olga
        .ok(
            &w,
            json!({"op": "create_invite", "community": id, "minutes": 60, "uses": null}),
        )
        .await;
    olga.ok(
        &w,
        json!({"op": "revoke_invite", "community": id, "id": other["id"]}),
    )
    .await;
    max.refused(
        &w,
        json!({"op": "join", "code": other["code"]}),
        "bad_invite",
    )
    .await;
    olga.refused(
        &w,
        json!({"op": "create_invite", "community": id, "minutes": 31 * 24 * 60, "uses": null}),
        "bad_invite",
    )
    .await;
    // Banning someone who is not in it keeps them out too; nobody bans
    // the owner or themselves.
    let eve = Person::new(&w, "eve").await;
    olga.ok(&w, json!({"op": "ban", "community": id, "user": "eve"}))
        .await;
    let fresh = olga
        .ok(
            &w,
            json!({"op": "create_invite", "community": id, "minutes": 60, "uses": null}),
        )
        .await;
    eve.refused(&w, json!({"op": "join", "code": fresh["code"]}), "banned")
        .await;
    olga.refused(
        &w,
        json!({"op": "ban", "community": id, "user": "olga"}),
        "not_allowed",
    )
    .await;
    olga.refused(
        &w,
        json!({"op": "ban", "community": id, "user": "niemand"}),
        "not_found",
    )
    .await;
    // Only the hash of a code is stored.
    assert!(!stored(&w, fresh["code"].as_str().unwrap()));
}

#[tokio::test]
async fn k06_reports_go_to_moderators_or_to_the_team() {
    let w = World::new();
    let olga = Person::new(&w, "olga").await;
    let ada = Person::new(&w, "ada").await;
    let max = Person::new(&w, "max").await;
    let id = community(&w, &olga, &[&ada, &max]).await;
    olga.ok(
        &w,
        json!({"op": "set_roles", "community": id, "user": "ada", "roles": [2]}),
    )
    .await;
    let (_, m) = max.post(&w, &id, "allgemein", "Beleidigung 1234").await;
    let mid = m["id"].as_i64().unwrap();
    // Olga reports it to the moderators.
    let rid = olga
        .ok(
            &w,
            json!({"op": "report", "community": id, "message": mid, "reason": "beleidigend"}),
        )
        .await["id"]
        .as_i64()
        .unwrap();
    max.refused(&w, json!({"op": "reports", "community": id}), "not_allowed")
        .await;
    let r = ada.ok(&w, json!({"op": "reports", "community": id})).await;
    assert_eq!(r["reports"][0]["target"], "max");
    assert_eq!(r["reports"][0]["snapshot"]["text"], "Beleidigung 1234");
    // The moderator deletes the message: gone from the server, the report
    // keeps what was reported.
    max.refused(
        &w,
        json!({"op": "delete_message", "community": id, "id": mid + 99}),
        "not_found",
    )
    .await;
    ada.ok(
        &w,
        json!({"op": "delete_message", "community": id, "id": mid}),
    )
    .await;
    assert!(
        messages_of(
            &olga
                .ok(
                    &w,
                    json!({"op": "messages", "community": id, "channel": "allgemein"})
                )
                .await
        )
        .is_empty()
    );
    ada.ok(&w, json!({"op": "handle_report", "community": id, "id": rid, "status": "done", "note": "gelöscht"}))
        .await;
    assert!(
        ada.ok(&w, json!({"op": "reports", "community": id})).await["reports"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let all = ada
        .ok(&w, json!({"op": "reports", "community": id, "all": true}))
        .await;
    assert_eq!(all["reports"][0]["status"], "done");
    assert_eq!(all["reports"][0]["handled_by"], "ada");
    // A member deletes its own message, not others'.
    let (_, own) = max.post(&w, &id, "allgemein", "Tippfehler").await;
    let (_, olgas) = olga.post(&w, &id, "allgemein", "Hallo").await;
    max.refused(
        &w,
        json!({"op": "delete_message", "community": id, "id": olgas["id"]}),
        "not_allowed",
    )
    .await;
    max.ok(
        &w,
        json!({"op": "delete_message", "community": id, "id": own["id"]}),
    )
    .await;
    // The moderator cannot delete the owner's message.
    ada.refused(
        &w,
        json!({"op": "delete_message", "community": id, "id": olgas["id"]}),
        "not_allowed",
    )
    .await;

    // Max reports Ada to the Trair team, with messages from an encrypted
    // chat he chose to attach.
    let team = json!({"op": "report", "community": null, "user": "ada", "team": true,
        "reason": "bedroht mich", "evidence": [{"from": "ada", "text": "Drohung 5678", "at": START}]});
    max.ok(&w, team).await;
    let r = ada
        .ok(&w, json!({"op": "reports", "community": id, "all": true}))
        .await;
    assert_eq!(
        r["reports"].as_array().unwrap().len(),
        1,
        "moderators do not see team reports"
    );
    // Only a valid team badge opens them.
    let badge = |p: &Person| {
        serde_json::to_value(
            w.team
                .sign_badge(p.account.public_key(), w.now(), 30)
                .unwrap(),
        )
        .unwrap()
    };
    let foreign = TeamKey::generate().unwrap();
    let fake = serde_json::to_value(
        foreign
            .sign_badge(olga.account.public_key(), w.now(), 30)
            .unwrap(),
    )
    .unwrap();
    olga.refused(
        &w,
        json!({"op": "team_reports", "badge": fake}),
        "not_allowed",
    )
    .await;
    // Someone else's badge does not work either.
    olga.refused(
        &w,
        json!({"op": "team_reports", "badge": badge(&max)}),
        "not_allowed",
    )
    .await;
    let tr = olga
        .ok(&w, json!({"op": "team_reports", "badge": badge(&olga)}))
        .await;
    assert_eq!(
        tr["reports"][0]["snapshot"]["forwarded"][0]["text"],
        "Drohung 5678"
    );
    let tid = tr["reports"][0]["id"].clone();
    olga.ok(
        &w,
        json!({"op": "team_handle", "badge": badge(&olga), "id": tid, "status": "dismissed"}),
    )
    .await;
    // Evidence only goes to the team; a report needs a reason.
    max.refused(
        &w,
        json!({"op": "report", "community": id, "user": "ada", "reason": "x", "evidence": []}),
        "bad_evidence",
    )
    .await;
    max.refused(
        &w,
        json!({"op": "report", "community": id, "user": "ada", "reason": "  "}),
        "reason_needed",
    )
    .await;
}

#[tokio::test]
async fn k07_the_long_poll_brings_messages_deletions_and_removal() {
    let w = World::new();
    let olga = Person::new(&w, "olga").await;
    let max = Person::new(&w, "max").await;
    let id = community(&w, &olga, &[&max]).await;
    let since = max.sync(&w, -1).await["seq"].as_i64().unwrap();
    let (_, m) = olga.post(&w, &id, "allgemein", "Willkommen").await;
    olga.ok(
        &w,
        json!({"op": "delete_message", "community": id, "id": m["id"]}),
    )
    .await;
    let s = max.sync(&w, since).await;
    let kinds: Vec<&str> = s["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    // The message is gone already: only its deletion is left to see.
    assert_eq!(kinds, ["deleted"], "{s}");
    let since = s["seq"].as_i64().unwrap();
    olga.post(&w, &id, "allgemein", "Zweite").await;
    let s = max.sync(&w, since).await;
    assert_eq!(s["events"][0]["message"]["text"], "Zweite", "{s}");
    assert_eq!(s["events"][0]["message"]["user"], "olga");
    let since = s["seq"].as_i64().unwrap();
    olga.ok(&w, json!({"op": "kick", "community": id, "user": "max"}))
        .await;
    let s = max.sync(&w, since).await;
    let ev = s["events"].as_array().unwrap();
    assert!(
        ev.iter()
            .any(|e| e["kind"] == "removed" && e["target"] == "max"),
        "{s}"
    );
    // After that, nothing more of this community reaches Max.
    let since = s["seq"].as_i64().unwrap();
    olga.post(&w, &id, "allgemein", "ohne Max").await;
    assert!(
        max.sync(&w, since).await["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // A long poll with nothing new waits and comes back empty.
    let (status, body) = w
        .call(
            "GET",
            &format!("/v1/communities/sync?since={since}&wait=1"),
            Some(&max.token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["events"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn k08_idle_owner_hands_on_and_the_last_one_out_deletes_it() {
    let w = World::new();
    let olga = Person::new(&w, "olga").await;
    let ada = Person::new(&w, "ada").await;
    let mut max = Person::new(&w, "max").await;
    let id = community(&w, &olga, &[&ada, &max]).await;
    olga.ok(
        &w,
        json!({"op": "set_roles", "community": id, "user": "max", "roles": [2]}),
    )
    .await;
    olga.refused(
        &w,
        json!({"op": "leave", "community": id}),
        "hand_over_first",
    )
    .await;
    ada.refused(
        &w,
        json!({"op": "transfer", "community": id, "user": "ada"}),
        "not_allowed",
    )
    .await;
    ada.ok(&w, json!({"op": "leave", "community": id})).await;
    ada.refused(&w, json!({"op": "info", "community": id}), "not_allowed")
        .await;
    olga.post(&w, &id, "allgemein", "von Olga 9999").await;
    // Half a year: only Max logs in again. Olga's and Ada's accounts go;
    // the community goes to Max, Olga's messages with her.
    w.later(IDLE_LIMIT + 1);
    max.relogin(&w).await;
    assert_eq!(w.server.cleanup().unwrap().accounts, 2);
    assert!(!stored(&w, "von Olga 9999"));
    let i = info(&w, &max, &id).await;
    assert_eq!(i["community"]["owner"], "max");
    assert_eq!(i["members"].as_array().unwrap().len(), 1);
    // The last one leaving deletes it.
    assert_eq!(
        max.ok(&w, json!({"op": "leave", "community": id})).await["deleted"],
        true
    );
    assert!(!stored(&w, "Abi 2026"));
}

#[tokio::test]
async fn k09_an_owner_who_deletes_the_account_leaves_a_successor() {
    let w = World::new();
    let olga = Person::new(&w, "olga").await;
    let ada = Person::new(&w, "ada").await;
    let max = Person::new(&w, "max").await;
    let id = community(&w, &olga, &[&ada, &max]).await;
    olga.ok(
        &w,
        json!({"op": "set_roles", "community": id, "user": "max", "roles": [2]}),
    )
    .await;
    olga.post(&w, &id, "allgemein", "Olgas Nachricht").await;
    let now = w.now();
    let sig = trair_core::proofs::sign_account_deletion(&olga.account, now).unwrap();
    let (status, _) = w
        .call(
            "POST",
            "/v1/accounts/delete",
            Some(&olga.token),
            Some(json!({"now": now, "signature": hex::encode(sig)})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let i = info(&w, &max, &id).await;
    assert_eq!(
        i["community"]["owner"], "max",
        "the moderator stands above Ada"
    );
    assert_eq!(i["me"]["perms"], perm::ALL);
    assert!(
        messages_of(
            &max.ok(
                &w,
                json!({"op": "messages", "community": id, "channel": "allgemein"})
            )
            .await
        )
        .is_empty()
    );
    // Max hands it to Ada and she deletes it: everyone is told.
    let since = max.sync(&w, -1).await["seq"].as_i64().unwrap();
    max.ok(
        &w,
        json!({"op": "transfer", "community": id, "user": "ada"}),
    )
    .await;
    max.refused(&w, json!({"op": "delete", "community": id}), "not_allowed")
        .await;
    ada.ok(&w, json!({"op": "delete", "community": id})).await;
    let s = max.sync(&w, since).await;
    assert!(
        s["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "removed" && e["target"] == "max")
    );
    max.refused(&w, json!({"op": "info", "community": id}), "not_found")
        .await;
    assert!(
        max.ok(&w, json!({"op": "list"})).await["communities"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
