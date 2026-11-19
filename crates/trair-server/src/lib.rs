// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Trair server (milestone M3, step 2): accounts, login by device key,
//! MLS key packages, delivery of encrypted messages and the relay for
//! pairing a second device.
//!
//! What the server never gets: passwords, private keys, readable
//! messages, profile contents. What it does see: which account sends to
//! which account and when (metadata; "sealed sender" can reduce this
//! later). Messages are stored without sender and deleted when fetched,
//! at the latest after [`MESSAGE_LIFETIME`]. Accounts nobody used for
//! [`IDLE_LIMIT`] are deleted with all their data.
//!
//! All requests and answers are JSON. Binary data (messages, key
//! packages, pairing messages) is base64. Errors are
//! `{"ok": false, "error": "<code>"}` with a fitting HTTP status.

mod db;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, Query, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use trair_core::SignedDeviceList;
use trair_core::account::validate_name;
use trair_core::proofs::{CHALLENGE_LEN, verify_account_deletion, verify_login};

pub use db::{Cleaned, Db};

/// How long a session token works (12 hours).
pub const SESSION_LIFETIME: u64 = 12 * 3600;
/// How long a login challenge works (60 seconds).
pub const CHALLENGE_LIFETIME: u64 = 60;
/// Messages not fetched within 30 days are deleted.
pub const MESSAGE_LIFETIME: u64 = 30 * 24 * 3600;
/// Accounts unused for 6 months are deleted (decision of 17.11.2026).
pub const IDLE_LIMIT: u64 = 183 * 24 * 3600;
/// A pairing channel lives 10 minutes, like the code.
pub const PAIRING_LIFETIME: u64 = 10 * 60;
/// Largest message, in bytes.
pub const MAX_MESSAGE: usize = 256 * 1024;
/// Largest request, in bytes (base64 and JSON included).
pub const MAX_REQUEST: usize = 1024 * 1024;
/// Most waiting messages per device.
pub const MAX_QUEUE: usize = 10_000;
/// Most recipients of one message.
pub const MAX_RECIPIENTS: usize = 1000;
/// Most stored key packages per device, and largest key package.
pub const MAX_KEYPACKAGES: usize = 100;
/// Largest key package, in bytes.
pub const MAX_KEYPACKAGE: usize = 16 * 1024;
/// Largest pairing message, in bytes.
pub const MAX_PAIRING_MESSAGE: usize = 160 * 1024;
/// Longest long poll, in seconds.
pub const MAX_WAIT: u64 = 30;
/// New accounts per IP address and hour.
pub const REGISTRATIONS_PER_HOUR: usize = 10;
/// Pairing attempts by new devices per IP address and hour.
pub const GUEST_ATTEMPTS_PER_HOUR: usize = 20;
/// Open pairing channels per account.
pub const MAX_OPEN_PAIRINGS: usize = 3;
/// Login challenges per IP address and hour.
pub const CHALLENGES_PER_HOUR: usize = 600;
/// Most login challenges kept at once (they live 60 seconds).
pub const MAX_CHALLENGES: usize = 100_000;

/// The clock: unix time in seconds. Tests use their own.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The real clock.
pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    })
}

/// One pairing channel. Each part is written once.
struct Slot {
    created: u64,
    owner: (String, String),
    host_hello: Option<Vec<u8>>,
    guest: Option<(Vec<u8>, Vec<u8>)>,
    bundle: Option<Vec<u8>>,
    notify: Arc<Notify>,
}

#[derive(Default)]
struct Memory {
    challenges: HashMap<String, (String, String, u64)>,
    waiters: HashMap<(String, String), Arc<Notify>>,
    slots: HashMap<u16, Slot>,
    limits: HashMap<(&'static str, IpAddr), Vec<u64>>,
}

/// The running server.
pub struct Server {
    db: Mutex<Db>,
    memory: Mutex<Memory>,
    clock: Clock,
}

impl Server {
    /// A server with the given database and clock.
    pub fn new(db: Db, clock: Clock) -> Arc<Server> {
        Arc::new(Server {
            db: Mutex::new(db),
            memory: Mutex::new(Memory::default()),
            clock,
        })
    }

    fn now(&self) -> u64 {
        (self.clock)()
    }

