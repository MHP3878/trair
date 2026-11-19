// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! M4, step 1: contacts and one-to-one chats with the real core, stored
//! in the encrypted chat database (SQLCipher). Every "device" here has its
//! own database file; the in-memory server from `common` orders commits
//! like the real one will (one commit per chat and epoch).

mod common;

use std::path::PathBuf;
use std::rc::Rc;

use common::Net;
use trair_core::chats::CHATS_FILE;
use trair_core::devices::DeviceList;
use trair_core::{
    Account, ChatState, CoreError, DeviceIdentity, Incoming, Messenger, PendingCommit,
    SignedDeviceList,
};

const NOW: u64 = 1_800_000_000;

/// A folder that is removed again when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).unwrap();
        let p = std::env::temp_dir().join(format!("trair-chats-{}", hex::encode(b)));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One device of an account, with everything it needs to reopen its
/// chat database later.
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

    fn restart_ok(&mut self) -> bool {
        self.m = None;
        match Messenger::open(
            &self.path,
            &self.key,
            &self.account,
            &self.identity,
            &self.list,
            NOW,
        ) {
            Ok(m) => {
                self.m = Some(m);
                true
            }
            Err(_) => false,
        }
    }

    /// Close and open the database again, as after a restart.
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

fn random_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    getrandom::fill(&mut k).unwrap();
    k
}

fn phone(account: &Rc<Account>, identity: DeviceIdentity, list: &DeviceList) -> Phone {
    let dir = TempDir::new();
    let path = dir.0.join(CHATS_FILE);
    let key = random_key();
    let m = Messenger::open(&path, &key, account, &identity, list, NOW).unwrap();
    Phone {
        _dir: dir,
        path,
        key,
        account: Rc::clone(account),
        identity,
        list: list.clone(),
        m: Some(m),
    }
}

/// A new account with one device.
fn person(user: &str) -> Phone {
    let account = Rc::new(Account::generate(user).unwrap());
    let identity = DeviceIdentity::generate().unwrap();
    let list = SignedDeviceList::first(&account, identity.entry()).unwrap();
    phone(&account, identity, &list.list)
}

/// A second device of the same account (as after pairing).
fn second_device(first: &mut Phone) -> Phone {
    let identity = DeviceIdentity::generate().unwrap();
    let signed = SignedDeviceList {
        list: first.list.clone(),
        signature: String::new(),
    }
    .with_device(&first.account, identity.entry())
    .unwrap();
    first.list = signed.list.clone();
    first.m().apply_device_list(&signed.list).unwrap();
    let account = Rc::clone(&first.account);
    phone(&account, identity, &signed.list)
}

fn publish(net: &mut Net, p: &mut Phone, n: usize) {
    let id = p.id();
    let kps = p.m().key_packages(n).unwrap();
    let lr = p.m().last_resort_key_package().unwrap();
    net.register_packages(&id, kps, lr);
}

