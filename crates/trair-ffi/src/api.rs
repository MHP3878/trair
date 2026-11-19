// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The protocol behind the door. One request in, one answer out, both
//! JSON. Answers always carry `"ok"`; failures carry an `"error"` code
//! from the list below and never any secret.
//!
//! | op                | fields                 | answer on success                |
//! |-------------------|------------------------|----------------------------------|
//! | `init`            | `dir`                  | `exists`, `unlocked`             |
//! | `status`          |                        | `exists`, `unlocked`             |
//! | `create`          | `password`, `profile`  | `fingerprint`, `profile`, `device`, `devices` |
//! | `unlock`          | `password`             | same as `create`, plus `upgraded` |
//! | `lock`            |                        |                                  |
//! | `save_profile`    | `profile`              |                                  |
//! | `change_password` | `old`, `new`           |                                  |
//! | `delete_account`  | `password`             |                                  |
//! | `protect_window`  | `on`                   | `windows` (how many were set)    |
//! | `qr`              | `text`                 | `size`, `rows` (`1` dark, `0` light) |
//! | `pick_image`      |                        | `path`; errors `cancelled`, `unsupported` |
//! | `rename_device`   | `id`, `label`          | `devices`                        |
//! | `withdraw_device` | `id`, `password`       | `devices`                        |
//! | `verify_badges`   | `holder`, `name`, `badges` | `badges`, `refused`, `name_warning` |
//! | `add_badge`       | `badge`                | `badges`                         |
//! | `server_identity` |                        | `user`, `device`, `account_key`, `device_list` |
//! | `sign_login`      | `challenge` (hex)      | `signature` (hex)                |
//! | `sign_deletion`   | `now`                  | `signature` (hex)                |
//! | `pair_host_start` | `channel`              | `code`, `hello`                  |
//! | `pair_host_finish`| `hello`, `offer`       | `bundle`, `device_list`, `devices` |
//! | `pair_guest_start`| `code`                 | `channel`, `hello`               |
//! | `pair_guest_offer`| `hello`                | `offer`                          |
//! | `pair_guest_finish`| `bundle`, `password`  | like `create`                    |
//! | `pair_cancel`     |                        |                                  |
//! | `apply_device_list` | `device_list`        | `devices`, `changed`; error `withdrawn` |
//! | `contact_code`    |                        | `code`                           |
//! | `add_contact`     | `code`, `name`         | `contact`                        |
//! | `contacts`        |                        | `contacts`                       |
//! | `confirm_contact` | `user`                 |                                  |
//! | `chats`           |                        | `chats`                          |
//! | `messages`        | `chat`, `before`?, `limit`? | `messages` (oldest first)   |
//! | `start_chat`      | `user`, `offers`       | `chat`, `commit`                 |
//! | `add_devices`     | `chat`, `offers`       | `commit`                         |
//! | `remove_devices`  | `chat`, `ids`          | `commit`                         |
//! | `update_keys`     | `chat`                 | `commit`                         |
//! | `withdrawn_members` | `chat`               | `ids`                            |
//! | `pending_commits` |                        | `commits`                        |
//! | `commit_answer`   | `chat`, `accepted`, `taken`? | `applied`                  |
//! | `send_message`    | `chat`, `text`         | `body`, `to`, `message`          |
//! | `receive`         | `body`                 | `incoming` (`kind`: chat, message, changed, duplicate, other) |
//! | `accept_request`  | `chat`, `name`         | `contact`                        |
//! | `decline_request` | `chat`                 |                                  |
//! | `key_packages`    | `n`, `last_resort`     | `packages`, `last_resort`        |
//! | `apply_contact_list` | `account_key`, `device_list` | `withdrawn`              |
//!
//! The chat ops (M4) need an unlocked vault; their details are in
//! `chats.rs`. `create`, `unlock` and `pair_guest_finish` also answer
//! `chats: true|false` (false: the chat database could not be opened).
//!
//! The server ops (M3, step 3) only compute; the app sends the results
//! over the network. Pairing keys stay in here: the app only ever sees
//! the code it has to show and messages for the relay (base64).
//!
//! `account_key` is the public account key in hex (for the team tool);
//! `badges` are the own badges that are valid now.
//! `device` is this device's id; `devices` lists the account's devices as
//! `{id, label, fingerprint, this}` (label may be null). `upgraded` is true
//! once, when a vault from Paket 1 got its device key and was stored again.
//!
//! `verify_badges` checks the badges of a profile (`holder`: the account
//! key in hex, `name`: the display name, `badges`: signed badges as JSON)
//! and returns only the valid ones as `{kind, community, community_id,
//! community_warning, expires}`. It needs no unlocked vault.
//!
//! `init` locks the folder for this process; another Trair window gets
//! `in_use` for the same folder.
//!
//! Error codes: `bad_request`, `not_initialized`, `exists`, `no_vault`, `in_use`,
//! `locked`, `wrong_password` (with `wait` in seconds), `wait` (with
//! `seconds`), `weak_password`, `damaged`, `refused`, `too_large`, `io`,
//! `unsupported`, `internal`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::Deserialize;
use serde_json::{Value, json};
use trair_core::SignedDeviceList;
use trair_core::badges::{self, Badge};
use trair_core::pairing::{Code, PairingGuest, PairingHost};
use trair_core::proofs;
use trair_core::vault::{Vault, VaultError};
use zeroize::Zeroizing;