    fn db(&self) -> MutexGuard<'_, Db> {
        self.db.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn memory(&self) -> MutexGuard<'_, Memory> {
        self.memory.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Delete old messages, expired sessions, idle accounts, stale
    /// challenges and pairing channels. Runs every hour.
    pub fn cleanup(&self) -> Result<Cleaned, String> {
        let now = self.now();
        {
            let mut m = self.memory();
            m.challenges
                .retain(|_, (_, _, at)| now < *at + CHALLENGE_LIFETIME);
            m.slots.retain(|_, s| now < s.created + PAIRING_LIFETIME);
            m.limits.retain(|_, times| {
                times.retain(|t| now < t + 3600);
                !times.is_empty()
            });
        }
        self.db()
            .cleanup(now, MESSAGE_LIFETIME, IDLE_LIMIT)
            .map_err(|e| e.to_string())
    }

    /// Column names of the stored messages (to prove there is no sender).
    pub fn message_columns(&self) -> Vec<String> {
        self.db().message_columns().unwrap_or_default()
    }

    /// Count an action of an IP address; false if over the hourly limit.
    fn allow(&self, what: &'static str, ip: Option<IpAddr>, per_hour: usize) -> bool {
        let Some(ip) = ip else { return true };
        let now = self.now();
        let mut m = self.memory();
        let times = m.limits.entry((what, ip)).or_default();
        times.retain(|t| now < t + 3600);
        if times.len() >= per_hour {
            return false;
        }
        times.push(now);
        true
    }

    fn waiter(&self, user: &str, device: &str) -> Arc<Notify> {
        self.memory()
            .waiters
            .entry((user.to_string(), device.to_string()))
            .or_default()
            .clone()
    }
}

/// All routes, ready to serve.
pub fn router(server: Arc<Server>) -> Router {
    Router::new()
        .route("/v1/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/v1/accounts", post(register))
        .route("/v1/accounts/{user}", get(account))
        .route("/v1/accounts/delete", post(delete_account))
        .route("/v1/login/challenge", post(challenge))
        .route("/v1/login", post(login))
        .route("/v1/devices", put(update_devices))
        .route("/v1/keypackages", post(upload_keypackages))
        .route("/v1/keypackages/claim", post(claim_keypackages))
        .route("/v1/messages", post(send).get(fetch))
        .route("/v1/messages/ack", post(ack))
        .route("/v1/pairing", post(open_pairing))
        .route(
            "/v1/pairing/{channel}/host",
            put(put_host_hello).get(get_host_hello),
        )
        .route("/v1/pairing/{channel}/guest", put(put_guest).get(get_guest))
        .route(
            "/v1/pairing/{channel}/bundle",
            put(put_bundle).get(get_bundle),
        )
        .layer(DefaultBodyLimit::max(MAX_REQUEST))
        .with_state(server)
}

// ---------------------------------------------------------------- errors

/// An error answer: HTTP status and a short code.
#[derive(Debug)]
pub struct ApiError(StatusCode, &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"ok": false, "error": self.1}))).into_response()
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

fn bad(code: &'static str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, code)
}

fn internal<E: std::fmt::Display>(_: E) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal")
}

fn decode(text: &str, max: usize) -> Result<Vec<u8>, ApiError> {
    let bytes = B64.decode(text).map_err(|_| bad("bad_base64"))?;
    if bytes.is_empty() || bytes.len() > max {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "too_large"));
    }
    Ok(bytes)
}

fn random_hex<const N: usize>() -> Result<String, ApiError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal"))?;
    Ok(hex::encode(b))
}

fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

// ---------------------------------------------------------------- extractors

/// A logged-in device, from `Authorization: Bearer <token>`.
pub struct Session {
    user: String,
    device: String,
}

impl FromRequestParts<Arc<Server>> for Session {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        server: &Arc<Server>,
    ) -> Result<Self, Self::Rejection> {
        let unauthorized = ApiError(StatusCode::UNAUTHORIZED, "login_needed");
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(unauthorized)?;
        let found = server
            .db()
            .session(&hash(token), server.now())
            .map_err(internal)?;
        let (user, device) = found.ok_or(ApiError(StatusCode::UNAUTHORIZED, "login_needed"))?;
        Ok(Session { user, device })
    }
}

/// The IP address of the caller, if known (only used for rate limits,
/// never stored).
pub struct ClientIp(Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|c| c.0.ip()),
        ))
    }
}

#[derive(Deserialize)]
struct Wait {
    #[serde(default)]
    wait: u64,
}

// ---------------------------------------------------------------- accounts

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Register {
    user: String,
    account_key: String,
    device_list: SignedDeviceList,
}

