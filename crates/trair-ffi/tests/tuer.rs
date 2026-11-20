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
/// A wrong password for the emergency PIN tests.
const NOT_PW: &str = "falsch-falsch"; // pragma: allowlist secret (Testpasswort)

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
    identity: trair_core::DeviceIdentity,
}

impl Bob {
    fn new() -> Bob {
        Bob::named("local-bob")
    }

    fn named(user: &str) -> Bob {
        let dir = fresh_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let account = trair_core::Account::generate(user).unwrap();
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
            identity,
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

#[test]
fn d17_an_account_is_open_in_one_window_only() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let mine = fresh_dir();
    let other = fresh_dir();
    assert_eq!(call(json!({"op": "init", "dir": mine}))["ok"], true);
    call(json!({"op": "create", "password": PW, "profile": {}}));
    // Another Trair window holds the other account's folder.
    std::fs::create_dir_all(&other).unwrap();
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(other.join(trair_ffi::api::LOCK_FILE))
        .unwrap();
    held.lock().unwrap();
    let refused = call(json!({"op": "init", "dir": other}));
    assert_eq!(error(&refused), "in_use");
    // Nothing changed here: this account is still open.
    assert_eq!(call(json!({"op": "status"}))["unlocked"], true);
    drop(held);
    let s = call(json!({"op": "init", "dir": other}));
    assert_eq!(s["ok"], true, "{s}");
    assert_eq!(s["exists"], false);
    // Switching gave the first folder free again.
    let again = std::fs::OpenOptions::new()
        .write(true)
        .open(mine.join(trair_ffi::api::LOCK_FILE))
        .unwrap();
    assert!(again.try_lock().is_ok(), "the old folder is no longer held");
    drop(again);
    let _ = std::fs::remove_dir_all(&mine);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn d18_qr_codes_come_from_the_core() {
    let code = "trair1:local-0a1b2c3d4e5f:0123456789abcdef0123456789abcdef:ab12"; // pragma: allowlist secret (Beispielcode)
    let q = call(json!({"op": "qr", "text": code}));
    assert_eq!(q["ok"], true, "{q}");
    let size = q["size"].as_u64().unwrap() as usize;
    let rows: Vec<&str> = q["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(rows.len(), size);
    assert!(
        rows.iter()
            .all(|r| r.len() == size && r.bytes().all(|b| b == b'0' || b == b'1'))
    );
    // The three finder squares sit in the corners.
    for (r, c) in [(0, 0), (0, size - 7), (size - 7, 0)] {
        assert_eq!(&rows[r][c..c + 7], "1111111");
        assert_eq!(&rows[r + 6][c..c + 7], "1111111");
    }
    assert_eq!(error(&call(json!({"op": "qr", "text": ""}))), "bad_request");
    assert_eq!(
        error(&call(json!({"op": "qr", "text": "x".repeat(513)}))),
        "bad_request"
    );
    #[cfg(not(windows))]
    assert_eq!(error(&call(json!({"op": "pick_image"}))), "unsupported");
}

fn files_left(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != trair_ffi::api::LOCK_FILE)
        .collect();
    names.sort();
    names
}

#[test]
fn d19_a_pin_that_only_deletes_leaves_nothing() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {"name": "Julian"}}));
    // Every vault has the emergency files from the start, PIN or not.
    assert!(files_left(&dir).contains(&"notfall.trair".to_string()));
    assert!(files_left(&dir).contains(&"notfall-daten.trair".to_string()));
    let pin = "loesch-pin-1";
    let set = call(json!({"op": "notfall_add", "pin": pin, "password": PW, "after": "vanish"}));
    assert_eq!(set["pins"].as_array().unwrap().len(), 1, "{set}");
    assert_eq!(set["pins"][0]["after"], "vanish");
    call(json!({"op": "lock"}));

    let a = call(json!({"op": "unlock", "password": pin}));
    assert_eq!(error(&a), "wrong_password", "looks like a typo");
    assert_eq!(a["notfall"]["send"], false);
    assert_eq!(a["notfall"]["after"], "vanish");
    assert!(files_left(&dir).is_empty(), "left: {:?}", files_left(&dir));
    assert_eq!(call(json!({"op": "status"}))["exists"], false);
    assert_eq!(
        error(&call(json!({"op": "unlock", "password": PW}))),
        "no_vault"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d20_a_pin_can_leave_an_account_that_only_says_wrong_password() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {"name": "Julian"}}));
    let pin = "falsch-pin-2";
    call(json!({"op": "notfall_add", "pin": pin, "password": PW, "after": "wrong_password"}));
    call(json!({"op": "lock"}));
    let before = std::fs::read(dir.join(VAULT_FILE)).unwrap();
    let a = call(json!({"op": "unlock", "password": pin}));
    assert_eq!(error(&a), "wrong_password");
    assert_eq!(a["notfall"]["after"], "wrong_password");
    // A vault is still there (another one), but nothing opens it any more.
    assert_eq!(call(json!({"op": "status"}))["exists"], true);
    assert_ne!(std::fs::read(dir.join(VAULT_FILE)).unwrap(), before);
    let real = call(json!({"op": "unlock", "password": PW}));
    assert_eq!(error(&real), "wrong_password");
    assert!(real.get("notfall").is_none());
    let again = call(json!({"op": "unlock", "password": pin}));
    assert!(
        again.get("notfall").is_none(),
        "the PIN went with the account"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d21_a_pin_sends_its_message_then_everything_is_gone() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    let created = call(
        json!({"op": "create", "password": PW, "profile": {"server": "http://127.0.0.1:8471"}}),
    );
    let account_key = hex::decode(created["account_key"].as_str().unwrap()).unwrap();
    let mut bob = Bob::new();
    let my_code = call(json!({"op": "contact_code"}))["code"]
        .as_str()
        .unwrap()
        .to_string();
    bob.m
        .add_contact(&my_code, "Julian", 1_800_000_000)
        .unwrap();
    call(json!({"op": "add_contact", "code": bob.m.contact_code(), "name": "Bob"}));
    let started = call(json!({"op": "start_chat", "user": "local-bob", "offers": bob.offer()}));
    let chat = started["chat"]["id"].as_str().unwrap().to_string();
    call(json!({"op": "commit_answer", "chat": chat, "accepted": true}));
    bob.m
        .receive(&unb64(&started["commit"]["welcome"]), 1_800_000_000)
        .unwrap();

    let short = call(
        json!({"op": "notfall_add", "pin": "hilfe-3", "password": PW, "after": "vanish",
        "chat": chat, "text": "Ich brauche Hilfe, ruf mich an."}),
    );
    assert_eq!(
        error(&short),
        "weak_sending_pin",
        "a PIN that opens the keys is as long as a password"
    );
    let pin = "hilfe-pin-drei";
    let set = call(
        json!({"op": "notfall_add", "pin": pin, "password": PW, "after": "vanish",
        "chat": chat, "text": "Ich brauche Hilfe, ruf mich an."}),
    );
    assert_eq!(set["pins"][0]["chat_name"], "Bob", "{set}");
    call(json!({"op": "lock"}));

    let a = call(json!({"op": "unlock", "password": pin}));
    assert_eq!(error(&a), "wrong_password");
    let n = &a["notfall"];
    assert_eq!(n["send"], true, "{a}");
    assert_eq!(n["server"], "http://127.0.0.1:8471");
    assert_eq!(n["to"], json!([bob.m.device_id()]));
    match bob.m.receive(&unb64(&n["body"]), 1_800_000_000).unwrap() {
        trair_core::Incoming::Message(m) => assert_eq!(m.text, "Ich brauche Hilfe, ruf mich an."),
        other => panic!("{other:?}"),
    }
    // Log in and delete on the server, with the account's own keys.
    let user = n["user"].as_str().unwrap();
    let del = call(json!({"op": "notfall_sign_deletion", "now": 1_800_000_000u64}));
    let sig = hex::decode(del["signature"].as_str().unwrap()).unwrap();
    assert!(
        trair_core::proofs::verify_account_deletion(&account_key, user, 1_800_000_000, &sig)
            .is_ok()
    );
    let login = call(json!({"op": "notfall_sign_login", "challenge": "ab".repeat(32)}));
    assert!(login["signature"].is_string());
    // Nothing else works in between.
    assert_eq!(error(&call(json!({"op": "contacts"}))), "locked");

    assert_eq!(call(json!({"op": "notfall_finish"}))["ok"], true);
    assert!(files_left(&dir).is_empty(), "left: {:?}", files_left(&dir));
    assert_eq!(error(&call(json!({"op": "notfall_finish"}))), "no_session");
    assert_eq!(
        error(&call(
            json!({"op": "notfall_sign_login", "challenge": "ab"})
        )),
        "no_session"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d22_setting_pins_needs_the_password_and_follows_the_rules() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {}}));
    let add = |pin: &str, pw: &str| {
        call(json!({"op": "notfall_add", "pin": pin, "password": pw, "after": "vanish"}))
    };
    assert_eq!(
        error(&add("pin-eins-1", "falsches passwort")),
        "wrong_password"
    );
    assert_eq!(error(&add("kurz", PW)), "weak_pin");
    assert_eq!(error(&add(PW, PW)), "pin_is_password");
    assert_eq!(add("pin-eins-1", PW)["ok"], true);
    assert_eq!(error(&add("pin-eins-1", PW)), "pin_taken");
    assert_eq!(add("pin-zwei-2", PW)["ok"], true);
    let bad_chat = call(
        json!({"op": "notfall_add", "pin": "pin-fuenf-5-lang", "password": PW, "after": "vanish",
        "chat": "nichts", "text": "x"}),
    );
    assert_eq!(error(&bad_chat), "unknown_chat");
    let three = add("pin-drei-3", PW);
    assert_eq!(three["pins"].as_array().unwrap().len(), 3);
    assert_eq!(error(&add("pin-vier-4", PW)), "full");
    // Removing one frees its slot; that PIN does nothing any more.
    let id = three["pins"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(
        error(&call(
            json!({"op": "notfall_remove", "id": id, "password": NOT_PW})
        )),
        "wrong_password"
    );
    let left = call(json!({"op": "notfall_remove", "id": id, "password": PW}));
    assert_eq!(left["pins"].as_array().unwrap().len(), 2);
    call(json!({"op": "lock"}));
    let a = call(json!({"op": "unlock", "password": "pin-eins-1"}));
    assert!(
        a.get("notfall").is_none(),
        "removed PIN: just a wrong password"
    );
    assert_eq!(call(json!({"op": "unlock", "password": PW}))["ok"], true);
    assert_eq!(
        error(&call(json!({"op": "notfall_list", "password": NOT_PW}))),
        "wrong_password"
    );
    let listed = call(json!({"op": "notfall_list", "password": PW}));
    assert_eq!(listed["pins"].as_array().unwrap().len(), 2);
    assert_eq!(listed["max"], 3);
    // A new password that is one of the PINs is refused.
    let same = call(json!({"op": "change_password", "old": PW, "new": "pin-zwei-2-lang"}));
    assert_eq!(same["ok"], true, "{same}");
    let clash =
        call(json!({"op": "change_password", "old": "pin-zwei-2-lang", "new": "pin-drei-3"}));
    assert_eq!(error(&clash), "pin_is_password");
    call(json!({"op": "lock"}));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A small PNG with a hidden text chunk.
fn png_picture(side: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (side * side * 4) as usize];
    getrandom::fill(&mut pixels).unwrap();
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, side, side);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.add_text_chunk("Kamera".into(), "Standort geheim".into())
            .unwrap();
        let mut w = e.write_header().unwrap();
        w.write_image_data(&pixels).unwrap();
        w.finish().unwrap();
    }
    out
}