/// Hand a change to the server, as the app will: apply it if the server
/// took it (or took exactly this one before), else throw it away.
fn commit(net: &mut Net, p: &mut Phone, out: &PendingCommit) -> bool {
    let id = p.id();
    let ok = net
        .commit_raw(
            &id,
            &out.chat,
            out.epoch,
            &out.commit,
            out.welcome.as_deref(),
            &out.welcome_to,
            &out.to,
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

fn send(net: &mut Net, p: &mut Phone, chat: &str, text: &str) {
    let recipients = p.m().recipients(chat).unwrap();
    let (bytes, _) = p.m().send(chat, text, NOW).unwrap();
    let id = p.id();
    net.send(&id, &recipients, &bytes);
}

fn texts(p: &mut Phone, chat: &str) -> Vec<String> {
    p.m()
        .messages(chat, None, 100)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect()
}

/// Alice and Bob exchange codes; Alice starts a chat.
fn chat_between(net: &mut Net, alice: &mut Phone, bob: &mut Phone) -> String {
    let a_code = alice.m().contact_code();
    let b_code = bob.m().contact_code();
    alice.m().add_contact(&b_code, "Bob", NOW).unwrap();
    bob.m().add_contact(&a_code, "Alice", NOW).unwrap();
    let bob_user = bob.account.user().to_string();
    let alice_user = alice.account.user().to_string();
    let mut offers = net.fetch(&bob_user);
    offers.extend(net.fetch(&alice_user));
    let (chat, out) = alice.m().start_chat(&bob_user, &offers, NOW).unwrap();
    assert!(commit(net, alice, &out));
    chat.id
}

#[test]
fn contacts_chat_and_everything_survives_a_restart() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut bob = person("bob");
    publish(&mut net, &mut bob, 2);
    // Bob restarts after publishing: the private halves of his key
    // packages must be on disk, or he could never join.
    bob.restart();
    let chat = chat_between(&mut net, &mut alice, &mut bob);

    let joined = poll_ok(&mut net, &mut bob);
    assert!(matches!(
        &joined[..],
        [Incoming::Chat { chat: c, update_keys: false }] if c.id == chat && c.state == ChatState::Active
    ));
    send(&mut net, &mut alice, &chat, "Hallo Bob");
    send(&mut net, &mut bob, &chat, "nicht jetzt");
    assert!(
        matches!(&poll_ok(&mut net, &mut bob)[..], [Incoming::Message(m)] if m.text == "Hallo Bob" && !m.outgoing)
    );
    assert!(
        matches!(&poll_ok(&mut net, &mut alice)[..], [Incoming::Message(m)] if m.text == "nicht jetzt")
    );

    // Both restart. History, contacts and the MLS state come back.
    alice.restart();
    bob.restart();
    assert_eq!(texts(&mut alice, &chat), ["Hallo Bob", "nicht jetzt"]);
    // Bob wrote before he read Alice's message: his order differs.
    assert_eq!(texts(&mut bob, &chat), ["nicht jetzt", "Hallo Bob"]);
    assert_eq!(alice.m().contacts().unwrap()[0].name, "Bob");
    send(&mut net, &mut alice, &chat, "nach dem Neustart");
    assert!(
        matches!(&poll_ok(&mut net, &mut bob)[..], [Incoming::Message(m)] if m.text == "nach dem Neustart" && m.missing == 0)
    );
    assert_eq!(alice.m().chats().unwrap()[0].peer, bob.account.user());
}

#[test]
fn the_file_is_encrypted_and_bound_to_its_device() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut bob = person("bob");
    publish(&mut net, &mut bob, 1);
    let chat = chat_between(&mut net, &mut alice, &mut bob);
    poll_ok(&mut net, &mut bob);
    send(&mut net, &mut alice, &chat, "geheime Nachricht 4711");
    poll_ok(&mut net, &mut bob);
    alice.m = None;

    let file = std::fs::read(&alice.path).unwrap();
    for needle in [
        "geheime Nachricht".as_bytes(),
        b"4711",
        bob.account.user().as_bytes(),
        bob.account.fingerprint().as_str().as_bytes(),
        chat.as_bytes(),
        b"SQLite format",
    ] {
        assert!(
            !file.windows(needle.len()).any(|w| w == needle),
            "{} is readable in the file",
            String::from_utf8_lossy(needle)
        );
    }

    // Another key opens nothing.
    let wrong = Messenger::open(
        &alice.path,
        &random_key(),
        &alice.account,
        &alice.identity,
        &alice.list,
        NOW,
    );
    assert!(matches!(wrong, Err(CoreError::Storage(_))), "{wrong:?}");
    // The right key with another device of the same account: refused.
    let other = DeviceIdentity::generate().unwrap();
    let wrong_device = Messenger::open(
        &alice.path,
        &alice.key,
        &alice.account,
        &other,
        &alice.list,
        NOW,
    );
    assert!(
        matches!(wrong_device, Err(CoreError::Storage(_))),
        "{wrong_device:?}"
    );
    // And it still opens with the right one.
    alice.restart();
    assert_eq!(texts(&mut alice, &chat), ["geheime Nachricht 4711"]);
}

