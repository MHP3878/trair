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
//! | `rename_device`   | `id`, `label`          | `devices`                        |
//! | `withdraw_device` | `id`, `password`       | `devices`                        |
//! | `verify_badges`   | `holder`, `name`, `badges` | `badges`, `refused`, `name_warning` |
//!
//! `device` is this device's id; `devices` lists the account's devices as
//! `{id, label, fingerprint, this}` (label may be null). `upgraded` is true
//! once, when a vault from Paket 1 got its device key and was stored again.
//!
//! `verify_badges` checks the badges of a profile (`holder`: the account
//! key in hex, `name`: the display name, `badges`: signed badges as JSON)
//! and returns only the valid ones as `{kind, community, community_id,
//! community_warning, expires}`. It needs no unlocked vault.
//!
//! Error codes: `bad_request`, `not_initialized`, `exists`, `no_vault`,
//! `locked`, `wrong_password` (with `wait` in seconds), `wait` (with
//! `seconds`), `weak_password`, `damaged`, `refused`, `too_large`, `io`,
//! `unsupported`, `internal`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};
use trair_core::badges::{self, Badge};
use trair_core::vault::{Vault, VaultError};
use zeroize::Zeroizing;

use crate::platform;

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
}

#[derive(Default)]
struct State {
    dir: Option<PathBuf>,
    vault: Option<Vault>,
    failures: u32,
    blocked_until: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    dir: None,
    vault: None,
    failures: 0,
    blocked_until: None,
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
            json!({"ok": true})
        }
        Request::SaveProfile { profile } => save_profile(state, profile),
        Request::ChangePassword { old, new } => change_password(state, &old, &new),
        Request::DeleteAccount { password } => delete_account(state, &password),
        Request::ProtectWindow { on } => match platform::protect_windows(on) {
            Some(n) => json!({"ok": true, "windows": n}),
            None => fail("unsupported"),
        },
        Request::RenameDevice { id, label } => rename_device(state, &id, &label),
        Request::WithdrawDevice { id, password } => withdraw_device(state, &id, &password),
        Request::VerifyBadges {
            holder,
            name,
            badges,
        } => verify_badges(&holder, &name, badges),
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
        state.vault = None;
        state.failures = 0;
        state.blocked_until = None;
    }
    state.dir = Some(dir);
    status(state)
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
            answer
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
            answer
        }
        Err(VaultError::WrongPassword) => {
            state.failures = state.failures.saturating_add(1);
            let wait = wait_after(state.failures);
            state.blocked_until = (!wait.is_zero()).then(|| Instant::now() + wait);
            json!({"ok": false, "error": "wrong_password", "wait": wait.as_secs()})
        }
        Err(e) => vault_error(e),
    }
}

fn save_profile(state: &mut State, profile: Value) -> Value {
    let Some(path) = vault_path(state) else {
        return fail("not_initialized");
    };
    let Some(vault) = state.vault.as_mut() else {
        return fail("locked");
    };
    if let Err(e) = vault.set_profile(profile) {
        return vault_error(e);
    }
    seal_and_write(vault, &path)
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
    with_devices(seal_and_write(vault, &path), vault)
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
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (shown, refused) = badges::verify_all(&parsed, &holder, now, badges::TEAM_KEYS);
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
    if wipe_file(&path).is_err() {
        return fail("io");
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
            Ok(()) => json!({"ok": true}),
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