/// Register an account: its public key and its first device list, signed
/// by that key (which proves the caller holds it).
async fn register(
    State(s): State<Arc<Server>>,
    ClientIp(ip): ClientIp,
    Json(r): Json<Register>,
) -> ApiResult {
    if !s.allow("register", ip, REGISTRATIONS_PER_HOUR) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "slow_down"));
    }
    validate_name(&r.user).map_err(|_| bad("bad_user"))?;
    let key = hex::decode(&r.account_key)
        .ok()
        .filter(|k| k.len() == 32)
        .ok_or(bad("bad_key"))?;
    let list = r
        .device_list
        .verify(&key, None)
        .map_err(|_| bad("bad_device_list"))?;
    if list.user != r.user || list.devices.is_empty() {
        return Err(bad("bad_device_list"));
    }
    let seq = list.seq;
    let text = serde_json::to_string(&r.device_list).map_err(internal)?;
    let created = s
        .db()
        .insert_account(&r.user, &hex::encode(&key), &text, seq, s.now())
        .map_err(internal)?;
    if !created {
        return Err(ApiError(StatusCode::CONFLICT, "user_taken"));
    }
    Ok(Json(json!({"ok": true})))
}

fn load_account(s: &Server, user: &str) -> Result<(Vec<u8>, SignedDeviceList, u64), ApiError> {
    let row = s
        .db()
        .account(user)
        .map_err(internal)?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "unknown_user"))?;
    let key = hex::decode(&row.key).map_err(internal)?;
    let list: SignedDeviceList = serde_json::from_str(&row.list).map_err(internal)?;
    Ok((key, list, row.seq))
}

