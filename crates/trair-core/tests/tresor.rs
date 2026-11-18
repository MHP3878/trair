// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Attack tests for the vault (the encrypted file with the account key).
//! Most tests use cheap Argon2id settings so they run fast; one test uses
//! the real settings.

use std::time::Instant;

use serde_json::json;
use trair_core::vault::{KdfParams, Limits, MAGIC, MIN_PASSWORD_CHARS, Vault, VaultError};

const PW: &str = "korrekt pferd batterie";
const CHEAP: KdfParams = KdfParams {
    m_kib: 8,
    t: 1,
    p: 1,
};
const TEST_LIMITS: Limits = Limits {
    min: CHEAP,
    max: Limits::PRODUCTION.max,
};

fn profile() -> serde_json::Value {
    json!({"name": "Julian", "glow": 4288777727u32, "strict": true})
}

fn new_vault() -> (Vault, Vec<u8>) {
    Vault::create_with(PW, profile(), CHEAP).expect("create")
}

fn open(file: &[u8], pw: &str) -> Result<Vault, VaultError> {
    Vault::open_with_limits(file, pw, TEST_LIMITS)
}

#[test]
fn t01_right_password_opens_with_same_account_and_profile() {
    let (v, file) = new_vault();
    let again = open(&file, PW).expect("open");
    assert_eq!(again.account().fingerprint(), v.account().fingerprint());
    assert_eq!(again.account().user(), v.account().user());
    assert_eq!(again.profile(), &profile());
    assert!(file.starts_with(MAGIC));
}

#[test]
fn t02_wrong_password_opens_nothing() {
    let (_, file) = new_vault();
    for pw in [
        "korrekt pferd batteriE",
        "",
        "korrekt pferd batterie ",
        "x".repeat(200).as_str(),
    ] {
        assert_eq!(
            open(&file, pw).unwrap_err(),
            VaultError::WrongPassword,
            "{pw:?}"
        );
    }
}

#[test]
fn t03_every_changed_byte_is_noticed() {
    let (_, file) = new_vault();
    for i in 0..file.len() {
        for bit in [0x01u8, 0x80] {
            let mut bad = file.clone();
            bad[i] ^= bit;
            assert!(
                open(&bad, PW).is_err(),
                "byte {i} bit {bit:#x} went unnoticed"
            );
        }
    }
}

#[test]
fn t04_cut_off_file_is_refused_without_panic() {
    let (_, file) = new_vault();
    for len in 0..file.len() {
        assert!(open(&file[..len], PW).is_err(), "length {len}");
    }
    let mut longer = file.clone();
    longer.push(0);
    assert!(open(&longer, PW).is_err());
}

#[test]
fn t05_file_cannot_demand_absurd_memory() {
    let (_, mut file) = new_vault();
    // 4 GiB memory: refused before any work is done.
    file[9..13].copy_from_slice(&(4u32 * 1024 * 1024).to_le_bytes());
    let start = Instant::now();
    assert!(matches!(open(&file, PW), Err(VaultError::Refused(_))));
    assert!(start.elapsed().as_millis() < 500);
    // Absurd number of passes.
    let (_, mut file) = new_vault();
    file[13..17].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(open(&file, PW), Err(VaultError::Refused(_))));
}

#[test]
fn t06_app_refuses_weakened_settings() {
    // A file with cheap settings (an attacker's copy, or an old bug) is
    // never accepted by the app: guessing must stay expensive.
    let (_, file) = new_vault();
    assert!(matches!(
        Vault::open(&file, PW),
        Err(VaultError::Refused(_))
    ));
}

#[test]
fn t07_real_settings_work_and_cost_time() {
    let start = Instant::now();
    let (v, file) = Vault::create(PW, profile()).expect("create");
    let created = start.elapsed();
    let start = Instant::now();
    let again = Vault::open(&file, PW).expect("open");
    let opened = start.elapsed();
    assert_eq!(again.account().fingerprint(), v.account().fingerprint());
    assert_eq!(&file[9..21], &[0, 0, 4, 0, 4, 0, 0, 0, 1, 0, 0, 0]);
    println!("Argon2id 256 MiB, t=4: create {created:?}, open {opened:?}");
    let start = Instant::now();
    assert_eq!(
        Vault::open(&file, "falsches passwort!").unwrap_err(),
        VaultError::WrongPassword
    );
    assert!(
        start.elapsed().as_millis() > 50,
        "a wrong guess must also cost time"
    );
}

