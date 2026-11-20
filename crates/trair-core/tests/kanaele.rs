// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! M8, step 3b: closed channels of communities with the real core. The
//! in-memory server from `common` takes changes only from the accounts the
//! channel names (its keepers); "Mallory" is a device with a modified
//! Trair, driven at the MLS level, that tries to forge texts and history.

mod common;

use std::path::PathBuf;
use std::rc::Rc;

use common::Net;
use serde_json::{Value, json};
use trair_core::chats::CHATS_FILE;
use trair_core::devices::DeviceList;
use trair_core::groups::ChannelRef;
use trair_core::proofs::{OpenMessage, sign_closed_message, verify_closed_message};
use trair_core::{
    Account, ChatKind, ChatState, CoreError, Device, DeviceIdentity, Event, Incoming, Messenger,
    PendingCommit, Role, SignedDeviceList,
};

const NOW: u64 = 1_800_000_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).unwrap();
        let p = std::env::temp_dir().join(format!("trair-channels-{}", hex::encode(b)));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Phone {
    _dir: TempDir,
    path: PathBuf,
    key: [u8; 32],
    account: Rc<Account>,
    identity: DeviceIdentity,
    list: DeviceList,
    m: Option<Messenger>,
}

impl Phone {
    fn m(&mut self) -> &mut Messenger {
        self.m.as_mut().unwrap()
    }

    fn id(&self) -> String {
        self.m.as_ref().unwrap().device_id().to_string()
    }

    fn user(&self) -> String {
        self.account.user().to_string()
    }

    fn device_key(&self) -> Vec<u8> {
        hex::decode(self.identity.entry().key).unwrap()
    }

    fn restart(&mut self) {
        self.m = None;
        self.m = Some(
            Messenger::open(
                &self.path,
                &self.key,
                &self.account,
                &self.identity,
                &self.list,
                NOW,
            )
            .unwrap(),
        );
    }
}

fn person(user: &str) -> Phone {
    let account = Rc::new(Account::generate(user).unwrap());
    let identity = DeviceIdentity::generate().unwrap();
    let list = SignedDeviceList::first(&account, identity.entry()).unwrap();
    let dir = TempDir::new();
    let path = dir.0.join(CHATS_FILE);
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).unwrap();
    let m = Messenger::open(&path, &key, &account, &identity, &list.list, NOW).unwrap();
    Phone {
        _dir: dir,
        path,
        key,
        account,
        identity,
        list: list.list,
        m: Some(m),
    }
}

/// A device with a modified Trair: real keys, but it sends whatever it
/// likes.
struct Mallory {
    identity: DeviceIdentity,
    dev: Device,
}

fn mallory(net: &mut Net) -> Mallory {
    let account = Account::generate("mallory").unwrap();
    let identity = DeviceIdentity::generate().unwrap();
    let mut dev = Device::from_identity(&account, &identity).unwrap();
    dev.set_strict(false);
    net.register(&mut dev, 3);
    Mallory { identity, dev }
}

fn group_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).unwrap();
    hex::encode(b)
}

fn channel_ref() -> ChannelRef {
    ChannelRef {
        community: "k1".into(),
        channel: "c1".into(),
    }
}

fn publish(net: &mut Net, p: &mut Phone, n: usize) {
    let id = p.id();
    let kps = p.m().key_packages(n).unwrap();
    let lr = p.m().last_resort_key_package().unwrap();
    net.register_packages(&id, kps, lr);
}

fn commit(net: &mut Net, p: &mut Phone, out: &PendingCommit) -> bool {
    let id = p.id();
    let ok = net
        .commit_as(
            &id,
            &out.chat,
            out.epoch,
            &out.commit,
            out.welcome.as_deref(),
            &out.welcome_to,
            &out.to,
            out.committers.as_deref(),
        )
        .is_ok();
    let taken = net.taken(&out.chat);
    p.m()
        .commit_answer(&out.chat, ok, taken.as_deref(), NOW)
        .unwrap()
}

