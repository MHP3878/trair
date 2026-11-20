// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! What the app does around the door, in Rust: open the vault, log in,
//! keep key packages and changes going, receive, send. Ported from
//! `app/lib/app_state.dart` (connect, _chatsOnline, _receiveLoop,
//! _pushCommit, startChat, ...); the core decides everything that matters
//! for security, as in the app.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Value, json};

use crate::door::{self, call, ok};
use crate::net::Api;

/// Key packages the server should hold for this device at least.
const MIN_KEY_PACKAGES: i64 = 10;
/// How many to make when they run low.
const NEW_KEY_PACKAGES: i64 = 20;

/// Open (or make) the vault in `dir`. `create`: the profile for a new
/// account; without it, a missing vault is an error. Returns the stored
/// profile.
pub fn open_vault(dir: &Path, password: &str, create: Option<Value>) -> Result<Value, String> {
    let dir = std::path::absolute(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let init = call(json!({"op": "init", "dir": dir}));
    if !ok(&init) {
        return Err(match door::error(&init).as_str() {
            "in_use" => "Dieser Bot läuft schon (der Ordner ist in Benutzung).".into(),
            e => format!("Ordner nicht nutzbar ({e})."),
        });
    }
    let opened = match (init["exists"] == true, create) {
        (false, Some(profile)) => {
            call(json!({"op": "create", "password": password, "profile": profile}))
        }
        (false, None) => return Err("In diesem Ordner gibt es noch keinen Bot.".into()),
        (true, Some(_)) => return Err("In diesem Ordner gibt es schon einen Bot.".into()),
        (true, None) => call(json!({"op": "unlock", "password": password})),
    };
    if !ok(&opened) {
        return Err(match door::error(&opened).as_str() {
            "wrong_password" | "wait" => "Passwort falsch.".into(),
            "weak_password" => "Das Passwort ist zu kurz (mindestens 12 Zeichen).".into(),
            e => format!("Tresor ließ sich nicht öffnen ({e})."),
        });
    }
    if opened["chats"] != true {
        return Err("Die Chat-Datenbank ließ sich nicht öffnen.".into());
    }
    Ok(opened["profile"].clone())
}

/// Something that happened, for the bot's code.
pub type Event = Value;

/// A logged-in account (a bot, or in tests a person).
pub struct Client {
    /// The connection.
    pub api: Api,
    /// This account's name on the server.
    pub user: String,
    /// Names to show: contacts by the name given them, others by the name
    /// they chose.
    names: HashMap<String, String>,
    /// Chats as last seen, by id: (kind, state).
    chats: HashMap<String, (String, String)>,
}

impl Client {
    /// Log in at `server`, registering the account first if needed (as a
    /// bot or not), and bring the device lists in line.
    pub fn connect(server: &str, bot: bool) -> Result<Client, String> {
        let mut api = Api::new(server)
            .ok_or("Bis TLS fertig ist, nur ein Server auf diesem PC: http://127.0.0.1:PORT")?;
        let health = api.health();
        if !health.ok() {
            return Err(format!(
                "Server nicht erreichbar ({}). Läuft trair-server?",
                health.error()
            ));
        }
        let me = call(json!({"op": "server_identity"}));
        if !ok(&me) {
            return Err(format!("Tresor gesperrt ({}).", door::error(&me)));
        }
        let user = me["user"].as_str().unwrap_or_default().to_string();
        let device = me["device"].as_str().unwrap_or_default().to_string();
        let registered = api.register(&me, bot);
        if !registered.ok() && registered.error() != "user_taken" {
            return Err(format!(
                "Anmelden beim Server ging nicht ({}).",
                registered.error()
            ));
        }
        let challenge = api.challenge(&user, &device);
        let c = challenge.body["challenge"].as_str().unwrap_or_default();
        let signed = call(json!({"op": "sign_login", "challenge": c}));
        let login = api.login(
            &user,
            &device,
            c,
            signed["signature"].as_str().unwrap_or_default(),
        );
        if !challenge.ok() || !ok(&signed) || !login.ok() {
            return Err("Der Server kennt dieses Gerät nicht.".into());
        }
        api.token = login.body["token"].as_str().map(String::from);
        let account = api.account(&user);
        if account.ok() {
            let applied = call(json!({"op": "apply_device_list",
                "device_list": account.body["device_list"]}));
            if door::error(&applied) == "withdrawn" {
                return Err("Dieses Gerät wurde von einem anderen Gerät entzogen.".into());
            }
        }
        api.put_devices(&me["device_list"]);
        let mut client = Client {
            api,
            user,
            names: HashMap::new(),
            chats: HashMap::new(),
        };
        client.refresh();
        Ok(client)
    }

    /// After logging in: key packages, waiting changes, contacts' device
    /// lists, groups that need tidying, waiting join requests.
    pub fn online(&mut self) {
        self.top_up();
        let pending = call(json!({"op": "pending_commits"}));
        for c in pending["commits"].as_array().cloned().unwrap_or_default() {
            self.push_commit(&c);
        }
        for c in call(json!({"op": "contacts"}))["contacts"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let acc = self.api.account(c["user"].as_str().unwrap_or_default());
            if acc.ok() {
                call(json!({"op": "apply_contact_list",
                    "account_key": acc.body["account_key"], "device_list": acc.body["device_list"]}));
            }
        }
        self.remove_withdrawn();
        self.tidy_groups(None);
        for j in call(json!({"op": "join_requests"}))["requests"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            self.admit(
                j["chat"].as_str().unwrap_or_default(),
                j["user"].as_str().unwrap_or_default(),
            );
        }
        self.refresh();
    }

    /// Keep enough key packages on the server, so others can start chats.
    pub fn top_up(&self) {
        let count = self.api.key_package_count();
        if !count.ok() {
            return;
        }
        let have = count.body["count"].as_i64().unwrap_or(0);
        if have >= MIN_KEY_PACKAGES {
            return;
        }
        let kps =
            call(json!({"op": "key_packages", "n": NEW_KEY_PACKAGES, "last_resort": have == 0}));
        if ok(&kps) {
            self.api
                .upload_key_packages(&kps["packages"], &kps["last_resort"]);
        }
    }

    /// Hand a change to the server and tell the core the answer. Without
    /// an answer it stays waiting and goes out again on the next start.
    pub fn push_commit(&self, commit: &Value) -> bool {
        let r = self.api.post_commit(commit);
        if r.status == 0 {
            return false;
        }
        let mut answer = json!({"op": "commit_answer", "chat": commit["chat"], "accepted": r.ok()});
        if let Some(t) = r.body["taken"].as_str() {
            answer["taken"] = json!(t);
        }
        let a = call(answer);
        ok(&a) && a["applied"] == true
    }

    fn remove_withdrawn(&self) {
        for (id, (kind, state)) in &self.chats {
            if kind != "direct" || state != "active" {
                continue;
            }
            let w = call(json!({"op": "withdrawn_members", "chat": id}));
            let ids = w["ids"].as_array().cloned().unwrap_or_default();
            if ids.is_empty() {
                continue;
            }
            let out = call(json!({"op": "remove_devices", "chat": id, "ids": ids}));
            if ok(&out) {
                self.push_commit(&out["commit"]);
            }
        }
    }

    fn tidy_groups(&self, only: Option<&[String]>) {
        for (id, (kind, state)) in &self.chats {
            if kind != "group" || state != "active" || only.is_some_and(|o| !o.contains(id)) {
                continue;
            }
            let t = call(json!({"op": "tidy_group", "chat": id}));
            if !ok(&t) {
                continue;
            }
            if t["commit"].is_object() {
                self.push_commit(&t["commit"]);
            } else if let Some(body) = t["body"].as_str() {
                self.api.send_message(&t["to"], body);
            }
        }
    }

    fn admit(&self, join_chat: &str, user: &str) {
        let offers = self.api.claim(user);
        let a = call(json!({"op": "admit", "chat": join_chat, "offers": offers}));
        if ok(&a) {
            if self.push_commit(&a["commit"]) {
                call(json!({"op": "close_join", "chat": join_chat}));
            }
        } else if door::error(&a) == "rejected" {
            call(json!({"op": "close_join", "chat": join_chat}));
        }
    }

    /// Read chats and names from the core again.
    pub fn refresh(&mut self) {
        let chats = call(json!({"op": "chats"}));
        if let Some(list) = chats["chats"].as_array() {
            self.chats = list
                .iter()
                .filter_map(|c| {
                    Some((
                        c["id"].as_str()?.to_string(),
                        (
                            c["kind"].as_str()?.to_string(),
                            c["state"].as_str()?.to_string(),
                        ),
                    ))
                })
                .collect();
        }
        let mut names = HashMap::new();
        for p in call(json!({"op": "profiles"}))["profiles"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            if let (Some(u), Some(n)) = (p["user"].as_str(), p["name"].as_str()) {
                names.insert(u.to_string(), n.to_string());
            }
        }
        for c in call(json!({"op": "contacts"}))["contacts"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            if let (Some(u), Some(n)) = (c["user"].as_str(), c["name"].as_str()) {
                names.insert(u.to_string(), n.to_string());
            }
        }
        self.names = names;
    }

    /// How to call `user`.
    pub fn name_of(&self, user: &str) -> String {
        self.names
            .get(user)
            .cloned()
            .unwrap_or_else(|| user.to_string())
    }

    fn kind_of(&self, chat: &str) -> String {
        self.chats
            .get(chat)
            .map_or_else(|| "direct".into(), |(k, _)| k.clone())
    }

    /// Wait up to `wait` seconds for messages: `(server id, body)`.
    pub fn fetch(&self, wait: u64) -> Result<Vec<(i64, String)>, String> {
        let r = self.api.fetch(wait);
        if !r.ok() {
            return Err(r.error());
        }
        Ok(r.body["messages"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| Some((m["id"].as_i64()?, m["body"].as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Let the core process what arrived, act like the app does, tell the
    /// server what may go, and say what happened.
    pub fn process(&mut self, messages: &[(i64, String)]) -> Vec<Event> {
        let mut ids = Vec::new();
        let mut events = Vec::new();
        let mut groups = Vec::new();
        let mut refresh = false;
        for (id, body) in messages {
            let got = call(json!({"op": "receive", "body": body}));
            // Keep it on the server only if this device could not store it.
            if !matches!(
                door::error(&got).as_str(),
                "storage" | "internal" | "locked" | "chats_unavailable"
            ) {
                ids.push(*id);
            }
            let incoming = &got["incoming"];
            match incoming["kind"].as_str().unwrap_or("") {
                "message" => {
                    let m = &incoming["message"];
                    if m["outgoing"] == true {
                        continue;
                    }
                    let chat = m["chat"].as_str().unwrap_or_default();
                    let from = m["user"].as_str().unwrap_or_default();
                    events.push(json!({
                        "event": "message",
                        "chat": chat,
                        "kind": self.kind_of(chat),
                        "from": from,
                        "name": self.name_of(from),
                        "text": m["text"],
                        "id": m["id"],
                        "at": m["at"],
                    }));
                }
                "chat" => {
                    let c = &incoming["chat"];
                    let chat = c["id"].as_str().unwrap_or_default().to_string();
                    if incoming["update_keys"] == true {
                        let out = call(json!({"op": "update_keys", "chat": chat}));
                        if ok(&out) {
                            self.push_commit(&out["commit"]);
                        } else if door::error(&out) == "not_allowed" {
                            let ask = call(json!({"op": "propose_update", "chat": chat}));
                            if let Some(b) = ask["body"].as_str() {
                                self.api.send_message(&ask["to"], b);
                            }
                        }
                    }
                    let before = self.chats.get(&chat).map(|(_, s)| s.clone());
                    let state = c["state"].as_str().unwrap_or_default();
                    let peer = c["peer"].as_str().unwrap_or_default();
                    if before.as_deref() != Some(state) {
                        let event = match state {
                            "request" => "request",
                            "active" => "joined",
                            _ => "left",
                        };
                        events.push(json!({
                            "event": event,
                            "chat": chat,
                            "kind": c["kind"],
                            "name": c["name"],
                            "from": peer,
                        }));
                    }
                    self.chats.insert(
                        chat,
                        (
                            c["kind"].as_str().unwrap_or("direct").to_string(),
                            state.to_string(),
                        ),
                    );
                    refresh = true;
                }
                "changed" | "group" => {
                    let chat = incoming["chat"].as_str().unwrap_or_default().to_string();
                    events.push(json!({
                        "event": "changed",
                        "chat": chat,
                        "added": incoming["added"],
                        "removed": incoming["removed"],
                        "left": incoming["left"] == true,
                    }));
                    groups.push(chat);
                    refresh = true;
                }
                "join_request" => {
                    self.admit(
                        incoming["chat"].as_str().unwrap_or_default(),
                        incoming["user"].as_str().unwrap_or_default(),
                    );
                    groups.push(incoming["group"].as_str().unwrap_or_default().to_string());
                    refresh = true;
                }
                "join_refused" => {
                    let out = call(json!({"op": "deny_join", "chat": incoming["chat"]}));
                    if let Some(b) = out["body"].as_str() {
                        self.api.send_message(&out["to"], b);
                    }
                }
                "profile" | "ends" => refresh = true,
                _ => {}
            }
        }
        if !ids.is_empty() {
            self.api.ack(&ids);
        }
        if refresh {
            self.refresh();
        }
        if !groups.is_empty() {
            self.tidy_groups(Some(&groups));
        }
        events
    }

    /// Send a text. Returns the stored message's id.
    pub fn send(&mut self, chat: &str, text: &str) -> Result<Value, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("empty".into());
        }
        let sent = call(json!({"op": "send_message", "chat": chat, "text": text}));
        if !ok(&sent) {
            return Err(door::error(&sent));
        }
        let r = self
            .api
            .send_message(&sent["to"], sent["body"].as_str().unwrap_or_default());
        if !r.ok() {
            return Err(r.error());
        }
        Ok(sent["message"]["id"].clone())
    }

    /// Add a contact from their code; returns their account name.
    pub fn add_contact(&mut self, code: &str, name: &str) -> Result<String, String> {
        let a = call(json!({"op": "add_contact", "code": code.trim(), "name": name.trim()}));
        if !ok(&a) {
            return Err(door::error(&a));
        }
        self.refresh();
        Ok(a["contact"]["user"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// Start a chat with a contact; returns the chat id.
    pub fn start_chat(&mut self, user: &str) -> Result<String, String> {
        let mut offers = self.api.claim(user);
        offers.extend(self.api.claim(&self.user));
        let started = call(json!({"op": "start_chat", "user": user, "offers": offers}));
        if !ok(&started) {
            return Err(door::error(&started));
        }
        if !self.push_commit(&started["commit"]) {
            return Err("not_taken".into());
        }
        let id = started["chat"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let invite = call(json!({"op": "invite_message", "chat": id}));
        if let Some(b) = invite["body"].as_str() {
            self.api.send_message(&invite["to"], b);
        }
        self.refresh();
        Ok(id)
    }

    /// Take a chat request or a group invitation.
    pub fn accept(&mut self, chat: &str) -> Result<(), String> {
        let a = match self.chats.get(chat).map(|(k, _)| k.as_str()) {
            Some("group") => call(json!({"op": "accept_group", "chat": chat})),
            Some("direct") => {
                let name = call(json!({"op": "chats"}))["chats"]
                    .as_array()
                    .and_then(|l| l.iter().find(|c| c["id"] == chat).cloned())
                    .map(|c| c["peer"].as_str().unwrap_or_default().to_string())
                    .map(|peer| self.name_of(&peer))
                    .unwrap_or_default();
                call(json!({"op": "accept_request", "chat": chat, "name": name}))
            }
            _ => return Err("not_found".into()),
        };
        self.refresh();
        if ok(&a) { Ok(()) } else { Err(door::error(&a)) }
    }

    /// Turn down a request.
    pub fn decline(&mut self, chat: &str) -> Result<(), String> {
        let a = match self.chats.get(chat).map(|(k, _)| k.as_str()) {
            Some("group") => return self.leave(chat),
            Some(_) => call(json!({"op": "decline_request", "chat": chat})),
            None => return Err("not_found".into()),
        };
        self.refresh();
        if ok(&a) { Ok(()) } else { Err(door::error(&a)) }
    }

    /// Leave a group (an owner hands on first, an admin steps down).
    pub fn leave(&mut self, chat: &str) -> Result<(), String> {
        let prep = call(json!({"op": "leave_prepare", "chat": chat}));
        if ok(&prep) && prep["commit"].is_object() && !self.push_commit(&prep["commit"]) {
            return Err("busy".into());
        }
        let out = call(json!({"op": "leave_group", "chat": chat}));
        if !ok(&out) {
            return Err(door::error(&out));
        }
        self.api
            .send_message(&out["to"], out["body"].as_str().unwrap_or_default());
        self.refresh();
        Ok(())
    }

    /// Hand this account's profile to every chat that lacks this version.
    pub fn share_profile(&self, profile: &Value) -> Result<usize, String> {
        let out = call(json!({"op": "share_profile", "profile": profile, "only_contacts": false}));
        if !ok(&out) {
            return Err(door::error(&out));
        }
        let mut n = 0;
        for o in out["out"].as_array().cloned().unwrap_or_default() {
            if self
                .api
                .send_message(&o["to"], o["body"].as_str().unwrap_or_default())
                .ok()
            {
                call(json!({"op": "profile_sent", "chat": o["chat"], "rev": o["rev"]}));
                n += 1;
            }
        }
        Ok(n)
    }

    /// This account's bot certificate on the server, if its owner vouched.
    pub fn bot_cert(&self) -> Option<Value> {
        let r = self.api.account(&self.user);
        let cert = &r.body["bot"]["cert"];
        cert.is_object().then(|| cert.clone())
    }

    /// As a bot: a new request code for the owner (the server gets the
    /// hash of its secret).
    pub fn ask_owner(&self) -> Result<String, String> {
        let req = call(json!({"op": "bot_request"}));
        if !ok(&req) {
            return Err(door::error(&req));
        }
        let r = self
            .api
            .bot_request(req["hash"].as_str().unwrap_or_default());
        if !r.ok() {
            return Err(r.error());
        }
        Ok(req["code"].as_str().unwrap_or_default().to_string())
    }

    /// As an owner: vouch for the bot behind a request code.
    pub fn vouch(&self, code: &str) -> Result<(), String> {
        let v = call(json!({"op": "vouch_bot", "code": code}));
        if !ok(&v) {
            return Err(door::error(&v));
        }
        let r = self
            .api
            .vouch(&v["cert"], v["secret"].as_str().unwrap_or_default());
        if r.ok() { Ok(()) } else { Err(r.error()) }
    }

    /// Chats as the core lists them.
    pub fn chats(&self) -> Value {
        call(json!({"op": "chats"}))["chats"].clone()
    }

    /// The last `limit` messages of a chat (oldest first).
    pub fn messages(&self, chat: &str, limit: u64) -> Result<Value, String> {
        let m = call(json!({"op": "messages", "chat": chat, "limit": limit}));
        if ok(&m) {
            Ok(m["messages"].clone())
        } else {
            Err(door::error(&m))
        }
    }

    /// This account's contact code.
    pub fn contact_code(&self) -> String {
        call(json!({"op": "contact_code"}))["code"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    /// Delete chats whose time ran out (vanishing chats).
    pub fn sweep(&mut self) {
        let a = call(json!({"op": "sweep"}));
        if a["deleted"].as_array().is_some_and(|d| !d.is_empty()) {
            self.refresh();
        }
    }
}