#[test]
fn a_stranger_is_a_request_until_accepted() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut mallory = person("mallory");
    publish(&mut net, &mut alice, 2);
    // Mallory got Alice's code somewhere; Alice does not know Mallory.
    let a_code = alice.m().contact_code();
    mallory.m().add_contact(&a_code, "Alice", NOW).unwrap();
    let offers = net.fetch(alice.account.user());
    let (chat, out) = mallory
        .m()
        .start_chat(alice.account.user(), &offers, NOW)
        .unwrap();
    assert!(commit(&mut net, &mut mallory, &out));
    send(&mut net, &mut mallory, &chat.id, "kauf das hier");

    let got = poll_ok(&mut net, &mut alice);
    assert!(matches!(&got[0], Incoming::Chat { chat: c, .. } if c.state == ChatState::Request));
    assert!(matches!(&got[1], Incoming::Message(m) if m.text == "kauf das hier"));
    assert!(alice.m().contacts().unwrap().is_empty());
    // No answer before accepting, strict mode or not.
    assert!(matches!(
        alice.m().send(&chat.id, "wer bist du", NOW),
        Err(CoreError::Blocked(_))
    ));
    alice.m().set_strict(false).unwrap();
    assert!(matches!(
        alice.m().send(&chat.id, "wer bist du", NOW),
        Err(CoreError::Blocked(_))
    ));
    alice.m().set_strict(true).unwrap();

    // Accepting pins Mallory's key from the chat; then answering works.
    let contact = alice.m().accept_request(&chat.id, "", NOW).unwrap();
    assert_eq!(contact.fingerprint, mallory.account.fingerprint());
    assert_eq!(
        contact.name,
        mallory.account.user(),
        "no name given: the user name"
    );
    assert!(!contact.confirmed);
    send(&mut net, &mut alice, &chat.id, "nein danke");
    assert!(
        matches!(&poll_ok(&mut net, &mut mallory)[..], [Incoming::Message(m)] if m.text == "nein danke")
    );

    // A second stranger is declined: chat, keys and messages are gone.
    let mut spam = person("spam");
    spam.m().add_contact(&a_code, "", NOW).unwrap();
    let offers = net.fetch(alice.account.user());
    let (spam_chat, out) = spam
        .m()
        .start_chat(alice.account.user(), &offers, NOW)
        .unwrap();
    assert!(commit(&mut net, &mut spam, &out));
    send(&mut net, &mut spam, &spam_chat.id, "Gewinnspiel");
    poll_ok(&mut net, &mut alice);
    alice.m().decline_request(&spam_chat.id).unwrap();
    assert!(alice.m().chat(&spam_chat.id).is_err());
    assert!(
        alice
            .m()
            .messages(&spam_chat.id, None, 10)
            .unwrap()
            .is_empty()
    );
    send(&mut net, &mut spam, &spam_chat.id, "noch mal");
    assert!(
        poll(&mut net, &mut alice)[0].is_err(),
        "a declined chat stays closed"
    );
}

#[test]
fn a_device_signed_by_another_key_is_never_added_or_followed() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut bob = person("bob");
    // The server operator makes his own account called "bob".
    let mut fake_bob = person("bob");
    publish(&mut net, &mut fake_bob, 2);
    publish(&mut net, &mut alice, 2);
    let b_code = bob.m().contact_code();
    alice.m().add_contact(&b_code, "Bob", NOW).unwrap();

    // Alice asks for Bob's devices and gets the fake one: refused.
    let offers = net.fetch("bob");
    let refused = alice.m().start_chat("bob", &offers, NOW);
    assert!(matches!(refused, Err(CoreError::Blocked(_))), "{refused:?}");
    assert!(
        alice.m().chats().unwrap().is_empty(),
        "nothing stays behind"
    );

    // The fake Bob writes first: not even a request, it is dropped.
    let a_code = alice.m().contact_code();
    fake_bob.m().add_contact(&a_code, "", NOW).unwrap();
    let offers = net.fetch("alice");
    let (_, out) = fake_bob.m().start_chat("alice", &offers, NOW).unwrap();
    assert!(commit(&mut net, &mut fake_bob, &out));
    let got = poll(&mut net, &mut alice);
    assert!(matches!(got[..], [Err(CoreError::Blocked(_))]), "{got:?}");
    assert!(alice.m().chats().unwrap().is_empty());

    // The same code twice is fine; a code with another key for Bob is not.
    alice.m().add_contact(&b_code, "Bob", NOW).unwrap();
    let fake_code = fake_bob.m().contact_code();
    assert!(matches!(
        alice.m().add_contact(&fake_code, "Bob", NOW),
        Err(CoreError::Blocked(_))
    ));
    // A typo in a code is caught.
    let mut typo = b_code.clone();
    typo.pop();
    typo.push(if b_code.ends_with('0') { '1' } else { '0' });
    assert!(alice.m().add_contact(&typo, "Bob", NOW).is_err());
    // And nobody adds themselves.
    assert!(alice.m().add_contact(&a_code, "ich", NOW).is_err());
}