fn poll(net: &mut Net, p: &mut Phone) -> Vec<Result<Incoming, CoreError>> {
    let id = p.id();
    net.inbox(&id)
        .iter()
        .map(|b| p.m().receive(b, NOW))
        .collect()
}

fn poll_ok(net: &mut Net, p: &mut Phone) -> Vec<Incoming> {
    poll(net, p)
        .into_iter()
        .map(|r| r.unwrap_or_else(|e| panic!("could not process a message: {e}")))
        .collect()
}

fn poll_dev(net: &mut Net, m: &mut Mallory) -> Vec<Event> {
    let id = m.dev.id().to_string();
    net.inbox(&id)
        .iter()
        .map(|b| m.dev.receive(b).unwrap())
        .collect()
}

fn send(net: &mut Net, p: &mut Phone, chat: &str, text: &str) -> i64 {
    let to = p.m().recipients(chat).unwrap();
    let (bytes, msg) = p.m().send(chat, text, NOW).unwrap();
    let id = p.id();
    net.send(&id, &to, &bytes);
    msg.id
}

fn out(net: &mut Net, p: &mut Phone, (bytes, to): (Vec<u8>, Vec<String>)) {
    let id = p.id();
    net.send(&id, &to, &bytes);
}

fn texts(p: &mut Phone, chat: &str) -> Vec<String> {
    p.m()
        .messages(chat, None, 500)
        .unwrap()
        .into_iter()
        .filter(|m| !m.user.is_empty())
        .map(|m| m.text)
        .collect()
}

fn events(p: &mut Phone, chat: &str) -> Vec<String> {
    p.m()
        .messages(chat, None, 500)
        .unwrap()
        .into_iter()
        .filter(|m| m.user.is_empty())
        .map(|m| {
            let v: Value = serde_json::from_str(&m.text).unwrap();
            v["event"].as_str().unwrap().to_string()
        })
        .collect()
}

/// Olga (a keeper) makes a channel with these people; they are no
/// contacts of hers.
fn channel_of(net: &mut Net, olga: &mut Phone, others: &mut [&mut Phone]) -> String {
    let mut offers = Vec::new();
    for o in others.iter_mut() {
        publish(net, o, 3);
        offers.extend(net.fetch(&o.user()));
    }
    let group = group_id();
    let (chat, pending) = olga
        .m()
        .create_channel(&group, "team", channel_ref(), &offers, NOW)
        .unwrap();
    assert_eq!(chat.id, group);
    assert_eq!(chat.kind, ChatKind::Channel);
    assert!(commit(net, olga, &pending.unwrap()));
    for o in others.iter_mut() {
        let got = poll_ok(net, o);
        assert!(
            matches!(&got[..], [Incoming::Chat { chat: c, .. }] if c.id == group && c.kind == ChatKind::Channel && c.state == ChatState::Active),
            "{got:?}"
        );
    }
    group
}