#[test]
fn t08_every_seal_uses_fresh_randomness() {
    let (a, fa) = new_vault();
    let (_, fb) = new_vault();
    assert_ne!(fa[21..37], fb[21..37], "salt repeated");
    assert_ne!(fa[37..61], fb[37..61], "nonce repeated");
    let again = a.seal().expect("seal");
    assert_ne!(fa[37..61], again[37..61], "nonce reused on reseal");
    assert_eq!(fa[21..37], again[21..37], "salt stays with the key");
    open(&again, PW).expect("resealed file opens");
    open(&fa, PW).expect("old file still opens");
}

#[test]
fn t09_change_password() {
    let (mut v, _) = new_vault();
    let fp = v.account().fingerprint();
    v.change_password_with("ein ganz anderes passwort", CHEAP)
        .expect("change");
    let file = v.seal().expect("seal");
    assert_eq!(open(&file, PW).unwrap_err(), VaultError::WrongPassword);
    let again = open(&file, "ein ganz anderes passwort").expect("new password");
    assert_eq!(
        again.account().fingerprint(),
        fp,
        "same account after the change"
    );
    assert_eq!(
        v.change_password_with("zu kurz", CHEAP).unwrap_err(),
        VaultError::WeakPassword
    );
}

#[test]
fn t10_short_passwords_are_refused_by_the_core() {
    let short = "ä".repeat(MIN_PASSWORD_CHARS - 1); // counts characters, not bytes
    assert_eq!(
        Vault::create_with(&short, profile(), CHEAP).unwrap_err(),
        VaultError::WeakPassword
    );
    let ok = "ä".repeat(MIN_PASSWORD_CHARS);
    Vault::create_with(&ok, profile(), CHEAP).expect("12 characters are enough");
}

#[test]
fn t11_nothing_readable_in_the_file() {
    let (v, file) = new_vault();
    let text = String::from_utf8_lossy(&file);
    assert!(!text.contains("Julian"));
    assert!(!text.contains(v.account().user()));
    assert!(!text.contains(&hex_of(v.account().public_key())));
    assert!(!file.windows(8).any(|w| w == &v.account().public_key()[..8]));
}

#[test]
fn t12_profile_size_is_limited() {
    let (mut v, _) = new_vault();
    let big = json!({"name": "x".repeat(70 * 1024)});
    assert_eq!(
        v.set_profile(big.clone()).unwrap_err(),
        VaultError::TooLarge
    );
    assert_eq!(
        Vault::create_with(PW, big, CHEAP).unwrap_err(),
        VaultError::TooLarge
    );
    v.set_profile(json!({"name": "Neu"}))
        .expect("small profile");
    let again = open(&v.seal().expect("seal"), PW).expect("open");
    assert_eq!(again.profile(), &json!({"name": "Neu"}));
}

#[test]
fn t13_other_files_are_not_vaults() {
    assert!(matches!(open(b"", PW), Err(VaultError::Damaged(_))));
    assert!(matches!(open(&[0u8; 200], PW), Err(VaultError::Damaged(_))));
    let (_, mut file) = new_vault();
    file[8] = 2; // unknown key derivation
    assert!(matches!(open(&file, PW), Err(VaultError::Damaged(_))));
}

#[test]
fn t14_restored_account_still_signs_devices() {
    let (_, file) = new_vault();
    let v = open(&file, PW).expect("open");
    let device_key = [7u8; 32];
    let cert = v
        .account()
        .certify_device("pc", &device_key)
        .expect("certify");
    let checked =
        trair_core::DeviceCertificate::check(&cert.to_bytes(), &device_key).expect("check");
    assert_eq!(checked.account, v.account().fingerprint());
}

#[test]
fn t15_debug_output_has_no_secrets() {
    let (v, _) = new_vault();
    let text = format!("{v:?}");
    assert!(!text.contains(&hex_of(v.account().public_key())));
    assert!(!text.to_lowercase().contains("key"));
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
