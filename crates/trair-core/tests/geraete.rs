// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! M2: device keys in the vault, the signed device list, withdrawing a
//! device, and the upgrade of vaults made by Paket 1.

use std::sync::LazyLock;

use serde_json::json;
use trair_core::vault::{DEFAULT_LABEL, KdfParams, Limits, Vault, VaultError};
use trair_core::{CoreError, Device, DeviceIdentity, Trust};

static PW: LazyLock<String> = LazyLock::new(|| secret(24));
const CHEAP: KdfParams = KdfParams {
    m_kib: 8,
    t: 1,
    p: 1,
};
const TEST_LIMITS: Limits = Limits {
    min: CHEAP,
    max: Limits::PRODUCTION.max,
};

fn secret(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).expect("random");
    bytes.iter().map(|b| char::from(b'a' + b % 26)).collect()
}

fn vault() -> Vault {
    Vault::create_with(&PW, json!({"name": "Julian"}), CHEAP)
        .expect("create")
        .0
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixtures/{name}")).expect("fixture")
}

fn fixture_password() -> String {
    std::fs::read_to_string("tests/fixtures/vault-v1.password")
        .expect("fixture")
        .trim_end()
        .to_string()
}

#[test]
fn g01_new_vault_has_a_signed_device() {
    let v = vault();
    let list = v
        .device_list()
        .verify(v.account().public_key(), None)
        .expect("signed by the account");
    assert_eq!(list.seq, 1);
    assert_eq!(list.devices, vec![v.device().entry()]);
    assert!(v.device().id.starts_with("d-") && v.device().id.len() == 10);
    assert_eq!(
        v.labels().get(&v.device().id).map(String::as_str),
        Some(DEFAULT_LABEL)
    );
    // The device key survives sealing and opening.
    let again = Vault::open_with_limits(&v.seal().unwrap(), &PW, TEST_LIMITS).unwrap();
    assert_eq!(again.device().entry(), v.device().entry());
    assert!(!again.migrated());
}

#[test]
fn g02_vault_from_paket_1_is_upgraded_and_keeps_the_account() {
    let file = fixture("vault-v1-cheap.bin");
    let expected = std::fs::read_to_string("tests/fixtures/vault-v1-cheap.fingerprint").unwrap();
    let v =
        Vault::open_with_limits(&file, &fixture_password(), TEST_LIMITS).expect("old vault opens");
    assert!(v.migrated(), "must be stored again right away");
    assert_eq!(
        v.account().fingerprint().as_str(),
        expected.trim(),
        "same account as before"
    );
    assert_eq!(v.profile()["name"], "Julian");
    v.device_list()
        .verify(v.account().public_key(), None)
        .expect("new list is signed");
    // After storing, it is a normal version 2 vault with the same device.
    let stored = v.seal().unwrap();
    let again = Vault::open_with_limits(&stored, &fixture_password(), TEST_LIMITS).unwrap();
    assert!(!again.migrated());
    assert_eq!(again.device().entry(), v.device().entry());
    assert_eq!(again.account().fingerprint(), v.account().fingerprint());
}

#[test]
fn g03_withdrawn_device_is_refused_even_without_strict_mode() {
    let mut v = vault();
    // A second own device (as pairing would add it).
    let other = DeviceIdentity::generate().unwrap();
    let list = v
        .device_list()
        .with_device(v.account(), other.entry())
        .unwrap();
    v.set_device_list(list).unwrap();
    let mut me = Device::from_identity(v.account(), v.device()).unwrap();
    let mut laptop = Device::from_identity(v.account(), &other).unwrap();
    me.set_strict(false);

    // Before: the laptop is a trusted own device.
    me.create_group("g").unwrap();
    let kp = laptop.key_packages(1).unwrap().remove(0);
    let offer = trair_core::KeyPackageOffer {
        claimed_device: laptop.id().to_string(),
        bytes: kp,
    };
    let out = me.add_members("g", std::slice::from_ref(&offer)).unwrap();
    me.confirm("g").unwrap();
    let joined = laptop.receive(out.welcome.as_ref().unwrap()).unwrap();
    assert!(matches!(joined, trair_core::Event::Joined { .. }));
    let trust = me
        .members("g")
        .unwrap()
        .into_iter()
        .find(|m| m.id == laptop.id())
        .unwrap()
        .trust;
    assert_eq!(trust, Trust::OwnAccount);

    // Withdraw it: from now on it is refused, strict mode or not.
    v.withdraw_device(&other.id).unwrap();
    let list = v
        .device_list()
        .verify(v.account().public_key(), None)
        .unwrap()
        .clone();
    assert!(list.is_withdrawn(&other.id) && !list.contains(&other.id));
    me.apply_device_list(&list);
    let trust = me
        .members("g")
        .unwrap()
        .into_iter()
        .find(|m| m.id == laptop.id())
        .unwrap()
        .trust;
    assert_eq!(trust, Trust::Withdrawn);
    assert!(matches!(
        me.encrypt("g", "hallo"),
        Err(CoreError::Withdrawn(_))
    ));
    me.create_group("neu").unwrap();
    let kp = laptop.key_packages(1).unwrap().remove(0);
    let offer = trair_core::KeyPackageOffer {
        claimed_device: laptop.id().to_string(),
        bytes: kp,
    };
    assert!(
        me.add_members("neu", &[offer]).is_err(),
        "a withdrawn device is never added again"
    );
}