#[test]
fn two_changes_at_once_one_wins_the_other_tries_again() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut bob = person("bob");
    publish(&mut net, &mut bob, 4);
    publish(&mut net, &mut alice, 4);
    let chat = chat_between(&mut net, &mut alice, &mut bob);
    poll_ok(&mut net, &mut bob);

    // Both get a second device at the same moment.
    let mut alice2 = second_device(&mut alice);
    let mut bob2 = second_device(&mut bob);
    publish(&mut net, &mut alice2, 2);
    publish(&mut net, &mut bob2, 2);
    let a_offer = net
        .fetch("alice")
        .into_iter()
        .filter(|o| o.claimed_device == alice2.id())
        .collect::<Vec<_>>();
    let b_offer = net
        .fetch("bob")
        .into_iter()
        .filter(|o| o.claimed_device == bob2.id())
        .collect::<Vec<_>>();
    let a_out = alice.m().add_devices(&chat, &a_offer).unwrap();
    let b_out = bob.m().add_devices(&chat, &b_offer).unwrap();
    assert_eq!(a_out.epoch, b_out.epoch);

    // Alice is first; Bob's commit is refused and thrown away.
    assert!(commit(&mut net, &mut alice, &a_out));
    assert!(!commit(&mut net, &mut bob, &b_out));
    // Bob reads Alice's change, then tries again on the new epoch.
    assert!(
        matches!(&poll_ok(&mut net, &mut bob)[..], [Incoming::Changed { added, .. }] if added.len() == 1)
    );
    let b_offer = net
        .fetch("bob")
        .into_iter()
        .filter(|o| o.claimed_device == bob2.id())
        .collect::<Vec<_>>();
    let b_out = bob.m().add_devices(&chat, &b_offer).unwrap();
    assert!(commit(&mut net, &mut bob, &b_out));

    poll_ok(&mut net, &mut alice2);
    poll_ok(&mut net, &mut alice);
    poll_ok(&mut net, &mut bob2);
    poll_ok(&mut net, &mut alice2);
    // All four devices agree and read the same message.
    let epoch = alice.m().epoch(&chat).unwrap();
    for p in [&mut alice2, &mut bob, &mut bob2] {
        assert_eq!(p.m().epoch(&chat).unwrap(), epoch);
        assert_eq!(p.m().members(&chat).unwrap().len(), 4);
    }
    send(&mut net, &mut bob2, &chat, "alle da?");
    for p in [&mut alice, &mut alice2, &mut bob] {
        let got = poll_ok(&mut net, p);
        assert!(matches!(&got[..], [Incoming::Message(m)] if m.text == "alle da?"));
    }
    // Bob's first device shows what his second device sent as his own.
    assert!(
        bob.m()
            .messages(&chat, None, 10)
            .unwrap()
            .iter()
            .any(|m| m.text == "alle da?" && m.outgoing)
    );
}