#[test]
fn d23_profiles_go_to_contacts_and_come_back_checked() {
    use trair_core::badges::{Badge, BadgeKind, Community};
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    let created = call(json!({"op": "create", "password": PW, "profile": {}}));
    let my_key = hex::decode(created["account_key"].as_str().unwrap()).unwrap();
    let mut bob = Bob::new();
    // Bob made Julian admin of his community: the badge travels along.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let bobs_community = Community::create("Bobs Runde", &bob.account).unwrap();
    let admin = Badge::issue(
        &bob.account,
        BadgeKind::Admin,
        &my_key,
        Some(bobs_community.clone()),
        now,
        3600,
        None,
    )
    .unwrap();
    assert_eq!(call(json!({"op": "add_badge", "badge": admin}))["ok"], true);
    let my_code = call(json!({"op": "contact_code"}))["code"]
        .as_str()
        .unwrap()
        .to_string();
    bob.m
        .add_contact(&my_code, "Julian", 1_800_000_000)
        .unwrap();
    call(json!({"op": "add_contact", "code": bob.m.contact_code(), "name": "Bob"}));
    let started = call(json!({"op": "start_chat", "user": "local-bob", "offers": bob.offer()}));
    let chat = started["chat"]["id"].as_str().unwrap().to_string();
    call(json!({"op": "commit_answer", "chat": chat, "accepted": true}));
    bob.m
        .receive(&unb64(&started["commit"]["welcome"]), 1_800_000_000)
        .unwrap();

    // Julian shares: one message for the chat with Bob, then not again.
    let pic = png_picture(48);
    let mine = json!({"v": 1, "rev": 5, "name": "Julian", "glow": 0xFFA18DFFu32,
        "about": "Hallo", "picture": b64(&pic), "badges": []});
    let shared = call(json!({"op": "share_profile", "profile": mine, "only_contacts": true}));
    assert_eq!(shared["out"].as_array().unwrap().len(), 1, "{shared}");
    let out = &shared["out"][0];
    assert_eq!(out["to"], json!([bob.m.device_id()]));
    assert_eq!(
        call(json!({"op": "profile_sent", "chat": chat, "rev": 5}))["ok"],
        true
    );
    let again = call(json!({"op": "share_profile", "profile": mine, "only_contacts": true}));
    assert!(again["out"].as_array().unwrap().is_empty());
    let got = bob.m.receive(&unb64(&out["body"]), 1_800_000_000).unwrap();
    assert!(
        matches!(got, trair_core::Incoming::Profile { .. }),
        "{got:?}"
    );
    let julian = bob.m.profiles().unwrap().remove(0);
    assert_eq!(julian.profile.name, "Julian");
    assert_eq!(
        julian.profile.badges.len(),
        1,
        "the vault's badge was added"
    );
    assert_eq!(julian.account_key, my_key);
    assert!(
        !String::from_utf8_lossy(julian.profile.picture.as_ref().unwrap()).contains("Standort")
    );
    // A broken profile of one's own is refused before anything goes out.
    let bad = call(
        json!({"op": "share_profile", "profile": {"v": 1, "rev": 6, "name": ""}, "only_contacts": false}),
    );
    assert_eq!(error(&bad), "rejected");

    // Bob's profile with one real badge (he founded a community) and one
    // he copied from someone else.
    let community = bobs_community;
    let real = Badge::issue(
        &bob.account,
        BadgeKind::Founder,
        bob.account.public_key(),
        Some(community.clone()),
        now,
        3600,
        None,
    )
    .unwrap();
    let other = trair_core::Account::generate("lena").unwrap();
    let theirs = Community::create("Lenas Runde", &other).unwrap();
    let copied = Badge::issue(
        &other,
        BadgeKind::Founder,
        other.public_key(),
        Some(theirs),
        now,
        3600,
        None,
    )
    .unwrap();
    let bobs = json!({"v": 1, "rev": 9, "name": "Robert", "glow": 0xFF5EE6B0u32, "about": "",
        "picture": null, "badges": [real, copied]});
    let out = bob.m.share_profile(&bobs, true).unwrap();
    let got = call(json!({"op": "receive", "body": b64(&out[0].bytes)}));
    assert_eq!(got["incoming"]["kind"], "profile", "{got}");
    assert_eq!(got["incoming"]["user"], "local-bob");
    let list = call(json!({"op": "profiles"}));
    let p = &list["profiles"][0];
    assert_eq!(p["name"], "Robert");
    assert_eq!(p["glow"], 0xFF5EE6B0u32);
    assert!(p["picture"].is_null());
    assert_eq!(
        p["badges"].as_array().unwrap().len(),
        1,
        "only his own badge: {p}"
    );
    assert_eq!(p["badges"][0]["community"], "Bobs Runde");
    // Locking and opening again keeps it.
    call(json!({"op": "lock"}));
    assert_eq!(error(&call(json!({"op": "profiles"}))), "locked");
    call(json!({"op": "unlock", "password": PW}));
    assert_eq!(
        call(json!({"op": "profiles"}))["profiles"][0]["name"],
        "Robert"
    );
    call(json!({"op": "lock"}));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d24_a_friendship_key_through_the_door_and_a_chat_that_vanishes() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {}}));
    let me = call(json!({"op": "server_identity"}));
    let my_device = format!(
        "{}:{}",
        me["user"].as_str().unwrap(),
        me["device"].as_str().unwrap()
    );
    let key = call(json!({"op": "create_invite", "minutes": 60}));
    assert_eq!(key["ok"], true, "{key}");
    let code = key["code"].as_str().unwrap().to_string();
    assert!(code.starts_with("trair2:"));
    assert_eq!(
        error(&call(json!({"op": "create_invite", "minutes": 0}))),
        "rejected"
    );
    assert_eq!(
        error(&call(
            json!({"op": "create_invite", "minutes": 31 * 24 * 60})
        )),
        "rejected"
    );

    // Bob uses the key and starts a chat; the token follows.
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut bob = Bob::new();
    bob.m.add_contact(&code, "Julian", t).unwrap();
    let kp = call(json!({"op": "key_packages", "n": 1, "last_resort": false}));
    let offers = vec![trair_core::KeyPackageOffer {
        claimed_device: my_device,
        bytes: unb64(&kp["packages"][0]),
    }];
    let (chat, out) = bob
        .m
        .start_chat(me["user"].as_str().unwrap(), &offers, t)
        .unwrap();
    bob.m.commit_answer(&chat.id, true, None, t).unwrap();
    let welcome = call(json!({"op": "receive", "body": b64(out.welcome.as_ref().unwrap())}));
    assert_eq!(welcome["incoming"]["chat"]["state"], "request", "{welcome}");
    let (bytes, _) = bob.m.invite_message(&chat.id, t).unwrap().unwrap();
    let opened = call(json!({"op": "receive", "body": b64(&bytes)}));
    assert_eq!(opened["incoming"]["kind"], "chat", "{opened}");
    assert_eq!(opened["incoming"]["chat"]["state"], "active");
    assert_eq!(
        call(json!({"op": "contacts"}))["contacts"][0]["user"],
        "local-bob"
    );

    // Let it vanish in an hour, then not; Bob learns both.
    let set = call(json!({"op": "set_chat_end", "chat": chat.id, "minutes": 60}));
    assert!(set["ends"].as_u64().unwrap() > 1_700_000_000, "{set}");
    let listed = call(json!({"op": "chats"}));
    assert_eq!(listed["chats"][0]["ends"], set["ends"]);
    match bob.m.receive(&unb64(&set["body"]), t).unwrap() {
        trair_core::Incoming::Ends { ends, .. } => assert!(ends.is_some()),
        other => panic!("{other:?}"),
    }
    let off = call(json!({"op": "set_chat_end", "chat": chat.id, "minutes": null}));
    assert!(off["ends"].is_null());
    assert!(call(json!({"op": "chats"}))["chats"][0]["ends"].is_null());
    let (bytes, _) = bob.m.set_chat_end(&chat.id, Some(t + 600), t).unwrap();
    let got = call(json!({"op": "receive", "body": b64(&bytes)}));
    assert_eq!(got["incoming"]["kind"], "ends", "{got}");
    assert_eq!(got["incoming"]["ends"], t + 600);
    assert_eq!(call(json!({"op": "chats"}))["chats"][0]["ends"], t + 600);
    assert_eq!(
        call(json!({"op": "sweep"}))["deleted"],
        json!([]),
        "not yet"
    );
    call(json!({"op": "lock"}));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d25_a_group_through_the_door_with_a_key_roles_and_closing() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {}}));
    let me = call(json!({"op": "server_identity"}));
    let my_user = me["user"].as_str().unwrap().to_string();
    let my_device = format!("{my_user}:{}", me["device"].as_str().unwrap());
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut bob = Bob::new();
    let my_code = call(json!({"op": "contact_code"}))["code"]
        .as_str()
        .unwrap()
        .to_string();
    bob.m.add_contact(&my_code, "Julian", t).unwrap();
    let added = call(json!({"op": "add_contact", "code": bob.m.contact_code(), "name": "Bob"}));
    assert_eq!(added["ok"], true, "{added}");

    // Julian starts a group with Bob.
    let made = call(
        json!({"op": "create_group", "name": "Abi 2026", "minutes": null,
        "offers": bob.offer()}),
    );
    assert_eq!(made["ok"], true, "{made}");
    assert_eq!(made["chat"]["kind"], "group");
    assert_eq!(made["commit"]["committers"], json!([my_user]));
    let chat = made["chat"]["id"].as_str().unwrap().to_string();
    let ok = call(json!({"op": "commit_answer", "chat": chat, "accepted": true, "taken": null}));
    assert_eq!(ok["applied"], true, "{ok}");
    match bob
        .m
        .receive(&unb64(&made["commit"]["welcome"]), t)
        .unwrap()
    {
        trair_core::Incoming::Chat { chat: c, .. } => {
            assert_eq!(c.kind, trair_core::ChatKind::Group);
            assert_eq!(c.state, trair_core::ChatState::Active);
        }
        other => panic!("{other:?}"),
    }
    let listed = call(json!({"op": "chats"}));
    assert_eq!(listed["chats"][0]["name"], "Abi 2026", "{listed}");
    assert_eq!(listed["chats"][0]["role"], "owner");
    assert_eq!(listed["chats"][0]["members"], 2);

    // A key for guests; Gus (a stranger) uses it.
    assert_eq!(
        error(&call(
            json!({"op": "create_group_invite", "chat": chat, "minutes": 0})
        )),
        "bad_request"
    );
    let key =
        call(json!({"op": "create_group_invite", "chat": chat, "minutes": 60, "guest": true}));
    assert_eq!(key["ok"], true, "{key}");
    let code = key["code"].as_str().unwrap().to_string();
    assert!(code.starts_with("trair3:"));
    assert_eq!(
        call(json!({"op": "group_invites", "chat": chat}))["count"],
        1
    );
    let mut gus = Bob::named("local-gus");
    let kp = call(json!({"op": "key_packages", "n": 1, "last_resort": false}));
    let offers = vec![trair_core::KeyPackageOffer {
        claimed_device: my_device.clone(),
        bytes: unb64(&kp["packages"][0]),
    }];
    let (jc, out) = gus.m.join_group(&code, &offers, t).unwrap();
    let mut both = vec!["local-gus".to_string(), my_user.clone()];
    both.sort();
    assert_eq!(out.committers, Some(both));
    gus.m.commit_answer(&jc.id, true, None, t).unwrap();
    let hidden = call(json!({"op": "receive", "body": b64(out.welcome.as_ref().unwrap())}));
    assert_eq!(hidden["incoming"]["kind"], "other", "{hidden}");
    let (bytes, _) = gus.m.join_message(&jc.id).unwrap().unwrap();
    let req = call(json!({"op": "receive", "body": b64(&bytes)}));
    assert_eq!(req["incoming"]["kind"], "join_request", "{req}");
    assert_eq!(req["incoming"]["user"], "local-gus");
    assert_eq!(req["incoming"]["group"], chat.as_str());
    let jchat = req["incoming"]["chat"].as_str().unwrap().to_string();
    let waiting = call(json!({"op": "join_requests"}));
    assert_eq!(waiting["requests"][0]["chat"], jchat.as_str(), "{waiting}");
    let admitted = call(json!({"op": "admit", "chat": jchat, "offers": gus.offer()}));
    assert_eq!(admitted["ok"], true, "{admitted}");
    let ok = call(json!({"op": "commit_answer", "chat": chat, "accepted": true, "taken": null}));
    assert_eq!(ok["applied"], true);
    assert_eq!(call(json!({"op": "close_join", "chat": jchat}))["ok"], true);
    assert!(
        call(json!({"op": "join_requests"}))["requests"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let joined = gus
        .m
        .receive(&unb64(&admitted["commit"]["welcome"]), t)
        .unwrap();
    assert!(
        matches!(joined, trair_core::Incoming::Chat { .. }),
        "{joined:?}"
    );
    bob.m
        .receive(&unb64(&admitted["commit"]["commit"]), t)
        .unwrap();
    let info = call(json!({"op": "group_info", "chat": chat}));
    let members = info["group"]["members"].as_array().unwrap();
    assert_eq!(members.len(), 3, "{info}");
    let g = members.iter().find(|m| m["user"] == "local-gus").unwrap();
    assert_eq!(g["role"], "guest");
    assert_eq!(g["guest_until"], key["ends"]);

    // Bob becomes admin; the server learns him as one who may change it.
    let p = call(json!({"op": "set_admin", "chat": chat, "user": "local-bob", "admin": true}));
    let mut both = vec!["local-bob".to_string(), my_user.clone()];
    both.sort();
    assert_eq!(p["commit"]["committers"], json!(both), "{p}");
    call(json!({"op": "commit_answer", "chat": chat, "accepted": true, "taken": null}));
    // The owner cannot just leave; nothing to tidy right now.
    assert_eq!(
        error(&call(json!({"op": "leave_group", "chat": chat}))),
        "not_allowed"
    );
    let tidy = call(json!({"op": "tidy_group", "chat": chat}));
    assert!(tidy["commit"].is_null() && tidy["body"].is_null(), "{tidy}");
    // Kick Gus, then close the group: gone here.
    let p = call(json!({"op": "kick", "chat": chat, "user": "local-gus"}));
    assert_eq!(p["commit"]["removed"].as_array().unwrap().len(), 1, "{p}");
    call(json!({"op": "commit_answer", "chat": chat, "accepted": true, "taken": null}));
    let p = call(json!({"op": "close_group", "chat": chat}));
    assert_eq!(p["ok"], true, "{p}");
    call(json!({"op": "commit_answer", "chat": chat, "accepted": true, "taken": null}));
    assert!(
        call(json!({"op": "chats"}))["chats"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    call(json!({"op": "lock"}));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Someone writing in an open channel, outside the door.
fn open_item(
    account: &trair_core::Account,
    identity: &trair_core::DeviceIdentity,
    text: &str,
    sent: u64,
) -> Value {
    let m = trair_core::proofs::OpenMessage {
        community: "c1",
        channel: "allgemein",
        user: account.user(),
        device: &identity.id,
        sent,
        text,
    };
    let sig = trair_core::proofs::sign_open_message(identity, &m).unwrap();
    json!({"user": account.user(), "device": identity.id, "text": text, "sent": sent, "sig": hex::encode(sig)})
}

fn author(account: &trair_core::Account, list: &trair_core::SignedDeviceList) -> Value {
    json!({"account_key": hex::encode(account.public_key()), "device_list": list})
}

fn check(messages: Value, authors: Value) -> Vec<String> {
    let r = call(
        json!({"op": "check_open_messages", "community": "c1", "channel": "allgemein",
        "messages": messages, "authors": authors}),
    );
    assert_eq!(r["ok"], true, "{r}");
    r["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn d26_open_messages_are_signed_here_and_checked_against_pinned_keys() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    assert_eq!(
        error(&call(
            json!({"op": "sign_open_message", "community": "c1", "channel": "allgemein", "text": "x", "sent": 1})
        )),
        "locked"
    );
    call(json!({"op": "create", "password": PW, "profile": {"name": "Julian"}}));
    let me = call(json!({"op": "server_identity"}));
    let user = me["user"].as_str().unwrap().to_string();

    // Our own message, signed in here.
    let signed = call(
        json!({"op": "sign_open_message", "community": "c1", "channel": "allgemein",
        "text": "Hallo", "sent": 1_800_000_000_u64}),
    );
    assert_eq!(signed["ok"], true, "{signed}");
    assert_eq!(signed["user"], user.as_str());
    let own = json!({"user": user, "device": signed["device"], "text": "Hallo",
        "sent": 1_800_000_000_u64, "sig": signed["signature"]});
    let mine =
        json!({user.clone(): {"account_key": me["account_key"], "device_list": me["device_list"]}});
    assert_eq!(check(json!([own.clone()]), mine.clone()), ["own"]);
    let mut changed = own.clone();
    changed["text"] = json!("Tschüss");
    assert_eq!(check(json!([changed]), mine.clone()), ["bad"]);
    // The server hands out another key under my own name: never "own".
    let twin = trair_core::Account::generate(&user).unwrap();
    let twin_id = trair_core::DeviceIdentity::generate().unwrap();
    let twin_list = trair_core::SignedDeviceList::first(&twin, twin_id.entry()).unwrap();
    assert_eq!(
        check(
            json!([open_item(&twin, &twin_id, "Ich bin du", 1_800_000_000)]),
            json!({user.clone(): author(&twin, &twin_list)})
        ),
        ["changed"]
    );

    // A contact: known key, then compared in person.
    let bob = Bob::new();
    call(json!({"op": "add_contact", "code": bob.m.contact_code(), "name": "Bob"}));
    let from_bob = open_item(&bob.account, &bob.identity, "Moin", 1_800_000_000);
    let bob_keys = json!({"local-bob": author(&bob.account, &bob.list)});
    assert_eq!(
        check(json!([from_bob.clone()]), bob_keys.clone()),
        ["contact"]
    );
    assert_eq!(
        call(json!({"op": "confirm_contact", "user": "local-bob"}))["ok"],
        true
    );
    assert_eq!(
        check(json!([from_bob.clone()]), bob_keys.clone()),
        ["verified"]
    );
    // The server hands out another key for Bob: never the contact's.
    let fake = trair_core::Account::generate("local-bob").unwrap();
    let fake_id = trair_core::DeviceIdentity::generate().unwrap();
    let fake_list = trair_core::SignedDeviceList::first(&fake, fake_id.entry()).unwrap();
    assert_eq!(
        check(
            json!([open_item(&fake, &fake_id, "Ich bin Bob", 1_800_000_000)]),
            json!({"local-bob": author(&fake, &fake_list)})
        ),
        ["changed"]
    );

    // A stranger: pinned the first time, then known; another key later
    // is a warning until it is accepted.
    let eve = trair_core::Account::generate("local-eve").unwrap();
    let eve_id = trair_core::DeviceIdentity::generate().unwrap();
    let eve_list = trair_core::SignedDeviceList::first(&eve, eve_id.entry()).unwrap();
    let from_eve = open_item(&eve, &eve_id, "Hi", 1_800_000_000);
    let eve_keys = json!({"local-eve": author(&eve, &eve_list)});
    assert_eq!(
        check(
            json!([from_eve.clone(), from_eve.clone()]),
            eve_keys.clone()
        ),
        ["new", "known"]
    );
    let eve2 = trair_core::Account::generate("local-eve").unwrap();
    let eve2_id = trair_core::DeviceIdentity::generate().unwrap();
    let eve2_list = trair_core::SignedDeviceList::first(&eve2, eve2_id.entry()).unwrap();
    let eve2_keys = json!({"local-eve": author(&eve2, &eve2_list)});
    let from_eve2 = open_item(&eve2, &eve2_id, "Neues Handy", 1_800_000_100);
    assert_eq!(
        check(json!([from_eve2.clone()]), eve2_keys.clone()),
        ["changed"]
    );
    // No team badge, no team inbox (and nothing while locked).
    assert_eq!(error(&call(json!({"op": "team_badge"}))), "none");
    // The pin survives locking.
    call(json!({"op": "lock"}));
    assert_eq!(error(&call(json!({"op": "team_badge"}))), "locked");
    assert_eq!(
        error(&call(
            json!({"op": "check_open_messages", "community": "c1", "channel": "allgemein", "messages": [], "authors": {}})
        )),
        "locked"
    );
    call(json!({"op": "unlock", "password": PW}));
    assert_eq!(
        check(json!([from_eve2.clone()]), eve2_keys.clone()),
        ["changed"]
    );
    let accepted = call(
        json!({"op": "accept_open_key", "user": "local-eve", "account_key": hex::encode(eve2.public_key())}),
    );
    assert_eq!(accepted["ok"], true, "{accepted}");
    assert_eq!(
        check(
            json!([from_eve2.clone(), from_eve.clone()]),
            eve2_keys.clone()
        ),
        ["known", "bad"]
    );

    // Missing keys, a device not in the list, a signature from someone
    // else, broken hex: all bad.
    assert_eq!(check(json!([from_eve.clone()]), json!({})), ["bad"]);
    let mut other_device = from_bob.clone();
    other_device["device"] = json!(eve_id.id);
    assert_eq!(check(json!([other_device]), bob_keys.clone()), ["bad"]);
    let mut borrowed = from_eve.clone();
    borrowed["user"] = json!("local-bob");
    assert_eq!(check(json!([borrowed]), bob_keys.clone()), ["bad"]);
    let mut broken = from_bob.clone();
    broken["sig"] = json!("zz");
    assert_eq!(check(json!([broken]), bob_keys), ["bad"]);
    // Another channel or community is another message.
    let r = call(
        json!({"op": "check_open_messages", "community": "c2", "channel": "allgemein",
        "messages": [from_eve], "authors": eve_keys}),
    );
    assert_eq!(r["results"], json!(["bad"]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d27_a_closed_channel_through_the_door_signed_with_history_and_deletion() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    call(json!({"op": "create", "password": PW, "profile": {}}));
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Bob is no contact: the keeper (this door) vouches for him.
    let mut bob = Bob::new();
    let group = "c1".repeat(16);
    assert_eq!(
        error(&call(
            json!({"op": "create_channel_group", "group": "kurz", "name": "team",
            "community": "k1", "channel": "c1", "offers": []})
        )),
        "rejected"
    );
    let made = call(
        json!({"op": "create_channel_group", "group": group, "name": "team",
        "community": "k1", "channel": "c1", "offers": bob.offer()}),
    );
    assert_eq!(made["ok"], true, "{made}");
    assert_eq!(made["chat"]["kind"], "channel");
    assert_eq!(made["chat"]["id"], group.as_str());
    let ok = call(json!({"op": "commit_answer", "chat": group, "accepted": true, "taken": null}));
    assert_eq!(ok["applied"], true, "{ok}");
    match bob
        .m
        .receive(&unb64(&made["commit"]["welcome"]), t)
        .unwrap()
    {
        trair_core::Incoming::Chat { chat: c, .. } => {
            assert_eq!(c.kind, trair_core::ChatKind::Channel)
        }
        other => panic!("{other:?}"),
    }
    let listed = call(json!({"op": "chats"}));
    let c = listed["chats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == group.as_str())
        .unwrap()
        .clone();
    assert_eq!(c["channel"], json!({"community": "k1", "channel": "c1"}));
    assert_eq!(c["role"], "owner");
    assert_eq!(c["name"], "team");

    // Bob writes: the door stores it with his device signature.
    let (bytes, _) = bob.m.send(&group, "Hallo aus dem Kanal", t).unwrap();
    let got = call(json!({"op": "receive", "body": b64(&bytes)}));
    assert_eq!(got["incoming"]["kind"], "message", "{got}");
    let msgs = call(json!({"op": "messages", "chat": group}));
    let m = msgs["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["text"] == "Hallo aus dem Kanal")
        .unwrap()
        .clone();
    assert_eq!(m["user"], "local-bob");
    assert_eq!(m["sent"], t);
    let key = hex::decode(&bob.list.list.devices[0].key).unwrap();
    let signed = trair_core::proofs::OpenMessage {
        community: "k1",
        channel: "c1",
        user: "local-bob",
        device: m["device"].as_str().unwrap(),
        sent: t,
        text: "Hallo aus dem Kanal",
    };
    trair_core::proofs::verify_closed_message(
        &key,
        &signed,
        &hex::decode(m["sig"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    // The door writes, signed by this device; Bob gets it.
    let sent = call(json!({"op": "send_message", "chat": group, "text": "Willkommen"}));
    assert_eq!(sent["ok"], true, "{sent}");
    let got = bob.m.receive(&unb64(&sent["body"]), t).unwrap();
    assert!(
        matches!(got, trair_core::Incoming::Message(ref m) if m.text == "Willkommen"),
        "{got:?}"
    );

    // Carl joins and gets the history, only for him.
    let mut carl = Bob::named("local-carl");
    let added = call(json!({"op": "add_to_channel", "chat": group, "offers": carl.offer()}));
    assert_eq!(added["ok"], true, "{added}");
    call(json!({"op": "commit_answer", "chat": group, "accepted": true, "taken": null}));
    bob.m
        .receive(&unb64(&added["commit"]["commit"]), t)
        .unwrap();
    carl.m
        .receive(&unb64(&added["commit"]["welcome"]), t)
        .unwrap();
    assert_eq!(
        call(json!({"op": "channel_history", "chat": group, "user": "local-nobody"}))["none"],
        true
    );
    let h = call(json!({"op": "channel_history", "chat": group, "user": "local-carl"}));
    assert_eq!(h["to"].as_array().unwrap().len(), 1, "{h}");
    assert!(h["to"][0].as_str().unwrap().starts_with("local-carl:"));
    carl.m.receive(&unb64(&h["body"]), t).unwrap();
    let texts: Vec<String> = carl
        .m
        .messages(&group, None, 50)
        .unwrap()
        .into_iter()
        .filter(|m| !m.user.is_empty())
        .map(|m| m.text)
        .collect();
    assert_eq!(texts, ["Hallo aus dem Kanal", "Willkommen"]);

    // Bob becomes a keeper; then the door deletes his text and removes Carl.
    let me = call(json!({"op": "server_identity"}))["user"]
        .as_str()
        .unwrap()
        .to_string();
    let k = call(json!({"op": "set_keepers", "chat": group, "keepers": [me, "local-bob"]}));
    assert!(k["commit"].is_object(), "{k}");
    call(json!({"op": "commit_answer", "chat": group, "accepted": true, "taken": null}));
    bob.m.receive(&unb64(&k["commit"]["commit"]), t).unwrap();
    carl.m.receive(&unb64(&k["commit"]["commit"]), t).unwrap();
    assert_eq!(
        bob.m.group_view(&group).unwrap().me,
        trair_core::Role::Admin
    );
    let del = call(json!({"op": "delete_channel_message", "chat": group, "id": m["id"]}));
    assert_eq!(del["ok"], true, "{del}");
    bob.m.receive(&unb64(&del["body"]), t).unwrap();
    let left: Vec<String> = bob
        .m
        .messages(&group, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(
        !left.contains(&"Hallo aus dem Kanal".to_string()),
        "{left:?}"
    );
    let gone = call(json!({"op": "messages", "chat": group}));
    assert!(
        !gone["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["text"] == "Hallo aus dem Kanal")
    );
    let removed = call(json!({"op": "channel_remove", "chat": group, "user": "local-carl"}));
    assert_eq!(removed["ok"], true, "{removed}");
    assert_eq!(
        error(&call(
            json!({"op": "channel_remove", "chat": group, "user": "local-carl"})
        )),
        "rejected",
        "one change at a time"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn d28_bot_codes_and_vouching_through_the_door() {
    use trair_core::Account;
    use trair_core::bots::{BotCert, parse_request, secret_hash, verify};
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = fresh_dir();
    call(json!({"op": "init", "dir": dir}));
    assert_eq!(error(&call(json!({"op": "bot_request"}))), "locked");
    assert_eq!(
        error(&call(json!({"op": "vouch_bot", "code": "trairbot1:x:y:z"}))),
        "locked"
    );
    let created = call(json!({"op": "create", "password": PW, "profile": {"name": "Julian"}}));
    assert_eq!(created["ok"], true);
    let me = call(json!({"op": "server_identity"}));
    let owner_key = hex::decode(me["account_key"].as_str().unwrap()).unwrap();

    // This account as a bot: a fresh code each time, the server gets the
    // hash of its secret.
    let a = call(json!({"op": "bot_request"}));
    let b = call(json!({"op": "bot_request"}));
    assert_ne!(a["code"], b["code"]);
    let req = parse_request(a["code"].as_str().unwrap()).unwrap();
    assert_eq!(req.bot, me["user"].as_str().unwrap());
    assert_eq!(req.key, owner_key);
    assert_eq!(secret_hash(&req.secret), a["hash"].as_str().unwrap());
    // Its own bot: refused.
    assert_eq!(
        error(&call(json!({"op": "vouch_bot", "code": a["code"]}))),
        "rejected"
    );

    // Vouching for another account's bot.
    let bot = Account::generate("wetterbot").unwrap();
    let (code, hash) = trair_core::bots::request_code(&bot).unwrap();
    let v = call(json!({"op": "vouch_bot", "code": code}));
    assert_eq!(v["ok"], true, "{v}");
    let cert: BotCert = serde_json::from_value(v["cert"].clone()).unwrap();
    verify(&cert, &owner_key, Some("wetterbot"), Some(bot.public_key())).unwrap();
    assert_eq!(
        secret_hash(&hex::decode(v["secret"].as_str().unwrap()).unwrap()),
        hash
    );
    for bad in ["", "trairbot1:wetterbot", "trair1:abc", &format!("{code}x")] {
        assert_eq!(
            error(&call(json!({"op": "vouch_bot", "code": bad}))),
            "rejected",
            "{bad:?}"
        );
    }
    // No profile of that bot here yet.
    assert_eq!(
        error(&call(
            json!({"op": "check_bot", "user": "wetterbot", "owner_key": hex::encode(&owner_key)})
        )),
        "not_found"
    );
    call(json!({"op": "lock"}));
    let _ = std::fs::remove_dir_all(&dir);
}