#[test]
fn g04_device_cannot_withdraw_itself_and_lists_cannot_go_back() {
    let mut v = vault();
    let own = v.device().id.clone();
    assert!(matches!(
        v.withdraw_device(&own),
        Err(VaultError::Refused(_))
    ));
    let old = v.device_list().clone();
    let other = DeviceIdentity::generate().unwrap();
    let newer = old.with_device(v.account(), other.entry()).unwrap();
    v.set_device_list(newer.clone()).unwrap();
    // The old list (fewer devices, lower number) is refused.
    assert!(v.set_device_list(old).is_err());
    assert!(
        v.set_device_list(newer).is_err(),
        "same number is not newer"
    );
}

#[test]
fn g05_list_signed_by_another_account_is_refused() {
    let mut v = vault();
    let stranger = vault();
    let list = stranger.device_list().clone();
    assert!(list.verify(v.account().public_key(), None).is_err());
    assert!(v.set_device_list(list).is_err());
    // A list whose content was changed after signing.
    let other = DeviceIdentity::generate().unwrap();
    let mut forged = v
        .device_list()
        .with_device(v.account(), other.entry())
        .unwrap();
    forged
        .list
        .devices
        .push(DeviceIdentity::generate().unwrap().entry());
    assert!(v.set_device_list(forged).is_err());
}

#[test]
fn g06_device_names_are_checked_and_stay_local() {
    let mut v = vault();
    let id = v.device().id.clone();
    v.set_label(&id, "Julians Laptop").unwrap();
    for bad in ["", " Laptop", "Laptop ", "a\nb", &"x".repeat(41)] {
        assert!(v.set_label(&id, bad).is_err(), "{bad:?}");
    }
    assert!(
        v.set_label("d-00000000", "Fremd").is_err(),
        "unknown device"
    );
    // Names are never part of the signed list that others see.
    let list = serde_json::to_string(v.device_list()).unwrap();
    assert!(!list.contains("Julians Laptop"));
}

#[test]
fn g07_a_device_restored_from_the_vault_keeps_its_identity() {
    let v = vault();
    let a = Device::from_identity(v.account(), v.device()).unwrap();
    let b = Device::from_identity(v.account(), v.device()).unwrap();
    assert_eq!(a.id(), format!("{}:{}", v.account().user(), v.device().id));
    assert_eq!(a.id(), b.id());
}

#[test]
fn g08_a_withdrawn_key_cannot_come_back_under_a_new_id() {
    let mut v = vault();
    let other = DeviceIdentity::generate().unwrap();
    let list = v
        .device_list()
        .with_device(v.account(), other.entry())
        .unwrap();
    v.set_device_list(list).unwrap();
    v.withdraw_device(&other.id).unwrap();
    let mut again = other.entry();
    again.id = "d-0badc0de".into();
    assert!(
        v.device_list()
            .with_device(v.account(), again.clone())
            .is_err(),
        "withdrawn key"
    );
    again.key = again.key.to_uppercase();
    assert!(
        v.device_list().with_device(v.account(), again).is_err(),
        "same key in capitals"
    );
    // This device's own key under another id, and keys that are no keys.
    let mut copy = v.device().entry();
    copy.id = "d-0badc0df".into();
    assert!(
        v.device_list().with_device(v.account(), copy).is_err(),
        "own key twice"
    );
    for key in ["", "zz", "00"] {
        let mut bad = DeviceIdentity::generate().unwrap().entry();
        bad.key = key.into();
        assert!(
            v.device_list().with_device(v.account(), bad).is_err(),
            "{key:?}"
        );
    }
}
