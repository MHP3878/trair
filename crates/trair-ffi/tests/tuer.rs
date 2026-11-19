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

fn device_ids(answer: &Value) -> Vec<String> {
    answer["devices"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|d| d["id"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn d07_devices_can_be_named_but_not_withdraw_themselves() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    let created = call(json!({"op": "create", "password": PW, "profile": {}}));
    let me = created["device"].as_str().unwrap().to_string();
    assert_eq!(device_ids(&created), vec![me.clone()]);
    let first = &created["devices"][0];
    assert_eq!(first["this"], true);
    assert_eq!(first["label"], "Dieser PC");
    assert_eq!(first["fingerprint"].as_str().unwrap().len(), 39);

    let renamed = call(json!({"op": "rename_device", "id": me, "label": "Julians Laptop"}));
    assert_eq!(
        renamed["devices"][0]["label"], "Julians Laptop",
        "{renamed}"
    );
    for bad in ["", " vorne", "a\u{7}b"] {
        let r = call(json!({"op": "rename_device", "id": me, "label": bad}));
        assert_eq!(error(&r), "refused", "{bad:?}");
    }
    let r = call(json!({"op": "rename_device", "id": "d-00000000", "label": "Fremd"}));
    assert_eq!(error(&r), "refused");
    // The name is stored in the vault, not only in memory.
    call(json!({"op": "lock"}));
    let again = call(json!({"op": "unlock", "password": PW}));
    assert_eq!(again["devices"][0]["label"], "Julians Laptop");
    assert_eq!(again["device"], me.as_str());
    assert_eq!(again["upgraded"], false);

    // Withdrawing needs the password and never hits this device.
    let r = call(json!({"op": "withdraw_device", "id": me, "password": "nicht das passwort"}));
    assert_eq!(error(&r), "wrong_password");
    let r = call(json!({"op": "withdraw_device", "id": me, "password": PW}));
    assert_eq!(error(&r), "refused");
    let r = call(json!({"op": "withdraw_device", "id": "d-00000000", "password": PW}));
    assert_eq!(error(&r), "refused");
    call(json!({"op": "lock"}));
    let r = call(json!({"op": "rename_device", "id": me, "label": "Neu"}));
    assert_eq!(error(&r), "locked");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The bare core format is only accepted where there is no DPAPI layer.
#[cfg(not(windows))]
#[test]
fn d08_vault_from_paket_1_is_upgraded_once() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../trair-core/tests/fixtures");
    let old = std::fs::read(fixtures.join("vault-v1-real.bin")).unwrap();
    let password = std::fs::read_to_string(fixtures.join("vault-v1.password")).unwrap();
    let expected = std::fs::read_to_string(fixtures.join("vault-v1-real.fingerprint")).unwrap();
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    std::fs::write(dir.join(VAULT_FILE), &old).unwrap();

    let first = call(json!({"op": "unlock", "password": password.trim_end()}));
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["upgraded"], true);
    assert_eq!(
        first["fingerprint"].as_str().unwrap().replace(' ', ""),
        expected.trim(),
        "same account as in Paket 1"
    );
    assert_eq!(first["profile"]["name"], "Julian");
    let stored = std::fs::read(dir.join(VAULT_FILE)).unwrap();
    assert!(stored != old, "upgraded vault was stored");

    call(json!({"op": "lock"}));
    let second = call(json!({"op": "unlock", "password": password.trim_end()}));
    assert_eq!(second["upgraded"], false, "only once");
    assert_eq!(
        second["device"], first["device"],
        "same device after the upgrade"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A second device (as pairing will add it, M3) is withdrawn through the
/// door and stays withdrawn.
#[cfg(not(windows))]
#[test]
fn d09_second_device_is_withdrawn_with_password() {
    use trair_core::DeviceIdentity;
    use trair_core::vault::Vault;
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    // A random password: nothing hard-coded flows into the core.
    let pw = random_text();
    let (mut vault, _) = Vault::create(&pw, json!({})).unwrap();
    let other = DeviceIdentity::generate().unwrap();
    let list = vault
        .device_list()
        .with_device(vault.account(), other.entry())
        .unwrap();
    vault.set_device_list(list).unwrap();
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    std::fs::write(dir.join(VAULT_FILE), vault.seal().unwrap()).unwrap();

    let opened = call(json!({"op": "unlock", "password": pw}));
    assert_eq!(device_ids(&opened).len(), 2, "{opened}");
    let unnamed = opened["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["this"] == false)
        .unwrap();
    assert_eq!(unnamed["label"], Value::Null);
    let r = call(json!({"op": "withdraw_device", "id": other.id, "password": pw}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(device_ids(&r), vec![vault.device().id.clone()]);
    call(json!({"op": "lock"}));
    let again = call(json!({"op": "unlock", "password": pw}));
    assert_eq!(device_ids(&again), vec![vault.device().id.clone()]);
    let r = call(json!({"op": "withdraw_device", "id": other.id, "password": pw}));
    assert_eq!(error(&r), "refused", "already withdrawn");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d10_only_real_badges_come_out_of_the_door() {
    use trair_core::Account;
    use trair_core::badges::{Badge, BadgeKind, Community};
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let founder = Account::generate("gruenderin").unwrap();
    let stranger = Account::generate("fremder").unwrap();
    let community = Community::create("Trair Beta", &founder).unwrap();
    let issue = |issuer: &Account, kind, holder: &Account| {
        let b = Badge::issue(
            issuer,
            kind,
            holder.public_key(),
            Some(community.clone()),
            now,
            3600,
            None,
        )
        .unwrap();
        serde_json::to_value(b).unwrap()
    };
    let badges = vec![
        issue(&founder, BadgeKind::Founder, &founder),
        issue(&founder, BadgeKind::Admin, &founder),
        issue(&stranger, BadgeKind::Admin, &founder),
        issue(&founder, BadgeKind::Mod, &stranger),
        json!({"claim": "kaputt"}),
    ];
    let holder = hex::encode(founder.public_key());
    let r =
        call(json!({"op": "verify_badges", "holder": holder, "name": "Lena", "badges": badges}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["badges"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["badges"][0]["kind"], "founder");
    assert_eq!(r["badges"][0]["community"], "Trair Beta");
    assert_eq!(
        r["badges"][0]["community_warning"], true,
        "name looks official"
    );
    assert_eq!(
        r["refused"], 3,
        "self-made admin, badge of someone else, unreadable"
    );
    assert_eq!(r["name_warning"], false);
    let r = call(
        json!({"op": "verify_badges", "holder": holder, "name": "Trair Support", "badges": []}),
    );
    assert_eq!(r["name_warning"], true);
    for holder in ["", "zz", "00"] {
        let r = call(json!({"op": "verify_badges", "holder": holder, "name": "x", "badges": []}));
        assert_eq!(error(&r), "bad_request", "{holder:?}");
    }
}

#[test]
fn d11_own_badges_are_checked_and_stored() {
    use trair_core::Account;
    use trair_core::badges::{Badge, BadgeKind, Community};
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    let created = call(json!({"op": "create", "password": PW, "profile": {}}));
    let key = created["account_key"].as_str().unwrap().to_string();
    assert_eq!(key.len(), 64);
    assert_eq!(created["badges"], json!([]));
    let holder = hex::decode(&key).unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let founder = Account::generate("gruenderin").unwrap();
    let community = Community::create("Trair Beta", &founder).unwrap();
    let admin = Badge::issue(
        &founder,
        BadgeKind::Admin,
        &holder,
        Some(community.clone()),
        now,
        3600,
        None,
    )
    .unwrap();
    let r = call(json!({"op": "add_badge", "badge": admin}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["badges"][0]["kind"], "admin");

    // Someone else's badge, a team badge without a team key, garbage.
    let other = Account::generate("fremd").unwrap();
    let foreign = Badge::issue(
        &founder,
        BadgeKind::Admin,
        other.public_key(),
        Some(community),
        now,
        3600,
        None,
    )
    .unwrap();
    assert_eq!(
        error(&call(json!({"op": "add_badge", "badge": foreign}))),
        "refused"
    );
    let team = Account::generate("trair-team").unwrap();
    let fake_team = Badge::issue(&team, BadgeKind::Team, &holder, None, now, 3600, None).unwrap();
    assert_eq!(
        error(&call(json!({"op": "add_badge", "badge": fake_team}))),
        "refused"
    );
    assert_eq!(
        error(&call(json!({"op": "add_badge", "badge": {"x": 1}}))),
        "bad_request"
    );

    // Stored in the vault: still there after locking.
    call(json!({"op": "lock"}));
    let again = call(json!({"op": "unlock", "password": PW}));
    assert_eq!(again["badges"].as_array().unwrap().len(), 1, "{again}");
    assert_eq!(again["account_key"], key.as_str());
    call(json!({"op": "lock"}));
    assert_eq!(
        error(&call(json!({"op": "add_badge", "badge": {"x": 1}}))),
        "locked"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unb64(v: &Value) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(v.as_str().unwrap())
        .unwrap()
}

#[test]
fn d12_door_as_the_unlocked_pc_pairs_a_new_device() {
    use trair_core::pairing::{Code, PairingGuest};
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {"name": "Julian"}}));
    let me = call(json!({"op": "server_identity"}));
    assert_eq!(me["ok"], true, "{me}");

    let started = call(json!({"op": "pair_host_start", "channel": 42}));
    let code = started["code"].as_str().unwrap().to_string();
    assert!(
        code.starts_with("42-") && code.split('-').count() == 4,
        "{code}"
    );
    let (mut guest, guest_hello) = PairingGuest::start(&Code::parse(&code).unwrap()).unwrap();
    let offer = guest.offer(&unb64(&started["hello"])).unwrap();
    let done =
        call(json!({"op": "pair_host_finish", "hello": b64(&guest_hello), "offer": b64(&offer)}));
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(done["devices"].as_array().unwrap().len(), 2);
    let joined = guest.finish(&unb64(&done["bundle"])).unwrap();
    assert_eq!(
        hex::encode(joined.account.public_key()),
        me["account_key"].as_str().unwrap()
    );
    assert_eq!(
        serde_json::to_value(&joined.list).unwrap(),
        done["device_list"]
    );
    // Used up; and the new list was stored in the vault.
    assert_eq!(
        error(&call(
            json!({"op": "pair_host_finish", "hello": b64(&guest_hello), "offer": b64(&offer)})
        )),
        "no_pairing"
    );
    call(json!({"op": "lock"}));
    let again = call(json!({"op": "unlock", "password": PW}));
    assert_eq!(again["devices"].as_array().unwrap().len(), 2);

    // A wrong code adds nothing and uses the pairing up.
    let started = call(json!({"op": "pair_host_start", "channel": 43}));
    let wrong = Code::random(43).unwrap();
    let (mut bad, bad_hello) = PairingGuest::start(&wrong).unwrap();
    let bad_offer = bad.offer(&unb64(&started["hello"])).unwrap();
    let r =
        call(json!({"op": "pair_host_finish", "hello": b64(&bad_hello), "offer": b64(&bad_offer)}));
    assert_eq!(error(&r), "pairing_failed");
    // Locking ends a running pairing.
    call(json!({"op": "pair_host_start", "channel": 44}));
    call(json!({"op": "lock"}));
    call(json!({"op": "unlock", "password": PW}));
    assert_eq!(
        error(&call(
            json!({"op": "pair_host_finish", "hello": "eA==", "offer": "eA=="})
        )),
        "no_pairing"
    );
    let r = call(json!({"op": "unlock", "password": PW}));
    assert_eq!(
        r["devices"].as_array().unwrap().len(),
        2,
        "nothing was added by the failed runs"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d13_door_as_the_new_device_joins_with_the_code() {
    use trair_core::pairing::PairingHost;
    use trair_core::proofs::verify_login;
    use trair_core::vault::{KdfParams, Vault};
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let host_pw = random_text();
    let (mut host, _) = Vault::create_with(
        &host_pw,
        json!({"name": "Julian"}),
        KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        },
    )
    .unwrap();
    let (mut h, code, host_hello) = PairingHost::start(7, std::time::Instant::now()).unwrap();

    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    assert_eq!(
        error(&call(
            json!({"op": "pair_guest_start", "code": "7-kein-echter-code"})
        )),
        "bad_code"
    );
    let started = call(json!({"op": "pair_guest_start", "code": code.text().to_uppercase()}));
    assert_eq!(started["channel"], 7, "{started}");
    let offered = call(json!({"op": "pair_guest_offer", "hello": b64(&host_hello)}));
    let bundle = h
        .finish(
            &unb64(&started["hello"]),
            &unb64(&offered["offer"]),
            &mut host,
            std::time::Instant::now(),
        )
        .unwrap();
    // Too short a password does not use the pairing up.
    let r = call(json!({"op": "pair_guest_finish", "bundle": b64(&bundle), "password": "kurz"}));
    assert_eq!(error(&r), "weak_password");
    let joined = call(json!({"op": "pair_guest_finish", "bundle": b64(&bundle), "password": PW}));
    assert_eq!(joined["ok"], true, "{joined}");
    assert_eq!(
        joined["fingerprint"],
        host.account().fingerprint().grouped()
    );
    assert_eq!(joined["profile"]["name"], "Julian");
    assert_eq!(joined["devices"].as_array().unwrap().len(), 2);
    assert_eq!(
        error(&call(
            json!({"op": "pair_guest_start", "code": code.text()})
        )),
        "exists"
    );

    // The new device can log in: its signature fits its entry in the list.
    let me = call(json!({"op": "server_identity"}));
    let mut challenge = [0u8; 32];
    getrandom::fill(&mut challenge).unwrap();
    let signed = call(json!({"op": "sign_login", "challenge": hex::encode(challenge)}));
    let list: trair_core::SignedDeviceList =
        serde_json::from_value(me["device_list"].clone()).unwrap();
    let entry = list
        .list
        .devices
        .iter()
        .find(|d| d.id == me["device"].as_str().unwrap())
        .unwrap();
    let sig = hex::decode(signed["signature"].as_str().unwrap()).unwrap();
    assert!(verify_login(entry, me["user"].as_str().unwrap(), &challenge, &sig).is_ok());
    assert_eq!(
        error(&call(json!({"op": "sign_login", "challenge": "zz"}))),
        "bad_request"
    );
    let deletion = call(json!({"op": "sign_deletion", "now": 1_800_000_000u64}));
    assert_eq!(deletion["signature"].as_str().unwrap().len(), 128);
    call(json!({"op": "lock"}));
    assert_eq!(
        error(&call(
            json!({"op": "sign_login", "challenge": hex::encode(challenge)})
        )),
        "locked"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Random text: nothing hard-coded flows into the core's crypto.
fn random_text() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).unwrap();
    bytes.iter().map(|b| char::from(b'a' + b % 26)).collect()
}

#[test]
fn d14_newer_lists_from_the_server_are_taken_only_if_signed_and_fitting() {
    use trair_core::DeviceIdentity;
    use trair_core::vault::{KdfParams, Vault};
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    // This device is the PC; the "laptop" vault shares the account and
    // makes list changes, as a second device would.
    let pc_pw = random_text();
    let (mut laptop, _) = Vault::create_with(
        &random_text(),
        json!({}),
        KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        },
    )
    .unwrap();
    let pc = DeviceIdentity::generate().unwrap();
    let with_pc = laptop
        .device_list()
        .with_device(laptop.account(), pc.entry())
        .unwrap();
    laptop.set_device_list(with_pc.clone()).unwrap();
    // Give the PC its vault through the door by pairing it from the laptop.
    let (mut h, code, hello) =
        trair_core::pairing::PairingHost::start(1, std::time::Instant::now()).unwrap();
    let started = call(json!({"op": "pair_guest_start", "code": code.text()}));
    let offered = call(json!({"op": "pair_guest_offer", "hello": b64(&hello)}));
    let bundle = h
        .finish(
            &unb64(&started["hello"]),
            &unb64(&offered["offer"]),
            &mut laptop,
            std::time::Instant::now(),
        )
        .unwrap();
    let joined =
        call(json!({"op": "pair_guest_finish", "bundle": b64(&bundle), "password": pc_pw}));
    assert_eq!(joined["ok"], true, "{joined}");
    let me = joined["device"].as_str().unwrap().to_string();
    let current = laptop.device_list().clone();

    // Same list again: nothing to do.
    let r = call(json!({"op": "apply_device_list", "device_list": current}));
    assert_eq!(r["changed"], false, "{r}");
    // The laptop pairs a phone: newer, signed, this device still on it.
    let phone = DeviceIdentity::generate().unwrap();
    let newer = current
        .with_device(laptop.account(), phone.entry())
        .unwrap();
    let r = call(json!({"op": "apply_device_list", "device_list": newer}));
    assert_eq!(
        (
            r["changed"].as_bool(),
            r["devices"].as_array().unwrap().len()
        ),
        (Some(true), 4),
        "{r}"
    );
    // A forged list (signed by another account) or one without this device.
    let stranger = Vault::create_with(
        &random_text(),
        json!({}),
        KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        },
    )
    .unwrap()
    .0;
    // Newer by number than this device's list, but signed by a stranger.
    let mut forged = stranger.device_list().clone();
    for _ in 0..4 {
        let extra = DeviceIdentity::generate().unwrap();
        forged = forged
            .with_device(stranger.account(), extra.entry())
            .unwrap();
    }
    assert!(forged.list.seq > 3);
    assert_eq!(
        error(&call(
            json!({"op": "apply_device_list", "device_list": forged})
        )),
        "refused"
    );
    laptop.set_device_list(newer.clone()).unwrap();
    laptop.withdraw_device(&me).unwrap();
    let without_me = laptop.device_list().clone();
    assert_eq!(
        error(&call(
            json!({"op": "apply_device_list", "device_list": without_me})
        )),
        "withdrawn"
    );
    let _ = with_pc;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Bob, outside the door: a real core with its own chat database.
struct Bob {
    m: trair_core::Messenger,
    dir: PathBuf,
    account: trair_core::Account,
    list: trair_core::SignedDeviceList,
}

impl Bob {
    fn new() -> Bob {
        let dir = fresh_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let account = trair_core::Account::generate("local-bob").unwrap();
        let identity = trair_core::DeviceIdentity::generate().unwrap();
        let list = trair_core::SignedDeviceList::first(&account, identity.entry()).unwrap();
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).unwrap();
        let m = trair_core::Messenger::open(
            &dir.join("chats.db"),
            &key,
            &account,
            &identity,
            &list.list,
            1_800_000_000,
        )
        .unwrap();
        Bob {
            m,
            dir,
            account,
            list,
        }
    }

    fn offer(&mut self) -> Value {
        let kp = self.m.key_packages(1).unwrap().remove(0);
        json!([{"device": self.m.device_id(), "package": b64(&kp)}])
    }
}

impl Drop for Bob {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn d15_a_chat_through_the_door_survives_locking() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    assert_eq!(error(&call(json!({"op": "contacts"}))), "locked");
    let created = call(json!({"op": "create", "password": PW, "profile": {"strict": true}}));
    assert_eq!(created["chats"], true, "{created}");
    let mut bob = Bob::new();

    // Codes both ways.
    let code = call(json!({"op": "contact_code"}));
    let my_code = code["code"].as_str().unwrap().to_string();
    assert!(my_code.starts_with("trair1:local-"), "{my_code}");
    assert_eq!(
        error(&call(
            json!({"op": "add_contact", "code": my_code, "name": "ich"})
        )),
        "rejected"
    );
    bob.m
        .add_contact(&my_code, "Julian", 1_800_000_000)
        .unwrap();
    let added = call(json!({"op": "add_contact", "code": bob.m.contact_code(), "name": "Bob"}));
    assert_eq!(added["contact"]["name"], "Bob", "{added}");

    // Start the chat; the server would take the commit.
    let started = call(json!({"op": "start_chat", "user": "local-bob", "offers": bob.offer()}));
    assert_eq!(started["ok"], true, "{started}");
    let chat = started["chat"]["id"].as_str().unwrap().to_string();
    let commit = &started["commit"];
    assert_eq!(commit["welcome_to"], json!([bob.m.device_id()]));
    assert_eq!(
        call(json!({"op": "pending_commits"}))["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let answer = call(json!({"op": "commit_answer", "chat": chat, "accepted": true}));
    assert_eq!(answer["applied"], true, "{answer}");
    assert!(
        call(json!({"op": "pending_commits"}))["commits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let welcome = unb64(&commit["welcome"]);
    assert!(matches!(
        bob.m.receive(&welcome, 1_800_000_000).unwrap(),
        trair_core::Incoming::Chat { .. }
    ));

    // Bob writes; the door reads. The door writes; Bob reads.
    let (bytes, _) = bob.m.send(&chat, "Hallo Julian", 1_800_000_000).unwrap();
    let got = call(json!({"op": "receive", "body": b64(&bytes)}));
    assert_eq!(got["incoming"]["kind"], "message", "{got}");
    assert_eq!(got["incoming"]["message"]["text"], "Hallo Julian");
    assert_eq!(
        call(json!({"op": "receive", "body": b64(&bytes)}))["incoming"]["kind"],
        "duplicate"
    );
    let sent = call(json!({"op": "send_message", "chat": chat, "text": "Hallo Bob"}));
    assert_eq!(sent["to"], json!([bob.m.device_id()]));
    match bob.m.receive(&unb64(&sent["body"]), 1_800_000_000).unwrap() {
        trair_core::Incoming::Message(m) => assert_eq!(m.text, "Hallo Bob"),
        other => panic!("{other:?}"),
    }
    let chats = call(json!({"op": "chats"}));
    assert_eq!(chats["chats"][0]["name"], "Bob");
    assert_eq!(chats["chats"][0]["state"], "active");

    // Locked: nothing. Unlocked: everything is still there.
    call(json!({"op": "lock"}));
    assert_eq!(
        error(&call(json!({"op": "messages", "chat": chat}))),
        "locked"
    );
    let unlocked = call(json!({"op": "unlock", "password": PW}));
    assert_eq!(unlocked["chats"], true);
    let msgs = call(json!({"op": "messages", "chat": chat}));
    let texts: Vec<&str> = msgs["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["Hallo Julian", "Hallo Bob"]);
    let (bytes, _) = bob
        .m
        .send(&chat, "nach dem Sperren", 1_800_000_000)
        .unwrap();
    let got = call(json!({"op": "receive", "body": b64(&bytes)}));
    assert_eq!(
        got["incoming"]["message"]["text"], "nach dem Sperren",
        "{got}"
    );

    // The database file holds no plaintext; deleting the account removes it.
    let file = std::fs::read(dir.join("chats.db")).unwrap();
    for needle in ["Hallo", "Bob", "local-bob"] {
        assert!(
            !file.windows(needle.len()).any(|w| w == needle.as_bytes()),
            "{needle}"
        );
    }
    assert_eq!(
        call(json!({"op": "delete_account", "password": PW}))["ok"],
        true
    );
    assert!(!dir.join("chats.db").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d16_the_chat_ops_refuse_what_does_not_fit() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {}}));
    let mut bob = Bob::new();
    let stranger = bob.offer();
    assert_eq!(
        error(&call(json!({"op": "receive", "body": "kein base64!"}))),
        "bad_request"
    );
    assert_eq!(
        error(&call(json!({"op": "receive", "body": b64(b"zufall")}))),
        "rejected"
    );
    assert_eq!(
        error(&call(
            json!({"op": "start_chat", "user": "local-bob", "offers": stranger})
        )),
        "rejected",
        "Bob is no contact yet"
    );
    assert_eq!(
        error(&call(
            json!({"op": "key_packages", "n": 101, "last_resort": false})
        )),
        "bad_request"
    );
    let kps = call(json!({"op": "key_packages", "n": 2, "last_resort": true}));
    assert_eq!(kps["packages"].as_array().unwrap().len(), 2);
    assert!(kps["last_resort"].is_string());
    assert_eq!(
        error(&call(
            json!({"op": "commit_answer", "chat": "x", "accepted": true})
        )),
        "rejected"
    );
    assert_eq!(
        error(&call(
            json!({"op": "add_contact", "code": "trair1:kaputt", "name": ""})
        )),
        "rejected"
    );
    assert_eq!(
        error(&call(
            json!({"op": "send_message", "chat": "nichts", "text": "x"})
        )),
        "unknown_chat"
    );
    assert_eq!(
        error(&call(json!({"op": "contacts", "extra": 1}))),
        "bad_request"
    );

    // A contact's device list: only with the pinned key and its signature.
    call(json!({"op": "add_contact", "code": bob.m.contact_code(), "name": "Bob"}));
    let other = trair_core::Account::generate("local-bob").unwrap();
    let other_dev = trair_core::DeviceIdentity::generate().unwrap();
    let foreign = trair_core::SignedDeviceList::first(&other, other_dev.entry()).unwrap();
    let wrong_key = call(json!({"op": "apply_contact_list",
        "account_key": hex::encode(other.public_key()), "device_list": foreign}));
    assert_eq!(error(&wrong_key), "blocked");
    // Bob's real list, after he withdrew a lost phone.
    let phone = trair_core::DeviceIdentity::generate().unwrap();
    let with_phone = bob.list.with_device(&bob.account, phone.entry()).unwrap();
    let without = with_phone.without_device(&bob.account, &phone.id).unwrap();
    let key = hex::encode(bob.account.public_key());
    let taken =
        call(json!({"op": "apply_contact_list", "account_key": key, "device_list": without}));
    assert_eq!(
        taken["withdrawn"],
        json!([format!("local-bob:{}", phone.id)]),
        "{taken}"
    );
    // A changed list does not pass the signature check.
    let mut forged = serde_json::to_value(&without).unwrap();
    forged["list"]["withdrawn"] = json!([]);
    let refused =
        call(json!({"op": "apply_contact_list", "account_key": key, "device_list": forged}));
    assert_eq!(error(&refused), "refused");
    call(json!({"op": "lock"}));
    let _ = std::fs::remove_dir_all(&dir);
}