use crate::chats::{self, Offer};
use crate::notfall;
use crate::platform;
use trair_core::Messenger;
use trair_core::notfall::After;

/// Name of the vault file in the data folder.
pub const VAULT_FILE: &str = "vault.trair";
/// Largest request accepted, in bytes.
pub const MAX_REQUEST: usize = 1024 * 1024;
/// Wrong passwords allowed before a wait is enforced.
pub const FREE_ATTEMPTS: u32 = 3;
/// Longest wait after many wrong passwords.
pub const MAX_WAIT: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Init {
        dir: String,
    },
    // Empty braces, not unit variants: serde only refuses unknown fields
    // for variants with braces, and the door refuses everything unexpected.
    Status {},
    Create {
        password: Zeroizing<String>,
        profile: Value,
    },
    Unlock {
        password: Zeroizing<String>,
    },
    Lock {},
    SaveProfile {
        profile: Value,
    },
    ChangePassword {
        old: Zeroizing<String>,
        new: Zeroizing<String>,
    },
    DeleteAccount {
        password: Zeroizing<String>,
    },
    ProtectWindow {
        on: bool,
    },
    /// A QR code for `text` (a contact code). Needs no vault.
    Qr {
        text: String,
    },
    /// The Windows file dialog for a profile picture.
    PickImage {},
    RenameDevice {
        id: String,
        label: String,
    },
    WithdrawDevice {
        id: String,
        password: Zeroizing<String>,
    },
    VerifyBadges {
        holder: String,
        name: String,
        badges: Vec<Value>,
    },
    AddBadge {
        badge: Value,
    },
    ServerIdentity {},
    SignLogin {
        challenge: String,
    },
    SignDeletion {
        now: u64,
    },
    PairHostStart {
        channel: u16,
    },
    PairHostFinish {
        hello: String,
        offer: String,
    },
    PairGuestStart {
        code: String,
    },
    PairGuestOffer {
        hello: String,
    },
    PairGuestFinish {
        bundle: String,
        password: Zeroizing<String>,
    },
    PairCancel {},
    ApplyDeviceList {
        device_list: Value,
    },
    ContactCode {},
    AddContact {
        code: String,
        name: String,
    },
    Contacts {},
    ConfirmContact {
        user: String,
    },
    Chats {},
    Messages {
        chat: String,
        before: Option<i64>,
        limit: Option<u32>,
    },
    StartChat {
        user: String,
        offers: Vec<Offer>,
    },
    AddDevices {
        chat: String,
        offers: Vec<Offer>,
    },
    RemoveDevices {
        chat: String,
        ids: Vec<String>,
    },
    UpdateKeys {
        chat: String,
    },
    WithdrawnMembers {
        chat: String,
    },
    PendingCommits {},
    CommitAnswer {
        chat: String,
        accepted: bool,
        taken: Option<String>,
    },
    SendMessage {
        chat: String,
        text: String,
    },
    Receive {
        body: String,
    },
    ShareProfile {
        profile: Value,
        only_contacts: bool,
    },
    ProfileSent {
        chat: String,
        rev: u64,
    },
    Profiles {},
    CreateInvite {
        minutes: u64,
        #[serde(default)]
        vanishing: bool,
    },
    InviteMessage {
        chat: String,
    },
    SetChatEnd {
        chat: String,
        minutes: Option<u64>,
    },
    Sweep {},
    #[serde(rename = "accept_request")]
    AcceptChat {
        chat: String,
        name: String,
    },
    #[serde(rename = "decline_request")]
    DeclineChat {
        chat: String,
    },
    KeyPackages {
        n: usize,
        last_resort: bool,
    },
    ApplyContactList {
        account_key: String,
        device_list: Value,
    },
    NotfallList {
        password: Zeroizing<String>,
    },
    NotfallAdd {
        pin: Zeroizing<String>,
        password: Zeroizing<String>,
        after: String,
        chat: Option<String>,
        text: Option<String>,
    },
    NotfallRemove {
        id: String,
        password: Zeroizing<String>,
    },
    NotfallSignLogin {
        challenge: String,
    },
    NotfallSignDeletion {
        now: u64,
    },
    NotfallFinish {},
}

#[derive(Default)]
struct State {
    dir: Option<PathBuf>,
    vault: Option<Vault>,
    failures: u32,
    blocked_until: Option<Instant>,
    pair_host: Option<PairingHost>,
    pair_guest: Option<PairingGuest>,
    /// The chat database, open while the vault is.
    chats: Option<Messenger>,
    /// Held while this process uses the data folder: a second Trair
    /// window cannot open the same account at the same time.
    folder_lock: Option<std::fs::File>,
    /// An emergency PIN that sends is running.
    notfall: Option<notfall::Session>,
}

static STATE: Mutex<State> = Mutex::new(State {
    dir: None,
    vault: None,
    failures: 0,
    blocked_until: None,
    pair_host: None,
    pair_guest: None,
    chats: None,
    folder_lock: None,
    notfall: None,
});