#[test]
fn c01_a_keeper_lets_strangers_in_and_every_text_is_signed() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    let chat = channel_of(&mut net, &mut olga, &mut [&mut ada, &mut max]);
    assert_eq!(ada.m().channel_of(&chat).unwrap(), Some(channel_ref()));
    assert_eq!(events(&mut ada, &chat), ["joined"]);
    assert_eq!(
        events(&mut olga, &chat),
        ["created", "added"],
        "every addition is a note"
    );
    let view = max.m().group_view(&chat).unwrap();
    assert_eq!(view.me, Role::Member);
    assert_eq!(view.members[0].user, "olga");
    assert_eq!(view.members[0].role, Role::Owner);

    // Ada writes; everyone gets it with Ada's device signature.
    let mine = send(&mut net, &mut ada, &chat, "Hallo Team");
    for p in [&mut olga, &mut max] {
        let got = poll_ok(&mut net, p);
        assert!(
            matches!(&got[..], [Incoming::Message(m)] if m.text == "Hallo Team" && m.user == "ada"),
            "{got:?}"
        );
        assert_eq!(texts(p, &chat), ["Hallo Team"]);
    }
    let theirs = olga
        .m()
        .messages(&chat, None, 10)
        .unwrap()
        .pop()
        .unwrap()
        .id;
    let ada_key = ada.device_key();
    for (p, id) in [(&mut ada, mine), (&mut olga, theirs)] {
        let t = p.m().channel_signature(id).unwrap().unwrap();
        assert_eq!(
            (t.user.as_str(), t.text.as_str(), t.sent),
            ("ada", "Hallo Team", NOW)
        );
        let m = OpenMessage {
            community: "k1",
            channel: "c1",
            user: "ada",
            device: &t.device,
            sent: t.sent,
            text: &t.text,
        };
        verify_closed_message(&ada_key, &m, &t.sig).unwrap();
    }

    // Members change nothing; only keepers do.
    publish(&mut net, &mut olga, 1);
    let eve = {
        let mut eve = person("eve");
        publish(&mut net, &mut eve, 2);
        eve
    };
    let offers = net.fetch(&eve.user());
    assert!(matches!(
        max.m().add_to_channel(&chat, &offers),
        Err(CoreError::NotAllowed(_))
    ));
    assert!(matches!(
        max.m().channel_remove(&chat, "ada"),
        Err(CoreError::NotAllowed(_))
    ));
    assert!(max.m().set_keepers(&chat, &["max".into()]).is_err());
    assert!(max.m().channel_history(&chat, "ada").is_err());
}

#[test]
fn c02_forged_or_unsigned_texts_are_never_stored() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut mal = mallory(&mut net);
    publish(&mut net, &mut ada, 3);
    let mut offers = net.fetch("ada");
    offers.extend(net.fetch("mallory"));
    let group = group_id();
    let (_, pending) = olga
        .m()
        .create_channel(&group, "team", channel_ref(), &offers, NOW)
        .unwrap();
    assert!(commit(&mut net, &mut olga, &pending.unwrap()));
    poll_ok(&mut net, &mut ada);
    assert!(matches!(
        &poll_dev(&mut net, &mut mal)[..],
        [Event::Joined { .. }]
    ));
    let to = mal.dev.recipients(&group).unwrap();
    let mid = mal.dev.id().to_string();
    let sign = |identity: &DeviceIdentity, community: &str, text: &str| {
        sign_closed_message(
            identity,
            &OpenMessage {
                community,
                channel: "c1",
                user: "mallory",
                device: &identity.id,
                sent: NOW,
                text,
            },
        )
        .unwrap()
    };

    // Without a signature, with one over other words, for another
    // community, or as someone else: dropped.
    let plain = mal.dev.encrypt(&group, "ohne Unterschrift").unwrap();
    net.send(&mid, &to, &plain);
    let other_words = sign(&mal.identity, "k1", "harmlos");
    let b = mal
        .dev
        .encrypt_signed(&group, "Ich bin Olga", NOW, &other_words)
        .unwrap();
    net.send(&mid, &to, &b);
    let other_place = sign(&mal.identity, "k2", "falscher Ort");
    let b = mal
        .dev
        .encrypt_signed(&group, "falscher Ort", NOW, &other_place)
        .unwrap();
    net.send(&mid, &to, &b);
    let olgas = DeviceIdentity::generate().unwrap();
    let foreign = sign(&olgas, "k1", "fremder Schlüssel");
    let b = mal
        .dev
        .encrypt_signed(&group, "fremder Schlüssel", NOW, &foreign)
        .unwrap();
    net.send(&mid, &to, &b);
    for p in [&mut olga, &mut ada] {
        let got = poll_ok(&mut net, p);
        assert!(got.iter().all(|g| matches!(g, Incoming::Other)), "{got:?}");
        assert!(texts(p, &group).is_empty());
    }
    // Correctly signed by its own device, Mallory's text counts like any.
    let real = sign(&mal.identity, "k1", "echt von mir");
    let b = mal
        .dev
        .encrypt_signed(&group, "echt von mir", NOW, &real)
        .unwrap();
    net.send(&mid, &to, &b);
    assert!(
        matches!(&poll_ok(&mut net, &mut ada)[..], [Incoming::Message(m)] if m.user == "mallory")
    );

    // A history from someone who is no keeper is ignored.
    let forged = json!({"history": [{"user": "olga", "device": "d-00000000", "text": "erfunden",
        "sent": NOW, "sig": hex::encode(&real)}], "for": "ada"});
    let b = mal.dev.encrypt_control(&group, &forged).unwrap();
    net.send(&mid, &to, &b);
    poll_ok(&mut net, &mut ada);
    assert_eq!(texts(&mut ada, &group), ["echt von mir"]);
}

