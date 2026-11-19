// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Emergency PINs: the slot file and the emergency data.

use serde_json::json;
use trair_core::notfall::{self, Action, After, Message, SLOTS, SlotFile};
use trair_core::vault::{KdfParams, Limits, Vault};

const CHEAP: KdfParams = KdfParams {
    m_kib: 8,
    t: 1,
    p: 1,
};
const LIMITS: Limits = Limits {
    min: CHEAP,
    max: Limits::PRODUCTION.max,
};

fn secret(len: usize) -> String {
    let mut b = vec![0u8; len];
    getrandom::fill(&mut b).unwrap();
    b.iter().map(|x| (b'a' + x % 26) as char).collect()
}

fn action(after: After, message: Option<Message>) -> Action {
    Action {
        id: notfall::new_id().unwrap(),
        after,
        message,
    }
}

#[test]
fn n01_pins_open_their_slot_only_and_filler_looks_the_same() {
    let mut f = SlotFile::new(CHEAP).unwrap();
    let empty = f.to_bytes().unwrap();
    let (wipe, send) = (secret(8), secret(14));
    let key = [7u8; 32];
    f.set(0, &wipe, action(After::Vanish, None), None, LIMITS)
        .unwrap();
    let msg = Message {
        chat: "c1".into(),
        text: "Hilfe, ruf mich an".into(),
    };
    f.set(
        2,
        &send,
        action(After::WrongPassword, Some(msg.clone())),
        Some(&key),
        LIMITS,
    )
    .unwrap();
    let f = SlotFile::from_bytes(&f.to_bytes().unwrap()).unwrap();

    let a = f.open(&wipe, LIMITS).unwrap().expect("wipe PIN");
    assert_eq!(
        (a.slot, a.action.after, a.action.message),
        (0, After::Vanish, None)
    );
    assert!(
        a.data_key.is_none(),
        "a PIN that only deletes opens nothing"
    );
    let b = f.open(&send, LIMITS).unwrap().expect("send PIN");
    assert_eq!(b.slot, 2);
    assert_eq!(b.action.message, Some(msg));
    assert_eq!(*b.data_key.unwrap(), key);
    assert!(
        f.open(&secret(9), LIMITS).unwrap().is_none(),
        "a wrong PIN finds nothing"
    );

    // With or without PINs the file has the same size and shape.
    let used = f.to_bytes().unwrap();
    assert_eq!(empty.len(), used.len());
    let text = String::from_utf8(used).unwrap();
    // (Hex digits are everywhere; only words that are no hex are checked.)
    for word in ["Hilfe", "vanish", "wrong_password", "data_key", "message"] {
        assert!(!text.contains(word), "{word} is readable");
    }
    assert_eq!(SLOTS, 3);
}

#[test]
fn n02_a_cleared_slot_no_longer_answers_and_limits_hold() {
    let mut f = SlotFile::new(CHEAP).unwrap();
    let pin = secret(8);
    f.set(1, &pin, action(After::Vanish, None), None, LIMITS)
        .unwrap();
    f.clear(1).unwrap();
    assert!(f.open(&pin, LIMITS).unwrap().is_none());
    assert!(
        f.set(1, "kurz", action(After::Vanish, None), None, LIMITS)
            .is_err(),
        "PIN too short"
    );
    let msg = Some(Message {
        chat: "c".into(),
        text: "t".into(),
    });
    assert!(
        f.set(
            1,
            &pin,
            action(After::Vanish, msg.clone()),
            Some(&[1; 32]),
            LIMITS
        )
        .is_err(),
        "a PIN that opens keys needs password length"
    );
    let long_pin = secret(12);
    f.set(
        1,
        &long_pin,
        action(After::Vanish, msg),
        Some(&[1; 32]),
        LIMITS,
    )
    .unwrap();
    f.clear(1).unwrap();
    assert!(
        f.set(3, &pin, action(After::Vanish, None), None, LIMITS)
            .is_err(),
        "only 3 slots"
    );
    let long = Message {
        chat: "c".into(),
        text: "x".repeat(notfall::MAX_TEXT_CHARS + 1),
    };
    assert!(
        f.set(
            0,
            &long_pin,
            action(After::Vanish, Some(long)),
            Some(&[1; 32]),
            LIMITS
        )
        .is_err()
    );
    // Changed bytes in a slot: that PIN is gone, nothing panics.
    f.set(0, &pin, action(After::Vanish, None), None, LIMITS)
        .unwrap();
    let mut bytes = f.to_bytes().unwrap();
    let pos = bytes.windows(8).position(|w| w == b"\"sealed\"").unwrap() + 12;
    bytes[pos] = if bytes[pos] == b'0' { b'1' } else { b'0' };
    let changed = SlotFile::from_bytes(&bytes).unwrap();
    assert!(changed.open(&pin, LIMITS).unwrap().is_none());
    // A file that asks for absurd memory is refused.
    let mut absurd = SlotFile::new(CHEAP).unwrap().to_bytes().unwrap();
    let text = String::from_utf8(absurd.clone())
        .unwrap()
        .replace("\"m_kib\":8", "\"m_kib\":99999999");
    absurd = text.into_bytes();
    assert!(
        SlotFile::from_bytes(&absurd)
            .unwrap()
            .open(&pin, LIMITS)
            .is_err()
    );
}

#[test]
fn n03_emergency_data_carries_the_keys_but_not_the_rest() {
    let pw = secret(14);
    let (mut v, _) = Vault::create_with(
        &pw,
        json!({"name": "Julian", "server": "http://127.0.0.1:8471", "picture": "x"}),
        CHEAP,
    )
    .unwrap();
    assert!(v.notfall_key().is_none());
    let me = v.device().id.clone();
    v.set_label(&me, "Laptop im Büro").unwrap();
    let key = v.ensure_notfall_key().unwrap();
    let sealed = notfall::seal_data(&key, &v.emergency_secrets().unwrap()).unwrap();
    assert!(
        notfall::open_data(&[0; 32], &sealed).is_err(),
        "another key opens nothing"
    );
    let plain = notfall::open_data(&key, &sealed).unwrap();
    let text = String::from_utf8_lossy(&plain);
    assert!(
        !text.contains("Julian") && !text.contains("picture") && !text.contains("Büro"),
        "only what sending needs"
    );
    let e = Vault::from_emergency_secrets(&plain).unwrap();
    assert_eq!(e.account().fingerprint(), v.account().fingerprint());
    assert_eq!(e.device().id, v.device().id);
    assert_eq!(e.chats_key(), v.chats_key());
    assert_eq!(e.profile()["server"], "http://127.0.0.1:8471");
    // The key and the list survive sealing; without sending PINs the key goes.
    let entry = notfall::Entry {
        slot: 0,
        action: action(
            After::Vanish,
            Some(Message {
                chat: "c".into(),
                text: "t".into(),
            }),
        ),
    };
    v.set_notfall_entries(vec![entry.clone()]);
    let file = v.seal().unwrap();
    let again = Vault::open_with_limits(&file, &pw, LIMITS).unwrap();
    assert_eq!(again.notfall_entries(), [entry]);
    assert_eq!(again.notfall_key(), Some(&key));
    v.set_notfall_entries(vec![]);
    assert!(v.notfall_key().is_none());
    assert!(notfall::filler_data().unwrap().len() >= 2500);
}