/// Handle one request. Never panics on bad input; a poisoned lock (a
/// panic in an earlier call) locks the vault and starts over.
pub fn handle(request: &[u8]) -> Vec<u8> {
    let answer = if request.len() > MAX_REQUEST {
        fail("bad_request")
    } else {
        match serde_json::from_slice::<Request>(request) {
            Ok(req) => {
                let mut state = match STATE.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        let mut guard = poisoned.into_inner();
                        guard.vault = None;
                        guard.chats = None;
                        STATE.clear_poison();
                        guard
                    }
                };
                run(&mut state, req)
            }
            Err(_) => fail("bad_request"),
        }
    };
    serde_json::to_vec(&answer).unwrap_or_else(|_| br#"{"ok":false,"error":"internal"}"#.to_vec())
}

fn run(state: &mut State, req: Request) -> Value {
    match req {
        Request::Init { dir } => init(state, &dir),
        Request::Status {} => status(state),
        Request::Create { password, profile } => create(state, &password, profile),
        Request::Unlock { password } => unlock(state, &password),
        Request::Lock {} => {
            state.vault = None;
            state.chats = None;
            state.notfall = None;
            state.pair_host = None;
            json!({"ok": true})
        }
        Request::SaveProfile { profile } => save_profile(state, profile),
        Request::ChangePassword { old, new } => change_password(state, &old, &new),
        Request::DeleteAccount { password } => delete_account(state, &password),
        Request::ProtectWindow { on } => match platform::protect_windows(on) {
            Some(n) => json!({"ok": true, "windows": n}),
            None => fail("unsupported"),
        },
        Request::Qr { text } => qr(&text),
        Request::PickImage {} => match platform::pick_image() {
            Some(Some(path)) => json!({"ok": true, "path": path}),
            Some(None) => fail("cancelled"),
            None => fail("unsupported"),
        },
        Request::RenameDevice { id, label } => rename_device(state, &id, &label),
        Request::WithdrawDevice { id, password } => withdraw_device(state, &id, &password),
        Request::VerifyBadges {
            holder,
            name,
            badges,
        } => verify_badges(&holder, &name, badges),
        Request::AddBadge { badge } => add_badge(state, badge),
        Request::ServerIdentity {} => server_identity(state),
        Request::SignLogin { challenge } => sign_login(state, &challenge),
        Request::SignDeletion { now } => sign_deletion(state, now),
        Request::PairHostStart { channel } => pair_host_start(state, channel),
        Request::PairHostFinish { hello, offer } => pair_host_finish(state, &hello, &offer),
        Request::PairGuestStart { code } => pair_guest_start(state, &code),
        Request::PairGuestOffer { hello } => pair_guest_offer(state, &hello),
        Request::PairGuestFinish { bundle, password } => {
            pair_guest_finish(state, &bundle, &password)
        }
        Request::ApplyDeviceList { device_list } => apply_device_list(state, device_list),
        Request::PairCancel {} => {
            state.pair_host = None;
            state.pair_guest = None;
            json!({"ok": true})
        }
        Request::NotfallList { password } => notfall_list(state, &password),
        Request::NotfallAdd {
            pin,
            password,
            after,
            chat,
            text,
        } => notfall_add(state, &pin, &password, &after, chat, text),
        Request::NotfallRemove { id, password } => notfall_remove(state, &id, &password),
        Request::NotfallSignLogin { challenge } => match state.notfall.as_ref() {
            Some(s) => s.sign_login(&challenge),
            None => fail("no_session"),
        },
        Request::NotfallSignDeletion { now } => match state.notfall.as_ref() {
            Some(s) => s.sign_deletion(now),
            None => fail("no_session"),
        },
        Request::NotfallFinish {} => match (state.notfall.take(), state.dir.clone()) {
            (Some(session), Some(dir)) => {
                wipe_account(state, &dir, session.after());
                json!({"ok": true})
            }
            _ => fail("no_session"),
        },
        Request::ShareProfile {
            profile,
            only_contacts,
        } => share_profile(state, profile, only_contacts),
        other => chat_op(state, other),
    }
}

/// The chat ops: all need the open chat database.
fn chat_op(state: &mut State, req: Request) -> Value {
    if state.vault.is_none() {
        return fail("locked");
    }
    let Some(m) = state.chats.as_mut() else {
        return fail("chats_unavailable");
    };
    match req {
        Request::ContactCode {} => chats::contact_code(m),
        Request::AddContact { code, name } => chats::add_contact(m, &code, &name),
        Request::Contacts {} => chats::contacts(m),
        Request::ConfirmContact { user } => chats::confirm_contact(m, &user),
        Request::Chats {} => chats::chats(m),
        Request::Messages {
            chat,
            before,
            limit,
        } => chats::messages(m, &chat, before, limit),
        Request::StartChat { user, offers } => chats::start_chat(m, &user, offers),
        Request::AddDevices { chat, offers } => chats::add_devices(m, &chat, offers),
        Request::RemoveDevices { chat, ids } => chats::remove_devices(m, &chat, &ids),
        Request::UpdateKeys { chat } => chats::update_keys(m, &chat),
        Request::WithdrawnMembers { chat } => chats::withdrawn_members(m, &chat),
        Request::PendingCommits {} => chats::pending_commits(m),
        Request::CommitAnswer {
            chat,
            accepted,
            taken,
        } => chats::commit_answer(m, &chat, accepted, taken.as_deref()),
        Request::SendMessage { chat, text } => chats::send_message(m, &chat, &text),
        Request::Receive { body } => chats::receive(m, &body),
        Request::ProfileSent { chat, rev } => chats::profile_sent(m, &chat, rev),
        Request::Profiles {} => chats::profiles(m),
        Request::CreateInvite { minutes, vanishing } => chats::create_invite(m, minutes, vanishing),
        Request::InviteMessage { chat } => chats::invite_message(m, &chat),
        Request::SetChatEnd { chat, minutes } => chats::set_chat_end(m, &chat, minutes),
        Request::Sweep {} => chats::sweep(m),
        Request::AcceptChat { chat, name } => chats::accept_request(m, &chat, &name),
        Request::DeclineChat { chat } => chats::decline_request(m, &chat),
        Request::KeyPackages { n, last_resort } => chats::key_packages(m, n, last_resort),
        Request::ApplyContactList {
            account_key,
            device_list,
        } => chats::apply_contact_list(m, &account_key, device_list),
        _ => fail("bad_request"),
    }
}