#[test]
fn c03_newcomers_get_the_last_50_texts_and_no_keeper_can_forge_them() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    let mut mal = mallory(&mut net);
    publish(&mut net, &mut ada, 3);
    let mut offers = net.fetch("ada");
    offers.extend(net.fetch("mallory"));
    let group = group_id();
    let (_, pending) = olga
        .m()
        .create_channel(&group, "team", channel_ref(), &offers, NOW)
        .unwrap();
    assert!(commit(&mut net, &mut olga, &pending.unwrap()));
    poll_ok(&mut net, &mut ada);
    poll_dev(&mut net, &mut mal);
    for i in 1..=55 {
        send(&mut net, &mut ada, &group, &format!("Nachricht {i}"));
    }
    poll_ok(&mut net, &mut olga);
    poll_dev(&mut net, &mut mal);

    // Max comes in; Olga's device hands him the last 50, only to him.
    publish(&mut net, &mut max, 3);
    let offers = net.fetch("max");
    let p = olga.m().add_to_channel(&group, &offers).unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    poll_ok(&mut net, &mut max);
    poll_ok(&mut net, &mut ada);
    poll_dev(&mut net, &mut mal);
    let (bytes, to) = olga.m().channel_history(&group, "max").unwrap().unwrap();
    assert_eq!(to, [max.id()], "only to the newcomer");
    out(&mut net, &mut olga, (bytes, to));
    poll_ok(&mut net, &mut max);
    let got = texts(&mut max, &group);
    assert_eq!(got.len(), 50);
    assert_eq!(got[0], "Nachricht 6");
    assert_eq!(got[49], "Nachricht 55");
    assert!(events(&mut max, &group).contains(&"history".to_string()));
    // Handed on twice: nothing doubles.
    let again = olga.m().channel_history(&group, "max").unwrap().unwrap();
    out(&mut net, &mut olga, again);
    poll_ok(&mut net, &mut max);
    assert_eq!(texts(&mut max, &group).len(), 50);
    // The others never got it (and still read the next text: the skipped
    // step of Olga's sending ratchet is no problem).
    assert_eq!(texts(&mut ada, &group).len(), 55);
    send(&mut net, &mut olga, &group, "Willkommen Max");
    poll_ok(&mut net, &mut ada);
    assert!(texts(&mut ada, &group).contains(&"Willkommen Max".to_string()));

    // Mallory becomes a keeper and hands on a history with a text changed
    // and one in Ada's name signed by Mallory: both are dropped.
    let keepers = vec!["olga".to_string(), "mallory".to_string()];
    let p = olga.m().set_keepers(&group, &keepers).unwrap().unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    poll_dev(&mut net, &mut mal);
    poll_ok(&mut net, &mut max);
    let real = olga.m().messages(&group, None, 500).unwrap();
    let first = real.iter().find(|m| m.text == "Nachricht 1").unwrap().id;
    let t = olga.m().channel_signature(first).unwrap().unwrap();
    let mallorys = sign_closed_message(
        &mal.identity,
        &OpenMessage {
            community: "k1",
            channel: "c1",
            user: "ada",
            device: &t.device,
            sent: NOW,
            text: "Ada schuldet Mallory Geld",
        },
    );
    assert!(
        mallorys.is_err(),
        "the core does not even sign for another device"
    );
    let fake = json!({"history": [
        {"user": "ada", "device": t.device, "text": "Nachricht 1 (geändert)", "sent": t.sent,
            "sig": hex::encode(&t.sig)},
        {"user": "ada", "device": t.device, "text": "Nachricht 1", "sent": t.sent,
            "sig": hex::encode(&t.sig)},
    ], "for": "max"});
    let b = mal.dev.encrypt_control(&group, &fake).unwrap();
    let max_id = max.id();
    net.send(mal.dev.id(), &[max_id], &b);
    poll_ok(&mut net, &mut max);
    let got = texts(&mut max, &group);
    assert!(!got.iter().any(|t| t.contains("geändert")), "{got:?}");
    assert!(
        got.contains(&"Nachricht 1".to_string()),
        "a real old text from a keeper is taken"
    );
}

