// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Tests for the door: the JSON protocol and the four C functions. The
//! door has one global state, so the tests take turns.

#![allow(unsafe_code)]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};
use trair_ffi::abi::{trair_abi_version, trair_alloc, trair_call, trair_free};
use trair_ffi::api::{VAULT_FILE, handle, wait_after};

static TURN: Mutex<()> = Mutex::new(());
const PW: &str = "mein langes passwort";

fn call(req: Value) -> Value {
    serde_json::from_slice(&handle(&serde_json::to_vec(&req).unwrap())).unwrap()
}

fn fresh_dir() -> PathBuf {
    let mut r = [0u8; 8];
    getrandom::fill(&mut r).unwrap();
    let hex: String = r.iter().map(|b| format!("{b:02x}")).collect();
    std::env::temp_dir().join(format!("trair-test-{hex}"))
}

fn error(v: &Value) -> &str {
    v["error"].as_str().unwrap_or("")
}

#[test]
fn d01_full_life_of_an_account() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    assert_eq!(
        error(&call(json!({"op": "init", "dir": "relativ/pfad"}))),
        "bad_request"
    );
    let s = call(json!({"op": "init", "dir": dir}));
    assert_eq!(s, json!({"ok": true, "exists": false, "unlocked": false}));
    assert_eq!(
        error(&call(json!({"op": "unlock", "password": PW}))),
        "no_vault"
    );

    let weak = call(json!({"op": "create", "password": "kurz", "profile": {}}));
    assert_eq!(error(&weak), "weak_password");
    let created = call(json!({"op": "create", "password": PW, "profile": {"name": "Julian"}}));
    assert_eq!(created["ok"], true, "{created}");
    let fingerprint = created["fingerprint"].as_str().unwrap().to_string();
    assert_eq!(created["profile"], json!({"name": "Julian"}));
    assert_eq!(
        call(json!({"op": "status"})),
        json!({"ok": true, "exists": true, "unlocked": true})
    );
    assert_eq!(
        error(&call(
            json!({"op": "create", "password": PW, "profile": {}})
        )),
        "exists"
    );

    // Nothing readable on disk.
    let file = std::fs::read(dir.join(VAULT_FILE)).unwrap();
    assert!(!String::from_utf8_lossy(&file).contains("Julian"));

    // Profile is stored and comes back after locking.
    assert_eq!(
        call(json!({"op": "save_profile", "profile": {"name": "Jule"}}))["ok"],
        true
    );
    assert_eq!(call(json!({"op": "lock"}))["ok"], true);
    assert_eq!(
        error(&call(json!({"op": "save_profile", "profile": {}}))),
        "locked"
    );
    let again = call(json!({"op": "unlock", "password": PW}));
    assert_eq!(again["profile"], json!({"name": "Jule"}));
    assert_eq!(again["fingerprint"].as_str().unwrap(), fingerprint);

    // Changing the password needs the old one.
    let bad = call(
        json!({"op": "change_password", "old": "nicht das passwort", "new": "ein neues passwort"}),
    );
    assert_eq!(error(&bad), "wrong_password");
    let changed = call(json!({"op": "change_password", "old": PW, "new": "ein neues passwort"}));
    assert_eq!(changed["ok"], true, "{changed}");
    call(json!({"op": "lock"}));
    assert_eq!(
        error(&call(json!({"op": "unlock", "password": PW}))),
        "wrong_password"
    );
    let new_pw = call(json!({"op": "unlock", "password": "ein neues passwort"}));
    assert_eq!(
        new_pw["fingerprint"].as_str().unwrap(),
        fingerprint,
        "same account"
    );

    // Deleting needs the password, then everything is gone.
    let refused = call(json!({"op": "delete_account", "password": PW}));
    assert_eq!(error(&refused), "wrong_password");
    assert!(dir.join(VAULT_FILE).exists());
    let deleted = call(json!({"op": "delete_account", "password": "ein neues passwort"}));
    assert_eq!(deleted["ok"], true, "{deleted}");
    assert!(!dir.join(VAULT_FILE).exists());
    assert_eq!(
        call(json!({"op": "status"})),
        json!({"ok": true, "exists": false, "unlocked": false})
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d02_guessing_gets_slower() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(wait_after(1), Duration::ZERO);
    assert_eq!(wait_after(2), Duration::ZERO);
    assert_eq!(wait_after(3), Duration::from_secs(5));
    assert_eq!(wait_after(4), Duration::from_secs(10));
    assert_eq!(wait_after(10), Duration::from_secs(300));
    assert_eq!(wait_after(u32::MAX), Duration::from_secs(300));

    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    assert_eq!(
        call(json!({"op": "create", "password": PW, "profile": {}}))["ok"],
        true
    );
    call(json!({"op": "lock"}));
    for expected_wait in [0, 0, 5] {
        let r = call(json!({"op": "unlock", "password": "falsch falsch falsch"}));
        assert_eq!(error(&r), "wrong_password");
        assert_eq!(r["wait"], expected_wait);
    }
    // During the wait even the right password is not tried.
    let r = call(json!({"op": "unlock", "password": PW}));
    assert_eq!(error(&r), "wait");
    assert!(r["seconds"].as_u64().unwrap() >= 1);
    std::thread::sleep(Duration::from_millis(5100));
    assert_eq!(call(json!({"op": "unlock", "password": PW}))["ok"], true);
    // A success resets the counter.
    call(json!({"op": "lock"}));
    assert_eq!(
        call(json!({"op": "unlock", "password": "falsch falsch falsch"}))["wait"], // pragma: allowlist secret (Testpasswort)
        0
    );
    assert_eq!(call(json!({"op": "unlock", "password": PW}))["ok"], true);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d03_changed_file_does_not_open() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {}}));
    call(json!({"op": "lock"}));
    let path = dir.join(VAULT_FILE);
    let mut file = std::fs::read(&path).unwrap();
    let last = file.len() - 1;
    file[last] ^= 1;
    std::fs::write(&path, &file).unwrap();
    let r = call(json!({"op": "unlock", "password": PW}));
    assert_eq!(r["ok"], false);
    assert!(["wrong_password", "damaged"].contains(&error(&r)), "{r}");
    std::fs::write(&path, b"kein tresor").unwrap();
    assert_eq!(
        error(&call(json!({"op": "unlock", "password": PW}))),
        "damaged"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d04_bad_requests_are_refused() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    for raw in [
        &b"kein json"[..],
        br#"{"op":"gibtsnicht"}"#,
        br#"{"op":"lock","extra":1}"#,
        br#"{"op":"unlock"}"#,
        br#"{"op":"unlock","password":5}"#,
        b"",
    ] {
        let r: Value = serde_json::from_slice(&handle(raw)).unwrap();
        assert_eq!(error(&r), "bad_request", "{}", String::from_utf8_lossy(raw));
    }
    let huge = vec![b' '; 2 * 1024 * 1024];
    let r: Value = serde_json::from_slice(&handle(&huge)).unwrap();
    assert_eq!(error(&r), "bad_request");
}