/// Open the chat database after the vault opened. The vault stays open
/// even if this fails; the answer says `chats: false`.
fn open_chats(state: &mut State, mut answer: Value) -> Value {
    state.chats = None;
    state.notfall = None;
    if let (Some(dir), Some(vault)) = (state.dir.as_ref(), state.vault.as_ref()) {
        let _ = notfall::refresh(dir, vault, read_vault, write_vault);
    }
    if let (Some(dir), Some(vault)) = (state.dir.as_ref(), state.vault.as_ref()) {
        state.chats = chats::open(dir, vault).ok();
    }
    if answer["ok"] == true {
        answer["chats"] = json!(state.chats.is_some());
    }
    answer
}

/// The account's own device list changed: the chats learn withdrawn
/// devices from it.
fn chats_follow_own_list(state: &mut State) {
    if let (Some(vault), Some(m)) = (state.vault.as_ref(), state.chats.as_mut()) {
        let _ = m.apply_device_list(&vault.device_list().list);
    }
}

fn init(state: &mut State, dir: &str) -> Value {
    let dir = PathBuf::from(dir);
    if dir.as_os_str().is_empty() || !dir.is_absolute() {
        return fail("bad_request");
    }
    if platform::create_private_dir(&dir).is_err() {
        return fail("io");
    }
    if state.dir.as_ref() != Some(&dir) {
        // Take the new folder first; if another window has it, nothing
        // here changes.
        let lock = match lock_folder(&dir) {
            Ok(l) => l,
            Err(code) => return fail(code),
        };
        state.vault = None;
        state.chats = None;
        state.notfall = None;
        state.pair_host = None;
        state.pair_guest = None;
        state.failures = 0;
        state.blocked_until = None;
        state.folder_lock = Some(lock);
    }
    state.dir = Some(dir);
    status(state)
}

/// Name of the lock file in each data folder.
pub const LOCK_FILE: &str = "trair.lock";

fn lock_folder(dir: &Path) -> Result<std::fs::File, &'static str> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK_FILE))
        .map_err(|_| "io")?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err("in_use"),
        Err(_) => Err("io"),
    }
}

/// Longest text for a QR code.
pub const MAX_QR_TEXT: usize = 512;

/// The squares of a QR code: `size` rows of `size` characters, `1` dark,
/// `0` light. Error correction level M.
fn qr(text: &str) -> Value {
    if text.is_empty() || text.len() > MAX_QR_TEXT {
        return fail("bad_request");
    }
    match qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M) {
        Ok(code) => {
            let size = code.width();
            let rows: Vec<String> = code
                .to_colors()
                .chunks(size)
                .map(|row| {
                    row.iter()
                        .map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' })
                        .collect()
                })
                .collect();
            json!({"ok": true, "size": size, "rows": rows})
        }
        Err(_) => fail("bad_request"),
    }
}

fn status(state: &State) -> Value {
    match &state.dir {
        None => fail("not_initialized"),
        Some(dir) => json!({
            "ok": true,
            "exists": dir.join(VAULT_FILE).exists(),
            "unlocked": state.vault.is_some(),
        }),
    }
}

fn create(state: &mut State, password: &str, profile: Value) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    if path.exists() {
        return fail("exists");
    }
    match Vault::create(password, profile) {
        Ok((vault, file)) => {
            if write_vault(&path, &file).is_err() {
                return fail("io");
            }
            let answer = unlocked_answer(&vault);
            state.vault = Some(vault);
            state.failures = 0;
            open_chats(state, answer)
        }
        Err(e) => vault_error(e),
    }
}