#[test]
fn c04_keepers_remove_and_delete_members_cannot() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    let chat = channel_of(&mut net, &mut olga, &mut [&mut ada, &mut max]);
    // Ada becomes a keeper; then she removes Olga, who made the channel.
    let p = olga
        .m()
        .set_keepers(&chat, &["olga".into(), "ada".into()])
        .unwrap()
        .unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);
    assert_eq!(ada.m().group_view(&chat).unwrap().me, Role::Admin);
    assert!(
        olga.m()
            .set_keepers(&chat, &["olga".into(), "ada".into()])
            .unwrap()
            .is_none(),
        "nothing to change"
    );
    let p = ada.m().channel_remove(&chat, "olga").unwrap();
    assert!(commit(&mut net, &mut ada, &p));
    let got = poll_ok(&mut net, &mut olga);
    assert!(
        matches!(&got[..], [Incoming::Changed { left: true, .. }]),
        "{got:?}"
    );
    assert_eq!(olga.m().chat(&chat).unwrap().state, ChatState::Left);
    poll_ok(&mut net, &mut max);
    let view = max.m().group_view(&chat).unwrap();
    assert_eq!(
        view.members
            .iter()
            .map(|m| m.user.as_str())
            .collect::<Vec<_>>(),
        ["ada", "max"]
    );
    assert_eq!(view.members[0].role, Role::Owner);

    // Max writes something; he may delete his own text, not Ada's; Ada
    // deletes his.
    let spam = send(&mut net, &mut max, &chat, "Spam");
    let own = send(&mut net, &mut max, &chat, "Tippfehler");
    let adas = send(&mut net, &mut ada, &chat, "Hallo");
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);
    let adas_on_max = max
        .m()
        .messages(&chat, None, 50)
        .unwrap()
        .into_iter()
        .find(|m| m.text == "Hallo")
        .unwrap()
        .id;
    assert!(matches!(
        max.m().delete_channel_message(&chat, adas_on_max),
        Err(CoreError::NotAllowed(_))
    ));
    let o = max.m().delete_channel_message(&chat, own).unwrap();
    out(&mut net, &mut max, o);
    let spam_on_ada = ada
        .m()
        .messages(&chat, None, 50)
        .unwrap()
        .into_iter()
        .find(|m| m.text == "Spam")
        .unwrap()
        .id;
    let o = ada.m().delete_channel_message(&chat, spam_on_ada).unwrap();
    out(&mut net, &mut ada, o);
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);
    for p in [&mut ada, &mut max] {
        assert_eq!(texts(p, &chat), ["Hallo"]);
    }
    let _ = (spam, adas);

    // After a restart the signatures are still there.
    ada.restart();
    let hallo = ada
        .m()
        .messages(&chat, None, 50)
        .unwrap()
        .into_iter()
        .find(|m| m.text == "Hallo")
        .unwrap()
        .id;
    assert_eq!(
        ada.m().channel_signature(hallo).unwrap().unwrap().text,
        "Hallo"
    );
    assert_eq!(ada.m().channel_of(&chat).unwrap(), Some(channel_ref()));
}