#[test]
fn a_withdrawn_device_is_removed_and_reads_nothing_new() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut bob = person("bob");
    let mut laptop = second_device(&mut alice);
    publish(&mut net, &mut bob, 2);
    publish(&mut net, &mut laptop, 2);
    let chat = chat_between(&mut net, &mut alice, &mut bob);
    poll_ok(&mut net, &mut bob);
    poll_ok(&mut net, &mut laptop);
    // Alice's own laptop takes the chat as hers, not as a request.
    assert_eq!(laptop.m().chats().unwrap()[0].state, ChatState::Active);
    assert_eq!(
        laptop.m().contacts().unwrap()[0].fingerprint,
        bob.account.fingerprint()
    );

    // Alice withdraws the laptop (lost). Bob learns the new list.
    let laptop_id = laptop.identity.id.clone();
    let signed = SignedDeviceList {
        list: alice.list.clone(),
        signature: String::new(),
    }
    .without_device(&alice.account, &laptop_id)
    .unwrap();
    alice.list = signed.list.clone();
    alice.m().apply_device_list(&signed.list).unwrap();
    bob.m().apply_device_list(&signed.list).unwrap();
    let gone = format!("alice:{laptop_id}");
    assert_eq!(
        bob.m().withdrawn_members(&chat).unwrap(),
        std::slice::from_ref(&gone)
    );
    // Nothing reaches a chat that still holds it.
    assert!(matches!(
        bob.m().send(&chat, "x", NOW),
        Err(CoreError::Withdrawn(_))
    ));

    let out = bob
        .m()
        .remove_devices(&chat, std::slice::from_ref(&gone))
        .unwrap();
    assert!(
        !out.to.contains(&gone),
        "the server no longer knows a withdrawn device"
    );
    assert_eq!(out.removed, std::slice::from_ref(&gone));
    assert!(commit(&mut net, &mut bob, &out));
    // The real server deletes a withdrawn device's mail; this test hands
    // it the commit anyway, to see that it gives up the chat's keys.
    net.inject(&laptop.id(), &out.commit);
    let got = poll_ok(&mut net, &mut laptop);
    assert!(matches!(&got[..], [Incoming::Changed { left: true, .. }]));
    assert_eq!(laptop.m().chat(&chat).unwrap().state, ChatState::Left);
    // Its keys for the chat are deleted; the history stays.
    assert!(matches!(
        laptop.m().members(&chat),
        Err(CoreError::UnknownGroup(_))
    ));
    assert!(
        laptop.restart_ok(),
        "a left chat does not stop the database from opening"
    );
    poll_ok(&mut net, &mut alice);

    send(&mut net, &mut bob, &chat, "ohne Laptop");
    assert!(
        matches!(&poll_ok(&mut net, &mut alice)[..], [Incoming::Message(m)] if m.text == "ohne Laptop")
    );
    // The laptop still gets mail from this fake server, but cannot read it.
    let last = net.log.last().unwrap().clone();
    net.inject(&laptop.id(), &last);
    assert!(poll(&mut net, &mut laptop).iter().all(|r| r.is_err()));
    assert!(!texts(&mut laptop, &chat).contains(&"ohne Laptop".to_string()));
}

#[test]
fn a_message_delivered_twice_counts_once() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut bob = person("bob");
    publish(&mut net, &mut bob, 1);
    let chat = chat_between(&mut net, &mut alice, &mut bob);
    poll_ok(&mut net, &mut bob);
    let recipients = alice.m().recipients(&chat).unwrap();
    let (bytes, _) = alice.m().send(&chat, "einmal", NOW).unwrap();
    net.send(&alice.id(), &recipients, &bytes);
    net.inject(&bob.id(), &bytes);
    let got = poll_ok(&mut net, &mut bob);
    assert!(matches!(
        &got[..],
        [Incoming::Message(_), Incoming::Duplicate]
    ));
    // Also after a restart (the server may resend what was not confirmed).
    bob.restart();
    assert_eq!(bob.m().receive(&bytes, NOW).unwrap(), Incoming::Duplicate);
    assert_eq!(texts(&mut bob, &chat), ["einmal"]);
}

#[test]
fn a_restart_between_commit_and_answer_keeps_the_commit() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut bob = person("bob");
    publish(&mut net, &mut bob, 1);
    let b_code = bob.m().contact_code();
    alice.m().add_contact(&b_code, "Bob", NOW).unwrap();
    let a_code = alice.m().contact_code();
    bob.m().add_contact(&a_code, "Alice", NOW).unwrap();
    let offers = net.fetch("bob");
    let (chat, out) = alice.m().start_chat("bob", &offers, NOW).unwrap();
    // Trair is closed before the server answered.
    alice.restart();
    assert!(commit(&mut net, &mut alice, &out));
    poll_ok(&mut net, &mut bob);
    send(&mut net, &mut alice, &chat.id, "angekommen");
    assert!(
        matches!(&poll_ok(&mut net, &mut bob)[..], [Incoming::Message(m)] if m.text == "angekommen")
    );
}