fn unlock(state: &mut State, password: &str) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    if let Some(wait) = remaining_wait(state) {
        return json!({"ok": false, "error": "wait", "seconds": wait});
    }
    let file = match read_vault(&path) {
        Ok(Some(file)) => file,
        Ok(None) => return fail("no_vault"),
        Err(code) => return fail(code),
    };
    match Vault::open(&file, password) {
        Ok(vault) => {
            // A vault from Paket 1 got its device key while opening. Store it
            // right away, or the next unlock would make a different device.
            if vault.migrated() {
                let stored = vault
                    .seal()
                    .map_err(|_| ())
                    .and_then(|f| write_vault(&path, &f));
                if stored.is_err() {
                    return fail("io");
                }
            }
            let mut answer = unlocked_answer(&vault);
            answer["upgraded"] = json!(vault.migrated());
            state.vault = Some(vault);
            state.failures = 0;
            state.blocked_until = None;
            open_chats(state, answer)
        }
        Err(VaultError::WrongPassword) => {
            state.failures = state.failures.saturating_add(1);
            let wait = wait_after(state.failures);
            state.blocked_until = (!wait.is_zero()).then(|| Instant::now() + wait);
            // Not the password: maybe an emergency PIN.
            if let Some(dir) = state.dir.clone()
                && let Some(opened) = notfall::try_pin(&dir, password, read_vault)
            {
                if let Some((session, info)) = notfall::prepare(&dir, &opened, read_vault) {
                    state.notfall = Some(session);
                    return notfall::answer(wait.as_secs(), info);
                }
                // Only delete (or sending is not possible): at once.
                wipe_account(state, &dir, opened.action.after);
                return notfall::answer(
                    wait.as_secs(),
                    notfall::info_without_send(opened.action.after),
                );
            }
            json!({"ok": false, "error": "wrong_password", "wait": wait.as_secs()})
        }
        Err(e) => vault_error(e),
    }
}

/// Delete the account on this device after an emergency PIN: vault, chat
/// database and emergency files are overwritten and removed. With
/// [`After::WrongPassword`] a new vault takes the old one's place that no
/// password opens, so the account still shows and says "wrong password".
fn wipe_account(state: &mut State, dir: &Path, after: After) {
    state.vault = None;
    state.chats = None;
    state.notfall = None;
    state.pair_host = None;
    state.pair_guest = None;
    let journal = format!("{}-journal", trair_core::chats::CHATS_FILE);
    for name in [
        VAULT_FILE,
        trair_core::chats::CHATS_FILE,
        journal.as_str(),
        trair_core::notfall::FILE,
        trair_core::notfall::DATA_FILE,
    ] {
        let p = dir.join(name);
        if p.exists() && wipe_file(&p).is_err() {
            let _ = std::fs::remove_file(&p);
        }
    }
    if after == After::WrongPassword {
        let mut r = [0u8; 24];
        if getrandom::fill(&mut r).is_ok() {
            let nobody_knows = Zeroizing::new(hex::encode(r));
            if let Ok((decoy, file)) = Vault::create(&nobody_knows, json!({})) {
                let _ = write_vault(&dir.join(VAULT_FILE), &file);
                let _ = notfall::refresh(dir, &decoy, read_vault, write_vault);
            }
        }
    }
}

/// The PINs are shown only after the password: someone at an open Trair
/// cannot see whether any are set.
fn notfall_list(state: &mut State, password: &str) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    if state.vault.is_none() {
        return fail("locked");
    }
    if let Some(answer) = check_password(state, &path, password) {
        return answer;
    }
    match state.vault.as_ref() {
        Some(v) => notfall::list(v, state.chats.as_ref()),
        None => fail("locked"),
    }
}

fn notfall_add(
    state: &mut State,
    pin: &str,
    password: &str,
    after: &str,
    chat: Option<String>,
    text: Option<String>,
) -> Value {
    let (Some(path), Some(dir)) = (vault_path(state), state.dir.clone()) else {
        return fail("not_initialized");
    };
    if state.vault.is_none() {
        return fail("locked");
    }
    // Setting a PIN needs the password, like withdrawing a device.
    if let Some(answer) = check_password(state, &path, password) {
        return answer;
    }
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    match notfall::add(
        &dir,
        vault,
        state.chats.as_ref(),
        password,
        pin,
        after,
        chat,
        text,
        read_vault,
    ) {
        Ok(slots) => store_pins(state, &path, &dir, slots),
        Err(code) => fail(code),
    }
}

fn notfall_remove(state: &mut State, id: &str, password: &str) -> Value {
    let (Some(path), Some(dir)) = (vault_path(state), state.dir.clone()) else {
        return fail("not_initialized");
    };
    if state.vault.is_none() {
        return fail("locked");
    }
    if let Some(answer) = check_password(state, &path, password) {
        return answer;
    }
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    match notfall::remove(&dir, vault, id, read_vault) {
        Ok(slots) => store_pins(state, &path, &dir, slots),
        Err(code) => fail(code),
    }
}

/// The vault first (it lists the PINs), then the slots and the data file.
fn store_pins(
    state: &mut State,
    path: &Path,
    dir: &Path,
    slots: trair_core::notfall::SlotFile,
) -> Value {
    let Some(vault) = state.vault.as_ref() else {
        return fail("locked");
    };
    let answer = seal_and_write(vault, path);
    if answer["ok"] != true {
        return answer;
    }
    let Ok(bytes) = slots.to_bytes() else {
        return fail("internal");
    };
    if write_vault(&dir.join(trair_core::notfall::FILE), &bytes).is_err() {
        return fail("io");
    }
    notfall::list(vault, state.chats.as_ref())
}