#[test]
fn d05_c_functions_round_trip() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(trair_abi_version(), 1);
    assert!(trair_alloc(0).is_null());
    assert!(trair_alloc(2 * 1024 * 1024).is_null());
    let req = br#"{"op":"status"}"#;
    let buf = trair_alloc(req.len());
    assert!(!buf.is_null());
    // SAFETY: `buf` has `req.len()` bytes from trair_alloc; the answer
    // buffer is read within its reported length and freed once.
    unsafe {
        std::ptr::copy_nonoverlapping(req.as_ptr(), buf, req.len());
        let out = trair_call(buf, req.len());
        trair_free(buf, req.len());
        assert!(!out.is_null());
        let len =
            u32::from_le_bytes(std::slice::from_raw_parts(out, 4).try_into().unwrap()) as usize;
        let answer: Value =
            serde_json::from_slice(std::slice::from_raw_parts(out.add(4), len)).unwrap();
        assert!(answer.get("ok").is_some());
        trair_free(out, 4 + len);
        assert!(trair_call(std::ptr::null(), 0).is_null());
        trair_free(std::ptr::null_mut(), 0);
    }
}

#[cfg(not(windows))]
#[test]
fn d06_screenshot_block_reports_unsupported_off_windows() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(
        error(&call(json!({"op": "protect_window", "on": true}))),
        "unsupported"
    );
}

#[cfg(windows)]
#[test]
fn d06_vault_is_bound_to_windows_user() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {}}));
    let file = std::fs::read(dir.join(VAULT_FILE)).unwrap();
    assert!(file.starts_with(b"TRAIRW1\0"), "DPAPI layer missing");
    // The bare core format is refused on Windows.
    let inner_magic = b"TRAIRV1\0";
    assert!(
        !file.windows(8).any(|w| w == inner_magic),
        "inner vault visible"
    );
    call(json!({"op": "lock"}));
    assert_eq!(call(json!({"op": "unlock", "password": PW}))["ok"], true);
    let r = call(json!({"op": "protect_window", "on": true}));
    assert_eq!(r["ok"], true, "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}