/// The public key and device list of an account, for checking its
/// devices. Only for logged-in devices.
async fn account(
    State(s): State<Arc<Server>>,
    _me: Session,
    Path(user): Path<String>,
) -> ApiResult {
    let (key, list, _) = load_account(&s, &user)?;
    Ok(Json(json!({
        "ok": true,
        "user": user,
        "account_key": hex::encode(key),
        "device_list": list,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteAccount {
    now: u64,
    signature: String,
}

/// Delete the own account. Needs a fresh signature of the account key,
/// not only a session.
async fn delete_account(
    State(s): State<Arc<Server>>,
    me: Session,
    Json(r): Json<DeleteAccount>,
) -> ApiResult {
    if r.now.abs_diff(s.now()) > 300 {
        return Err(bad("clock"));
    }
    let (key, _, _) = load_account(&s, &me.user)?;
    let signature = hex::decode(&r.signature).map_err(|_| bad("bad_signature"))?;
    verify_account_deletion(&key, &me.user, r.now, &signature)
        .map_err(|_| ApiError(StatusCode::FORBIDDEN, "bad_signature"))?;
    s.db().delete_account(&me.user).map_err(internal)?;
    Ok(Json(json!({"ok": true})))
}

// ---------------------------------------------------------------- login

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChallengeRequest {
    user: String,
    device: String,
}

/// A random challenge for one user and device, valid for 60 seconds.
/// Nobody can fill the server's memory with them: names are short,
/// each address gets a limited number, and there is an upper bound.
async fn challenge(
    State(s): State<Arc<Server>>,
    ClientIp(ip): ClientIp,
    Json(r): Json<ChallengeRequest>,
) -> ApiResult {
    if r.user.len() > 64 || r.device.len() > 64 {
        return Err(bad("too_long"));
    }
    if !s.allow("challenge", ip, CHALLENGES_PER_HOUR) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "slow_down"));
    }
    let c = random_hex::<CHALLENGE_LEN>()?;
    let now = s.now();
    let mut m = s.memory();
    if m.challenges.len() >= MAX_CHALLENGES {
        m.challenges
            .retain(|_, (_, _, at)| now < *at + CHALLENGE_LIFETIME);
        if m.challenges.len() >= MAX_CHALLENGES {
            return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "busy"));
        }
    }
    m.challenges.insert(c.clone(), (r.user, r.device, now));
    Ok(Json(json!({"ok": true, "challenge": c})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    user: String,
    device: String,
    challenge: String,
    signature: String,
}

/// Log in: the device signed the challenge. The device must be on the
/// account's current list (withdrawn devices cannot log in).
async fn login(State(s): State<Arc<Server>>, Json(r): Json<Login>) -> ApiResult {
    let refused = || ApiError(StatusCode::FORBIDDEN, "login_refused");
    let now = s.now();
    // A challenge works once, for the user and device it was made for.
    let issued = s.memory().challenges.remove(&r.challenge);
    match issued {
        Some((user, device, at))
            if user == r.user && device == r.device && now < at + CHALLENGE_LIFETIME => {}
        _ => return Err(refused()),
    }
    let (_, list, _) = load_account(&s, &r.user).map_err(|_| refused())?;
    let entry = list
        .list
        .devices
        .iter()
        .find(|d| d.id == r.device)
        .ok_or_else(refused)?;
    let challenge = hex::decode(&r.challenge).map_err(|_| refused())?;
    let signature = hex::decode(&r.signature).map_err(|_| refused())?;
    verify_login(entry, &r.user, &challenge, &signature).map_err(|_| refused())?;
    let token = random_hex::<32>()?;
    let db = s.db();
    db.insert_session(&hash(&token), &r.user, &r.device, now + SESSION_LIFETIME)
        .map_err(internal)?;
    db.touch(&r.user, now).map_err(internal)?;
    Ok(Json(
        json!({"ok": true, "token": token, "expires": now + SESSION_LIFETIME}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateDevices {
    device_list: SignedDeviceList,
}

/// A newer device list of the own account (after pairing or
/// withdrawing). Devices that left the list lose their sessions, key
/// packages and waiting messages at once.
async fn update_devices(
    State(s): State<Arc<Server>>,
    me: Session,
    Json(r): Json<UpdateDevices>,
) -> ApiResult {
    let (key, old, seq) = load_account(&s, &me.user)?;
    let list = r
        .device_list
        .verify(&key, Some(seq))
        .map_err(|_| ApiError(StatusCode::CONFLICT, "old_or_bad_list"))?;
    if list.user != me.user || list.devices.is_empty() {
        return Err(bad("bad_device_list"));
    }
    let new_seq = list.seq;
    let gone: Vec<String> = old
        .list
        .devices
        .iter()
        .filter(|d| !list.contains(&d.id))
        .map(|d| d.id.clone())
        .collect();
    let text = serde_json::to_string(&r.device_list).map_err(internal)?;
    let db = s.db();
    db.set_list(&me.user, &text, new_seq).map_err(internal)?;
    for device in &gone {
        db.drop_device(&me.user, device).map_err(internal)?;
    }
    Ok(Json(json!({"ok": true, "seq": new_seq, "removed": gone})))
}

// ---------------------------------------------------------------- key packages

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    packages: Vec<String>,
}

async fn upload_keypackages(
    State(s): State<Arc<Server>>,
    me: Session,
    Json(r): Json<Upload>,
) -> ApiResult {
    if r.packages.len() > MAX_KEYPACKAGES {
        return Err(bad("too_many"));
    }
    let packages = r
        .packages
        .iter()
        .map(|p| decode(p, MAX_KEYPACKAGE))
        .collect::<Result<Vec<_>, _>>()?;
    let stored = s
        .db()
        .add_keypackages(&me.user, &me.device, &packages, MAX_KEYPACKAGES)
        .map_err(internal)?;
    Ok(Json(json!({"ok": true, "stored": stored})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    user: String,
}

/// One key package for each current device of an account (to add the
/// account to a group). Each package is handed out once.
async fn claim_keypackages(
    State(s): State<Arc<Server>>,
    _me: Session,
    Json(r): Json<Claim>,
) -> ApiResult {
    let (_, list, _) = load_account(&s, &r.user)?;
    let db = s.db();
    let mut out = Vec::new();
    for d in &list.list.devices {
        if let Some(p) = db.claim_keypackage(&r.user, &d.id).map_err(internal)? {
            out.push(json!({"device": d.id, "package": B64.encode(p)}));
        }
    }
    Ok(Json(json!({"ok": true, "packages": out})))
}

// ---------------------------------------------------------------- messages

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct To {
    user: String,
    device: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendMessage {
    to: Vec<To>,
    body: String,
}

/// Queue an encrypted message for devices. The sender is not stored.
async fn send(State(s): State<Arc<Server>>, _me: Session, Json(r): Json<SendMessage>) -> ApiResult {
    if r.to.is_empty() || r.to.len() > MAX_RECIPIENTS {
        return Err(bad("recipients"));
    }
    let body = decode(&r.body, MAX_MESSAGE)?;
    // Check every recipient first: all or nothing.
    for t in &r.to {
        let (_, list, _) = load_account(&s, &t.user)
            .map_err(|_| ApiError(StatusCode::NOT_FOUND, "unknown_recipient"))?;
        if !list.list.contains(&t.device) {
            return Err(ApiError(StatusCode::NOT_FOUND, "unknown_recipient"));
        }
        if s.db().queue_len(&t.user, &t.device).map_err(internal)? >= MAX_QUEUE {
            return Err(ApiError(StatusCode::INSUFFICIENT_STORAGE, "queue_full"));
        }
    }
    let now = s.now();
    for t in &r.to {
        s.db()
            .push_message(&t.user, &t.device, &body, now)
            .map_err(internal)?;
        s.waiter(&t.user, &t.device).notify_waiters();
    }
    Ok(Json(json!({"ok": true})))
}

/// Waiting messages of this device. With `?wait=N` the answer waits up
/// to N seconds (at most 30) for a message to arrive.
async fn fetch(State(s): State<Arc<Server>>, me: Session, Query(w): Query<Wait>) -> ApiResult {
    let notify = s.waiter(&me.user, &me.device);
    let notified = notify.notified();
    tokio::pin!(notified);
    // Register before looking, so a message that arrives in between is
    // not missed.
    notified.as_mut().enable();
    let mut list = s
        .db()
        .messages(&me.user, &me.device, 100)
        .map_err(internal)?;
    if list.is_empty() && w.wait > 0 {
        let _ = tokio::time::timeout(Duration::from_secs(w.wait.min(MAX_WAIT)), notified).await;
        list = s
            .db()
            .messages(&me.user, &me.device, 100)
            .map_err(internal)?;
    }
    let messages: Vec<Value> = list
        .into_iter()
        .map(|(id, body, at)| json!({"id": id, "body": B64.encode(body), "at": at}))
        .collect();
    Ok(Json(json!({"ok": true, "messages": messages})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ack {
    ids: Vec<i64>,
}

/// Delete fetched messages.
async fn ack(State(s): State<Arc<Server>>, me: Session, Json(r): Json<Ack>) -> ApiResult {
    if r.ids.len() > 1000 {
        return Err(bad("too_many"));
    }
    let n = s.db().ack(&me.user, &me.device, &r.ids).map_err(internal)?;
    Ok(Json(json!({"ok": true, "deleted": n})))
}

// ---------------------------------------------------------------- pairing relay

fn slot_gone() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "no_such_pairing")
}

/// Run `f` on a live slot.
fn with_slot<T>(
    s: &Server,
    channel: u16,
    f: impl FnOnce(&mut Slot) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    let now = s.now();
    let mut m = s.memory();
    let alive = m
        .slots
        .get(&channel)
        .is_some_and(|slot| now < slot.created + PAIRING_LIFETIME);
    if !alive {
        m.slots.remove(&channel);
        return Err(slot_gone());
    }
    f(m.slots.get_mut(&channel).ok_or_else(slot_gone)?)
}

fn owner_only(slot: &Slot, me: &Session) -> Result<(), ApiError> {
    if slot.owner != (me.user.clone(), me.device.clone()) {
        return Err(slot_gone());
    }
    Ok(())
}

/// Long poll on a slot until `pick` finds something.
async fn wait_slot(
    s: &Server,
    channel: u16,
    wait: u64,
    pick: impl Fn(&Slot) -> Option<Value>,
) -> ApiResult {
    let notify = with_slot(s, channel, |slot| Ok(slot.notify.clone()))?;
    let notified = notify.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    if let Some(v) = with_slot(s, channel, |slot| Ok(pick(slot)))? {
        return Ok(Json(v));
    }
    if wait > 0 {
        let _ = tokio::time::timeout(Duration::from_secs(wait.min(MAX_WAIT)), notified).await;
        if let Some(v) = with_slot(s, channel, |slot| Ok(pick(slot)))? {
            return Ok(Json(v));
        }
    }
    Ok(Json(json!({"ok": true, "ready": false})))
}

/// The unlocked device asks for a channel number for its pairing code.
async fn open_pairing(State(s): State<Arc<Server>>, me: Session) -> ApiResult {
    let now = s.now();
    let mut m = s.memory();
    m.slots
        .retain(|_, slot| now < slot.created + PAIRING_LIFETIME);
    let open = m
        .slots
        .values()
        .filter(|slot| slot.owner.0 == me.user)
        .count();
    if open >= MAX_OPEN_PAIRINGS {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "slow_down"));
    }
    for _ in 0..64 {
        let mut b = [0u8; 2];
        getrandom::fill(&mut b).map_err(internal)?;
        let channel = 1 + u16::from_le_bytes(b) % 9999;
        if let std::collections::hash_map::Entry::Vacant(free) = m.slots.entry(channel) {
            free.insert(Slot {
                created: now,
                owner: (me.user.clone(), me.device.clone()),
                host_hello: None,
                guest: None,
                bundle: None,
                notify: Arc::default(),
            });
            return Ok(Json(
                json!({"ok": true, "channel": channel, "expires": now + PAIRING_LIFETIME}),
            ));
        }
    }
    Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "busy"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostHello {
    hello: String,
}

async fn put_host_hello(
    State(s): State<Arc<Server>>,
    me: Session,
    Path(channel): Path<u16>,
    Json(r): Json<HostHello>,
) -> ApiResult {
    let hello = decode(&r.hello, MAX_PAIRING_MESSAGE)?;
    with_slot(&s, channel, |slot| {
        owner_only(slot, &me)?;
        if slot.host_hello.is_some() {
            return Err(ApiError(StatusCode::CONFLICT, "already_sent"));
        }
        slot.host_hello = Some(hello);
        slot.notify.notify_waiters();
        Ok(())
    })?;
    Ok(Json(json!({"ok": true})))
}

/// The new device (no account yet) fetches the host hello.
async fn get_host_hello(
    State(s): State<Arc<Server>>,
    Path(channel): Path<u16>,
    Query(w): Query<Wait>,
) -> ApiResult {
    wait_slot(&s, channel, w.wait, |slot| {
        slot.host_hello
            .as_ref()
            .map(|h| json!({"ok": true, "ready": true, "hello": B64.encode(h)}))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Guest {
    hello: String,
    offer: String,
}

/// The new device sends its hello and offer. Only one attempt per
/// channel: a guesser gets exactly one try.
async fn put_guest(
    State(s): State<Arc<Server>>,
    ClientIp(ip): ClientIp,
    Path(channel): Path<u16>,
    Json(r): Json<Guest>,
) -> ApiResult {
    if !s.allow("guest", ip, GUEST_ATTEMPTS_PER_HOUR) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "slow_down"));
    }
    let hello = decode(&r.hello, MAX_PAIRING_MESSAGE)?;
    let offer = decode(&r.offer, MAX_PAIRING_MESSAGE)?;
    with_slot(&s, channel, |slot| {
        if slot.host_hello.is_none() {
            return Err(ApiError(StatusCode::CONFLICT, "not_ready"));
        }
        if slot.guest.is_some() {
            return Err(ApiError(StatusCode::CONFLICT, "used"));
        }
        slot.guest = Some((hello, offer));
        slot.notify.notify_waiters();
        Ok(())
    })?;
    Ok(Json(json!({"ok": true})))
}

/// The host fetches the guest's hello and offer.
async fn get_guest(
    State(s): State<Arc<Server>>,
    me: Session,
    Path(channel): Path<u16>,
    Query(w): Query<Wait>,
) -> ApiResult {
    with_slot(&s, channel, |slot| owner_only(slot, &me))?;
    wait_slot(&s, channel, w.wait, |slot| {
        slot.guest.as_ref().map(|(h, o)| {
            json!({"ok": true, "ready": true, "hello": B64.encode(h), "offer": B64.encode(o)})
        })
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    bundle: String,
}

async fn put_bundle(
    State(s): State<Arc<Server>>,
    me: Session,
    Path(channel): Path<u16>,
    Json(r): Json<Bundle>,
) -> ApiResult {
    let bundle = decode(&r.bundle, MAX_PAIRING_MESSAGE)?;
    with_slot(&s, channel, |slot| {
        owner_only(slot, &me)?;
        if slot.guest.is_none() || slot.bundle.is_some() {
            return Err(ApiError(StatusCode::CONFLICT, "not_ready"));
        }
        slot.bundle = Some(bundle);
        slot.notify.notify_waiters();
        Ok(())
    })?;
    Ok(Json(json!({"ok": true})))
}

/// The new device fetches the encrypted bundle. Only the device that
/// knows the code can open it.
async fn get_bundle(
    State(s): State<Arc<Server>>,
    Path(channel): Path<u16>,
    Query(w): Query<Wait>,
) -> ApiResult {
    wait_slot(&s, channel, w.wait, |slot| {
        slot.bundle
            .as_ref()
            .map(|b| json!({"ok": true, "ready": true, "bundle": B64.encode(b)}))
    })
    .await
}