/// The app's profile plus this account's badges from the vault (only
/// those valid now; the receiver checks them again).
fn share_profile(state: &mut State, mut profile: Value, only_contacts: bool) -> Value {
    let Some(vault) = state.vault.as_ref() else {
        return fail("locked");
    };
    let Some(m) = state.chats.as_mut() else {
        return fail("chats_unavailable");
    };
    if !profile.is_object() {
        return fail("bad_request");
    }
    let now = unix_now();
    let own: Vec<Value> = vault
        .badges()
        .iter()
        .filter(|b| badges::verify(b, vault.account().public_key(), now, badges::TEAM_KEYS).is_ok())
        .filter_map(|b| serde_json::to_value(b).ok())
        .collect();
    profile["badges"] = Value::Array(own);
    chats::share_profile(m, &profile, only_contacts)
}

fn save_profile(state: &mut State, profile: Value) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    let strict = chats::strict_of(&profile);
    if let Err(e) = vault.set_profile(profile) {
        return vault_error(e);
    }
    let answer = seal_and_write(vault, &path);
    if let Some(m) = state.chats.as_mut() {
        let _ = m.set_strict(strict);
    }
    answer
}

fn change_password(state: &mut State, old: &str, new: &str) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    if state.vault.is_none() {
        return fail("locked");
    }
    // Someone at an unlocked PC must still know the password to change it.
    if let Some(answer) = check_password(state, &path, old) {
        return answer;
    }
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    // A password that is also a PIN: the PIN would never fire again.
    if let Some(dir) = state.dir.as_ref()
        && notfall::try_pin(dir, new, read_vault).is_some()
    {
        return fail("pin_is_password");
    }
    if let Err(e) = vault.change_password(new) {
        return vault_error(e);
    }
    seal_and_write(vault, &path)
}

fn rename_device(state: &mut State, id: &str, label: &str) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    if let Err(e) = vault.set_label(id, label) {
        return vault_error(e);
    }
    with_devices(seal_and_write(vault, &path), vault)
}

fn withdraw_device(state: &mut State, id: &str, password: &str) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    if state.vault.is_none() {
        return fail("locked");
    }
    // Withdrawing cannot be undone, so it needs the password again.
    if let Some(answer) = check_password(state, &path, password) {
        return answer;
    }
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    if let Err(e) = vault.withdraw_device(id) {
        return vault_error(e);
    }
    let answer = with_devices(seal_and_write(vault, &path), vault);
    chats_follow_own_list(state);
    answer
}

// ---------------------------------------------------------------- server (M3)

/// What the server needs to register this account: all public.
fn server_identity(state: &State) -> Value {
    let Some(vault) = state.vault.as_ref() else {
        return fail("locked");
    };
    json!({
        "ok": true,
        "user": vault.account().user(),
        "device": vault.device().id,
        "account_key": hex::encode(vault.account().public_key()),
        "device_list": vault.device_list(),
    })
}

/// Sign the server's login challenge with this device's key.
fn sign_login(state: &State, challenge: &str) -> Value {
    let Some(vault) = state.vault.as_ref() else {
        return fail("locked");
    };
    let Ok(challenge) = hex::decode(challenge) else {
        return fail("bad_request");
    };
    match proofs::sign_login(vault.device(), vault.account().user(), &challenge) {
        Ok(sig) => json!({"ok": true, "signature": hex::encode(sig)}),
        Err(_) => fail("bad_request"),
    }
}

/// The account key agrees to delete the account on the server.
fn sign_deletion(state: &State, now: u64) -> Value {
    let Some(vault) = state.vault.as_ref() else {
        return fail("locked");
    };
    match proofs::sign_account_deletion(vault.account(), now) {
        Ok(sig) => json!({"ok": true, "signature": hex::encode(sig)}),
        Err(_) => fail("internal"),
    }
}

fn unbase64(text: &str) -> Option<Vec<u8>> {
    B64.decode(text).ok().filter(|b| !b.is_empty())
}

/// A device list from the server, made by another device of the account
/// (after pairing or withdrawing). Taken only if the account signed it,
/// it is newer and this device is still on it; older or equal lists are
/// answered with `changed: false`.
fn apply_device_list(state: &mut State, list: Value) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    let Ok(list) = serde_json::from_value::<SignedDeviceList>(list) else {
        return fail("bad_request");
    };
    if list.list.seq <= vault.device_list().list.seq {
        return json!({"ok": true, "changed": false, "devices": devices(vault)});
    }
    // Signed by the account, but without this device: another device of
    // the account withdrew this one. The app says so plainly.
    match list.verify(vault.account().public_key(), None) {
        Ok(l) if !l.contains(&vault.device().id) => return fail("withdrawn"),
        Ok(_) => {}
        Err(_) => return fail("refused"),
    }
    if let Err(e) = vault.set_device_list(list) {
        return vault_error(e);
    }
    let mut answer = seal_and_write(vault, &path);
    if answer["ok"] == true {
        answer["changed"] = json!(true);
        answer["devices"] = devices(vault);
    }
    chats_follow_own_list(state);
    answer
}