#[test]
fn only_devices_of_the_two_accounts_get_into_a_chat() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut bob = person("bob");
    let mut carol = person("carol");
    publish(&mut net, &mut bob, 2);
    publish(&mut net, &mut carol, 2);
    let b_code = bob.m().contact_code();
    let c_code = carol.m().contact_code();
    alice.m().add_contact(&b_code, "Bob", NOW).unwrap();
    alice.m().add_contact(&c_code, "Carol", NOW).unwrap();
    // Carol is a contact too, but she has no place in the chat with Bob.
    let mut offers = net.fetch("bob");
    offers.extend(net.fetch("carol"));
    let refused = alice.m().start_chat("bob", &offers, NOW);
    assert!(
        matches!(refused, Err(CoreError::InvalidKeyPackage(_))),
        "{refused:?}"
    );
    let offers = net.fetch("bob");
    let (chat, out) = alice.m().start_chat("bob", &offers, NOW).unwrap();
    assert!(commit(&mut net, &mut alice, &out));
    let carol_offer = net.fetch("carol");
    let later = alice.m().add_devices(&chat.id, &carol_offer);
    assert!(
        matches!(later, Err(CoreError::InvalidKeyPackage(_))),
        "{later:?}"
    );
    // A chat needs at least one device of the other person.
    assert!(alice.m().start_chat("bob", &[], NOW).is_err());
    assert_eq!(alice.m().members(&chat.id).unwrap().len(), 2);
}

#[test]
fn a_change_the_server_took_before_a_crash_is_recognised() {
    let mut net = Net::default();
    let mut alice = person("alice");
    let mut bob = person("bob");
    publish(&mut net, &mut bob, 2);
    let chat = chat_between(&mut net, &mut alice, &mut bob);
    poll_ok(&mut net, &mut bob);
    let mut bob2 = second_device(&mut bob);
    publish(&mut net, &mut bob2, 1);
    let offer: Vec<_> = net
        .fetch("bob")
        .into_iter()
        .filter(|o| o.claimed_device == bob2.id())
        .collect();
    let out = bob.m().add_devices(&chat, &offer).unwrap();
    // Only one change at a time: refused before MLS is touched.
    assert!(matches!(
        bob.m().add_devices(&chat, &offer),
        Err(CoreError::Rejected(_))
    ));
    assert!(matches!(
        bob.m().update_keys(&chat),
        Err(CoreError::Rejected(_))
    ));
    // The server takes it, but Trair dies before it hears the answer.
    let bob_id = bob.id();
    net.commit_raw(
        &bob_id,
        &out.chat,
        out.epoch,
        &out.commit,
        out.welcome.as_deref(),
        &out.welcome_to,
        &out.to,
    )
    .unwrap();
    bob.restart();
    let pending = bob.m().pending_commits().unwrap();
    assert_eq!(
        pending,
        std::slice::from_ref(&out),
        "the change is still on disk"
    );
    // Sent again, the server refuses (epoch moved) but names this very
    // commit: Bob applies it instead of throwing it away.
    let again = net.commit_raw(
        &bob_id,
        &out.chat,
        out.epoch,
        &out.commit,
        out.welcome.as_deref(),
        &out.welcome_to,
        &out.to,
    );
    assert!(again.is_err());
    let taken = net.taken(&chat);
    assert!(
        bob.m()
            .commit_answer(&chat, false, taken.as_deref(), NOW)
            .unwrap()
    );
    assert!(bob.m().pending_commits().unwrap().is_empty());
    poll_ok(&mut net, &mut alice);
    poll_ok(&mut net, &mut bob2);
    assert_eq!(
        bob.m().epoch(&chat).unwrap(),
        alice.m().epoch(&chat).unwrap()
    );
    send(&mut net, &mut alice, &chat, "an beide Geräte");
    for p in [&mut bob, &mut bob2] {
        assert!(
            matches!(&poll_ok(&mut net, p)[..], [Incoming::Message(m)] if m.text == "an beide Geräte")
        );
    }
    // A foreign hash does not count as accepted.
    let out = alice.m().update_keys(&chat).unwrap();
    assert!(
        !alice
            .m()
            .commit_answer(&chat, false, Some(&"0".repeat(64)), NOW)
            .unwrap()
    );
    assert_ne!(out.hash(), "0".repeat(64));
    assert!(alice.m().pending_commits().unwrap().is_empty());
}