/// The unlocked device starts pairing on a channel from the server.
fn pair_host_start(state: &mut State, channel: u16) -> Value {
    if state.vault.is_none() {
        return fail("locked");
    }
    match PairingHost::start(channel, Instant::now()) {
        Ok((host, code, hello)) => {
            state.pair_host = Some(host);
            json!({"ok": true, "code": code.text(), "hello": B64.encode(hello)})
        }
        Err(_) => fail("internal"),
    }
}

/// The guest's answer arrived: add the new device, store the vault, and
/// return the bundle for the relay and the new list for the server. The
/// pairing is used up either way.
fn pair_host_finish(state: &mut State, hello: &str, offer: &str) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    let Some(mut host) = state.pair_host.take() else {
        return fail("no_pairing");
    };
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    let (Some(hello), Some(offer)) = (unbase64(hello), unbase64(offer)) else {
        return fail("bad_request");
    };
    let bundle = match host.finish(&hello, &offer, vault, Instant::now()) {
        Ok(b) => b,
        // Wrong code, changed message or expired: nothing was added.
        Err(_) => return fail("pairing_failed"),
    };
    let mut answer = seal_and_write(vault, &path);
    if answer["ok"] == true {
        answer["bundle"] = json!(B64.encode(bundle));
        answer["device_list"] = json!(vault.device_list());
        answer["devices"] = devices(vault);
    }
    answer
}

/// A new device (no vault yet) starts pairing with the typed code.
fn pair_guest_start(state: &mut State, code: &str) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    if path.exists() || state.vault.is_some() {
        return fail("exists");
    }
    let Ok(code) = Code::parse(code) else {
        return fail("bad_code");
    };
    match PairingGuest::start(&code) {
        Ok((guest, hello)) => {
            state.pair_guest = Some(guest);
            json!({"ok": true, "channel": code.channel(), "hello": B64.encode(hello)})
        }
        Err(_) => fail("internal"),
    }
}

/// The host's hello arrived: answer with this device's encrypted offer.
fn pair_guest_offer(state: &mut State, hello: &str) -> Value {
    let Some(guest) = state.pair_guest.as_mut() else {
        return fail("no_pairing");
    };
    let Some(hello) = unbase64(hello) else {
        return fail("bad_request");
    };
    match guest.offer(&hello) {
        Ok(offer) => json!({"ok": true, "offer": B64.encode(offer)}),
        Err(_) => {
            state.pair_guest = None;
            fail("pairing_failed")
        }
    }
}

/// The bundle arrived: check it, make this device's vault with its own
/// password and unlock it.
fn pair_guest_finish(state: &mut State, bundle: &str, password: &str) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    if path.exists() {
        return fail("exists");
    }
    let Some(bundle) = unbase64(bundle) else {
        return fail("bad_request");
    };
    // Check the password before the pairing is used up.
    if let Err(e) = trair_core::vault::check_password(password) {
        return vault_error(e);
    }
    let Some(guest) = state.pair_guest.take() else {
        return fail("no_pairing");
    };
    let joined = match guest.finish(&bundle) {
        Ok(j) => j,
        Err(_) => return fail("pairing_failed"),
    };
    match Vault::create_joined(password, joined) {
        Ok((vault, file)) => {
            if write_vault(&path, &file).is_err() {
                return fail("io");
            }
            let answer = unlocked_answer(&vault);
            state.vault = Some(vault);
            state.failures = 0;
            open_chats(state, answer)
        }
        Err(e) => vault_error(e),
    }
}

fn add_badge(state: &mut State, badge: Value) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    let Ok(badge) = serde_json::from_value::<Badge>(badge) else {
        return fail("bad_request");
    };
    if let Err(e) = vault.add_badge(badge, unix_now(), badges::TEAM_KEYS) {
        return vault_error(e);
    }
    let mut answer = seal_and_write(vault, &path);
    if answer["ok"] == true {
        answer["badges"] = own_badges(vault);
    }
    answer
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn own_badges(vault: &Vault) -> Value {
    json!(vault.verified_badges(unix_now(), badges::TEAM_KEYS))
}

fn verify_badges(holder: &str, name: &str, badges: Vec<Value>) -> Value {
    let Ok(holder) = hex::decode(holder) else {
        return fail("bad_request");
    };
    if holder.len() != 32 {
        return fail("bad_request");
    }
    // Each badge on its own: one broken badge does not hide the others.
    let total = badges.len();
    let parsed: Vec<Badge> = badges
        .into_iter()
        .filter_map(|b| serde_json::from_value(b).ok())
        .collect();
    let unreadable = total - parsed.len();
    let (shown, refused) = badges::verify_all(&parsed, &holder, unix_now(), badges::TEAM_KEYS);
    json!({
        "ok": true,
        "name_warning": badges::name_needs_warning(name, &shown),
        "badges": shown,
        "refused": refused + unreadable,
    })
}

fn delete_account(state: &mut State, password: &str) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    if let Some(answer) = check_password(state, &path, password) {
        return answer;
    }
    state.vault = None;
    state.chats = None;
    if wipe_file(&path).is_err() {
        return fail("io");
    }
    // The chat database is useless without the vault's key; it goes too,
    // and so do the emergency files.
    if let Some(dir) = state.dir.as_ref() {
        chats::remove_files(dir);
        for name in [trair_core::notfall::FILE, trair_core::notfall::DATA_FILE] {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
    json!({"ok": true})
}

/// `None` if `password` opens the stored vault, else the answer to give.
/// Counts towards the wait like a wrong unlock.
fn check_password(state: &mut State, path: &Path, password: &str) -> Option<Value> {
    if let Some(wait) = remaining_wait(state) {
        return Some(json!({"ok": false, "error": "wait", "seconds": wait}));
    }
    let file = match read_vault(path) {
        Ok(Some(file)) => file,
        Ok(None) => return Some(fail("no_vault")),
        Err(code) => return Some(fail(code)),
    };
    match Vault::open(&file, password) {
        Ok(_) => {
            state.failures = 0;
            None
        }
        Err(VaultError::WrongPassword) => {
            state.failures = state.failures.saturating_add(1);
            let wait = wait_after(state.failures);
            state.blocked_until = (!wait.is_zero()).then(|| Instant::now() + wait);
            Some(json!({"ok": false, "error": "wrong_password", "wait": wait.as_secs()}))
        }
        Err(e) => Some(vault_error(e)),
    }
}

/// No wait for the first [`FREE_ATTEMPTS`] wrong passwords, then 5 s,
/// doubling each time, at most [`MAX_WAIT`]. This slows down guessing at
/// the PC itself; a copied file is protected by Argon2id instead.
pub fn wait_after(failures: u32) -> Duration {
    if failures < FREE_ATTEMPTS {
        return Duration::ZERO;
    }
    let doublings = (failures - FREE_ATTEMPTS).min(16);
    Duration::from_secs(5u64 << doublings).min(MAX_WAIT)
}

fn remaining_wait(state: &State) -> Option<u64> {
    let until = state.blocked_until?;
    let now = Instant::now();
    (until > now).then(|| (until - now).as_secs().max(1))
}

fn unlocked_answer(vault: &Vault) -> Value {
    json!({
        "ok": true,
        "fingerprint": vault.account().fingerprint().grouped(),
        "profile": vault.profile(),
        "device": vault.device().id,
        "devices": devices(vault),
        "account_key": hex::encode(vault.account().public_key()),
        "badges": own_badges(vault),
    })
}

/// The account's devices for the app. Withdrawn devices are left out.
fn devices(vault: &Vault) -> Value {
    let own = &vault.device().id;
    vault
        .device_list()
        .list
        .devices
        .iter()
        .map(|d| {
            json!({
                "id": d.id,
                "label": vault.labels().get(&d.id),
                "fingerprint": d.fingerprint().grouped(),
                "this": &d.id == own,
            })
        })
        .collect()
}

fn with_devices(mut answer: Value, vault: &Vault) -> Value {
    if answer["ok"] == true {
        answer["devices"] = devices(vault);
    }
    answer
}

fn seal_and_write(vault: &Vault, path: &Path) -> Value {
    match vault.seal() {
        Ok(file) => match write_vault(path, &file) {
            Ok(()) => {
                // The emergency data follows the vault (keys, device list,
                // server address).
                if let Some(dir) = path.parent() {
                    let _ = notfall::refresh(dir, vault, read_vault, write_vault);
                }
                json!({"ok": true})
            }
            Err(()) => fail("io"),
        },
        Err(e) => vault_error(e),
    }
}

fn vault_path(state: &State) -> Option<PathBuf> {
    state.dir.as_ref().map(|d| d.join(VAULT_FILE))
}

fn read_vault(path: &Path) -> Result<Option<Vec<u8>>, &'static str> {
    match std::fs::read(path) {
        Ok(bytes) => platform::unwrap(&bytes).map(Some).map_err(|_| "damaged"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("io"),
    }
}

/// Write to a temporary file, flush it to disk, then rename it over the
/// old one, so a crash never leaves a half written vault.
fn write_vault(path: &Path, inner: &[u8]) -> Result<(), ()> {
    use std::io::Write;
    let bytes = platform::wrap(inner).map_err(|_| ())?;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp).map_err(|_| ())?;
    f.write_all(&bytes).map_err(|_| ())?;
    f.sync_all().map_err(|_| ())?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|_| ())
}

/// Overwrite with random bytes, then delete. On SSDs the old blocks may
/// survive, but they are encrypted with a key nobody has any more.
fn wipe_file(path: &Path) -> Result<(), ()> {
    use std::io::Write;
    let len = std::fs::metadata(path).map_err(|_| ())?.len() as usize;
    let mut noise = vec![0u8; len];
    getrandom::fill(&mut noise).map_err(|_| ())?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| ())?;
    f.write_all(&noise).map_err(|_| ())?;
    f.sync_all().map_err(|_| ())?;
    drop(f);
    std::fs::remove_file(path).map_err(|_| ())
}

fn vault_error(e: VaultError) -> Value {
    fail(match e {
        VaultError::WeakPassword => "weak_password", // pragma: allowlist secret (Fehlercode)
        VaultError::WrongPassword => "wrong_password", // pragma: allowlist secret (Fehlercode)
        VaultError::Damaged(_) => "damaged",
        VaultError::Refused(_) => "refused",
        VaultError::TooLarge => "too_large",
        VaultError::Internal(_) => "internal",
    })
}

fn fail(code: &str) -> Value {
    json!({"ok": false, "error": code})
}
