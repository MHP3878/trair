// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! M5: group chats with the real core. Every person has their own chat
//! database; the in-memory server from `common` orders commits like the
//! real one and, like it, takes changes of a group only from the accounts
//! the group named (owner and admins).

mod common;

use std::path::PathBuf;
use std::rc::Rc;

use common::Net;
use serde_json::Value;
use trair_core::chats::CHATS_FILE;
use trair_core::devices::DeviceList;
use trair_core::{
    Account, ChatKind, ChatState, CoreError, DeviceIdentity, Event, GroupState, Incoming,
    Messenger, PendingCommit, Role, SignedDeviceList, Tidy,
};

const NOW: u64 = 1_800_000_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).unwrap();
        let p = std::env::temp_dir().join(format!("trair-groups-{}", hex::encode(b)));
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

fn publish(net: &mut Net, p: &mut Phone, n: usize) {
    let id = p.id();
    let kps = p.m().key_packages(n).unwrap();
    let lr = p.m().last_resort_key_package().unwrap();
    net.register_packages(&id, kps, lr);
}

/// Hand a change to the server like the app: with the accounts that may
/// change the group, then tell the core the answer.
fn commit_at(net: &mut Net, p: &mut Phone, out: &PendingCommit, now: u64) -> bool {
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
        .commit_answer(&out.chat, ok, taken.as_deref(), now)
        .unwrap()
}

fn commit(net: &mut Net, p: &mut Phone, out: &PendingCommit) -> bool {
    commit_at(net, p, out, NOW)
}

fn poll_at(net: &mut Net, p: &mut Phone, now: u64) -> Vec<Result<Incoming, CoreError>> {
    let id = p.id();
    net.inbox(&id)
        .iter()
        .map(|b| p.m().receive(b, now))
        .collect()
}

fn poll_ok(net: &mut Net, p: &mut Phone) -> Vec<Incoming> {
    poll_at(net, p, NOW)
        .into_iter()
        .map(|r| r.unwrap_or_else(|e| panic!("could not process a message: {e}")))
        .collect()
}

fn send_at(net: &mut Net, p: &mut Phone, chat: &str, text: &str, now: u64) {
    let to = p.m().recipients(chat).unwrap();
    let (bytes, _) = p.m().send(chat, text, now).unwrap();
    let id = p.id();
    net.send(&id, &to, &bytes);
}

fn send(net: &mut Net, p: &mut Phone, chat: &str, text: &str) {
    send_at(net, p, chat, text, NOW);
}

fn out(net: &mut Net, p: &mut Phone, (bytes, to): (Vec<u8>, Vec<String>)) {
    let id = p.id();
    net.send(&id, &to, &bytes);
}

/// Texts written by people (not notes).
fn texts(p: &mut Phone, chat: &str) -> Vec<String> {
    p.m()
        .messages(chat, None, 200)
        .unwrap()
        .into_iter()
        .filter(|m| !m.user.is_empty())
        .map(|m| m.text)
        .collect()
}

/// Notes in the history: `event:user,user` (or `event:name`).
fn notes(p: &mut Phone, chat: &str) -> Vec<String> {
    p.m()
        .messages(chat, None, 200)
        .unwrap()
        .into_iter()
        .filter(|m| m.user.is_empty())
        .map(|m| {
            let v: Value = serde_json::from_str(&m.text).unwrap();
            let what = v["event"].as_str().unwrap().to_string();
            let who = match &v["users"] {
                Value::Array(a) => a
                    .iter()
                    .map(|u| u.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(","),
                _ => v["name"].as_str().unwrap_or_default().to_string(),
            };
            format!("{what}:{who}")
        })
        .collect()
}

fn befriend(a: &mut Phone, b: &mut Phone) {
    let ac = a.m().contact_code();
    let bc = b.m().contact_code();
    let bn = b.user();
    let an = a.user();
    a.m().add_contact(&bc, &bn, NOW).unwrap();
    b.m().add_contact(&ac, &an, NOW).unwrap();
}

/// Olga starts a group with her contacts; everyone takes the welcome.
fn group_of(net: &mut Net, owner: &mut Phone, name: &str, others: &mut [&mut Phone]) -> String {
    let mut offers = Vec::new();
    for o in others.iter_mut() {
        befriend(owner, o);
        publish(net, o, 3);
        offers.extend(net.fetch(&o.user()));
    }
    let (chat, pending) = owner.m().create_group(name, None, &offers, NOW).unwrap();
    assert!(commit(net, owner, &pending.unwrap()));
    for o in others.iter_mut() {
        let got = poll_ok(net, o);
        assert!(
            matches!(&got[..], [Incoming::Chat { chat: c, .. }] if c.id == chat.id && c.kind == ChatKind::Group && c.state == ChatState::Active),
            "{got:?}"
        );
    }
    chat.id
}

fn role_of(p: &mut Phone, chat: &str, user: &str) -> Role {
    p.m()
        .group_view(chat)
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.user == user)
        .unwrap_or_else(|| panic!("{user} is not in the group"))
        .role
}

fn users(p: &mut Phone, chat: &str) -> Vec<String> {
    p.m()
        .group_view(chat)
        .unwrap()
        .members
        .into_iter()
        .map(|m| m.user)
        .collect()
}

/// Someone uses a group key; the admin's device checks it and adds them.
/// Returns what the admin's device saw.
fn join_with(
    net: &mut Net,
    admin: &mut Phone,
    joiner: &mut Phone,
    code: &str,
    admin_now: u64,
) -> Vec<Incoming> {
    publish(net, admin, 2);
    publish(net, joiner, 2);
    let offers = net.fetch(&admin.user());
    let (jc, pending) = joiner.m().join_group(code, &offers, NOW).unwrap();
    assert_eq!(jc.kind, ChatKind::Join);
    assert!(commit(net, joiner, &pending));
    let msg = joiner.m().join_message(&jc.id).unwrap().unwrap();
    out(net, joiner, msg);
    // The token goes out once.
    assert!(joiner.m().join_message(&jc.id).unwrap().is_none());
    poll_at(net, admin, admin_now)
        .into_iter()
        .map(|r| r.unwrap())
        .collect()
}

fn admit_all(net: &mut Net, admin: &mut Phone, seen: &[Incoming]) {
    for inc in seen {
        if let Incoming::JoinRequest { chat, user, .. } = inc {
            let offers = net.fetch(user);
            let pending = admin.m().admit(chat, &offers).unwrap();
            assert!(commit(net, admin, &pending));
            admin.m().close_join(chat).unwrap();
        }
    }
}

#[test]
fn g01_a_group_of_three_writes_reads_and_survives_a_restart() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    let chat = group_of(&mut net, &mut olga, "Abi 2026", &mut [&mut ada, &mut max]);
    // Ada and Max never met: the admin vouches, strict mode lets them talk.
    send(&mut net, &mut ada, &chat, "Hallo zusammen");
    send(&mut net, &mut max, &chat, "Servus");
    poll_ok(&mut net, &mut olga);
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);
    assert_eq!(texts(&mut olga, &chat), ["Hallo zusammen", "Servus"]);
    let from_max = olga.m().messages(&chat, None, 10).unwrap();
    assert!(
        from_max
            .iter()
            .any(|m| m.user == "max" && m.text == "Servus")
    );

    for p in [&mut olga, &mut ada, &mut max] {
        let v = p.m().group_view(&chat).unwrap();
        assert_eq!(v.name, "Abi 2026");
        assert_eq!(
            v.members
                .iter()
                .map(|m| (m.user.as_str(), m.role))
                .collect::<Vec<_>>(),
            [
                ("olga", Role::Owner),
                ("ada", Role::Member),
                ("max", Role::Member)
            ]
        );
    }
    assert_eq!(notes(&mut ada, &chat), ["joined:"]);
    assert_eq!(notes(&mut olga, &chat), ["created:", "added:ada,max"]);
    // The server lets only Olga change the group.
    assert_eq!(net.committers_of(&chat), Some(vec!["olga".to_string()]));

    olga.restart();
    ada.restart();
    send(&mut net, &mut olga, &chat, "nach dem Neustart");
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);
    assert_eq!(
        texts(&mut max, &chat),
        ["Servus", "Hallo zusammen", "nach dem Neustart"]
    );
    assert_eq!(role_of(&mut ada, &chat, "olga"), Role::Owner);
}

#[test]
fn g02_only_admins_change_a_group_and_the_server_refuses_members() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    let chat = group_of(&mut net, &mut olga, "Team", &mut [&mut ada, &mut max]);
    // Max's own Trair refuses before anything leaves.
    for r in [
        max.m().kick(&chat, "ada").map(|_| ()),
        max.m().rename_group(&chat, "Meins").map(|_| ()),
        max.m().set_admin(&chat, "max", true).map(|_| ()),
        max.m().close_group(&chat).map(|_| ()),
        max.m()
            .set_group_end(&chat, Some(NOW + 60), NOW)
            .map(|_| ()),
        max.m()
            .create_group_invite(&chat, false, NOW + 60, NOW)
            .map(|_| ()),
    ] {
        assert!(matches!(r, Err(CoreError::NotAllowed(_))), "{r:?}");
    }
    // A modified Trair would still meet the server: it takes changes of
    // this group only from Olga.
    let epoch = max.m().epoch(&chat).unwrap();
    let refused = net.commit_as(&max.id(), &chat, epoch, b"anything", None, &[], &[], None);
    assert!(refused.unwrap_err().starts_with("403"));
    // Nothing moved: everyone still talks.
    send(&mut net, &mut max, &chat, "noch da");
    assert_eq!(poll_ok(&mut net, &mut olga).len(), 1);
    assert_eq!(texts(&mut olga, &chat), ["noch da"]);
}

#[test]
fn g03_a_forbidden_change_is_not_followed() {
    // Device level, with a server that takes anything: Max runs a
    // modified Trair that removes Ada with plain MLS.
    let mut net = Net::default();
    let (_, mut olga) = common::person("olga", "pc");
    let (_, mut ada) = common::person("ada", "pc");
    let (_, mut max) = common::person("max", "pc");
    let (_, mut eve) = common::person("eve", "pc");
    net.register(&mut ada, 2);
    net.register(&mut max, 2);
    net.register(&mut eve, 2);
    let state = GroupState::new_group("Abi", olga.acct(), None).unwrap();
    olga.create_trair_group("g3", &state).unwrap();
    let offers = [net.fetch("ada"), net.fetch("max")].concat();
    let allow = [ada.acct(), max.acct()];
    let o = olga.change("g3", &offers, &[], None, &allow).unwrap();
    net.commit(olga.id(), &o, &[]).unwrap();
    olga.confirm("g3").unwrap();
    for d in [&mut ada, &mut max] {
        let ev = common::poll_ok(&mut net, d);
        assert!(matches!(&ev[0], Event::Joined { state: Some(s), .. } if s.name == "Abi"));
    }
    let to = max.recipients("g3").unwrap();
    let o = max.remove_user("g3", "ada").unwrap();
    net.commit(max.id(), &o, &to).unwrap();
    max.confirm("g3").unwrap();
    for d in [&mut olga, &mut ada] {
        let r = common::poll(&mut net, d);
        assert!(matches!(&r[..], [Err(CoreError::NotAllowed(_))]), "{r:?}");
    }
    // Olga and Ada stay together; only Max is on his own now.
    common::send(&mut net, &mut olga, "g3", "ohne Max").unwrap();
    assert!(
        matches!(&common::poll_ok(&mut net, &mut ada)[..], [Event::Message { text, .. }] if text == "ohne Max")
    );
    assert!(common::poll(&mut net, &mut max)[0].is_err());

    // A welcome from someone who is no admin is refused as well (Max's
    // modified Trair ignores strict mode, too).
    max.set_strict(false);
    let o = max.add_members("g3", &net.fetch("eve")).unwrap();
    net.commit(max.id(), &o, &[]).unwrap();
    let r = common::poll(&mut net, &mut eve);
    assert!(matches!(&r[..], [Err(CoreError::NotAllowed(_))]), "{r:?}");
}

#[test]
fn g04_the_owner_names_admins_admins_remove_members_but_not_the_owner() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    let chat = group_of(&mut net, &mut olga, "Klasse", &mut [&mut ada, &mut max]);
    let p = olga.m().set_admin(&chat, "ada", true).unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    assert_eq!(
        p.committers.as_deref(),
        Some(&["ada".to_string(), "olga".to_string()][..])
    );
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);
    assert_eq!(role_of(&mut max, &chat, "ada"), Role::Admin);
    // Ada may not touch the owner or name admins.
    assert!(matches!(
        ada.m().kick(&chat, "olga"),
        Err(CoreError::NotAllowed(_))
    ));
    assert!(matches!(
        ada.m().set_admin(&chat, "max", true),
        Err(CoreError::NotAllowed(_))
    ));
    // Ada removes Max.
    let p = ada.m().kick(&chat, "max").unwrap();
    assert!(commit(&mut net, &mut ada, &p));
    let got = poll_ok(&mut net, &mut max);
    assert!(
        matches!(&got[..], [Incoming::Changed { left: true, .. }]),
        "{got:?}"
    );
    assert_eq!(max.m().chat(&chat).unwrap().state, ChatState::Left);
    poll_ok(&mut net, &mut olga);
    assert_eq!(users(&mut olga, &chat), ["olga", "ada"]);
    assert_eq!(
        notes(&mut olga, &chat),
        ["created:", "added:ada,max", "admin:ada", "removed:max"]
    );
    assert_eq!(
        notes(&mut max, &chat),
        ["joined:", "admin:ada", "removed:max"]
    );
    // What is written now does not reach Max.
    send(&mut net, &mut olga, &chat, "ohne Max");
    assert!(poll_at(&mut net, &mut max, NOW).iter().all(|r| r.is_err()));
    assert_eq!(poll_ok(&mut net, &mut ada).len(), 1);
    // A second change may not wait while one is out.
    let p = olga.m().rename_group(&chat, "Klasse 10b").unwrap();
    assert!(olga.m().rename_group(&chat, "x").is_err());
    assert!(commit(&mut net, &mut olga, &p));
    poll_ok(&mut net, &mut ada);
    assert_eq!(ada.m().group_view(&chat).unwrap().name, "Klasse 10b");
    // Two admins at once: the server takes one, the other tries again.
    let a = ada.m().rename_group(&chat, "von Ada").unwrap();
    let o = olga.m().rename_group(&chat, "von Olga").unwrap();
    assert!(commit(&mut net, &mut ada, &a));
    assert!(!commit(&mut net, &mut olga, &o));
    poll_ok(&mut net, &mut olga);
    assert_eq!(olga.m().group_view(&chat).unwrap().name, "von Ada");
}

#[test]
fn g05_a_group_key_lets_someone_join_once_an_admin_checks_it() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut gus = person("gus");
    let chat = group_of(&mut net, &mut olga, "Party", &mut [&mut ada]);
    let code = olga
        .m()
        .create_group_invite(&chat, false, NOW + 3600, NOW)
        .unwrap();
    assert!(code.starts_with("trair3:"));
    // A group key is no friendship key.
    assert!(gus.m().add_contact(&code, "x", NOW).is_err());
    let seen = join_with(&mut net, &mut olga, &mut gus, &code, NOW);
    assert!(
        seen.iter()
            .any(|i| matches!(i, Incoming::JoinRequest { user, group, .. } if user == "gus" && *group == chat)),
        "{seen:?}"
    );
    // The admin's side of the join chat is not shown.
    assert!(
        olga.m()
            .chats()
            .unwrap()
            .iter()
            .all(|c| c.kind != ChatKind::Admit)
    );
    assert_eq!(olga.m().join_requests().unwrap().len(), 1);
    admit_all(&mut net, &mut olga, &seen);
    assert!(olga.m().join_requests().unwrap().is_empty());
    let got = poll_ok(&mut net, &mut gus);
    assert!(
        got.iter().any(|i| matches!(i, Incoming::Chat { chat: c, .. } if c.id == chat && c.state == ChatState::Active)),
        "{got:?}"
    );
    // The join chat is gone; Gus is a member (not a contact of anyone).
    let kinds: Vec<ChatKind> = gus.m().chats().unwrap().iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [ChatKind::Group]);
    assert!(olga.m().contact("gus").unwrap().is_none());
    poll_ok(&mut net, &mut ada);
    assert_eq!(users(&mut ada, &chat), ["olga", "ada", "gus"]);
    send(&mut net, &mut gus, &chat, "Danke!");
    poll_ok(&mut net, &mut ada);
    assert_eq!(texts(&mut ada, &chat), ["Danke!"]);

    // The key works more than once.
    let mut ida = person("ida");
    let seen = join_with(&mut net, &mut olga, &mut ida, &code, NOW);
    admit_all(&mut net, &mut olga, &seen);
    poll_ok(&mut net, &mut ida);
    assert_eq!(ida.m().group_view(&chat).unwrap().members.len(), 4);

    // Too late: Olga's device refuses, and Eve learns it.
    let mut eve = person("eve");
    let late = olga
        .m()
        .create_group_invite(&chat, false, NOW + 60, NOW)
        .unwrap();
    let seen = join_with(&mut net, &mut olga, &mut eve, &late, NOW + 120);
    let refused: Vec<String> = seen
        .iter()
        .filter_map(|i| match i {
            Incoming::JoinRefused { chat } => Some(chat.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 1, "{seen:?}");
    let msg = olga.m().deny_join(&refused[0]).unwrap();
    out(&mut net, &mut olga, msg);
    let got = poll_ok(&mut net, &mut eve);
    assert!(matches!(&got[..], [Incoming::JoinFailed { .. }]), "{got:?}");
    assert!(eve.m().chats().unwrap().is_empty());

    // Taken back: refused as well.
    assert_eq!(olga.m().revoke_group_invites(&chat).unwrap(), 2);
    let mut fay = person("fay");
    let seen = join_with(&mut net, &mut olga, &mut fay, &code, NOW);
    assert!(
        seen.iter()
            .any(|i| matches!(i, Incoming::JoinRefused { .. }))
    );
    assert_eq!(olga.m().group_invites(&chat, NOW).unwrap().0, 0);

    // A key Olga never made gets no answer at all, and no request.
    let mut zed = person("zed");
    let parts: Vec<&str> = code.split(':').collect();
    let forged_token = "00".repeat(16);
    let forged = trair_core::InviteCode {
        contact: trair_core::ContactCode::parse(&olga.m().contact_code()).unwrap(),
        token: [0; 16],
        kind: trair_core::InviteKind::Member,
        ends: parts[4][1..].parse().unwrap(),
    }
    .text();
    assert!(forged.contains(&forged_token));
    let seen = join_with(&mut net, &mut olga, &mut zed, &forged, NOW);
    assert!(
        seen.iter()
            .all(|i| matches!(i, Incoming::Other | Incoming::Duplicate)),
        "{seen:?}"
    );
    // Its join chat vanishes on both sides after a while.
    olga.m()
        .sweep(NOW + trair_core::group_chats::JOIN_WAIT_SECS + 1)
        .unwrap();
    assert!(olga.m().join_requests().unwrap().is_empty());
}

#[test]
fn g06_guests_leave_again_at_the_end() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut gus = person("gus");
    let chat = group_of(&mut net, &mut olga, "Grillen", &mut [&mut ada]);
    let until = NOW + 3600;
    let code = olga
        .m()
        .create_group_invite(&chat, true, until, NOW)
        .unwrap();
    let seen = join_with(&mut net, &mut olga, &mut gus, &code, NOW);
    admit_all(&mut net, &mut olga, &seen);
    poll_ok(&mut net, &mut gus);
    poll_ok(&mut net, &mut ada);
    let v = ada.m().group_view(&chat).unwrap();
    let g = v.members.iter().find(|m| m.user == "gus").unwrap();
    assert_eq!((g.role, g.guest_until), (Role::Guest, Some(until)));
    assert_eq!(gus.m().group_view(&chat).unwrap().my_end, Some(until));
    assert_eq!(gus.m().chat_end(&chat).unwrap(), Some(until));
    send(&mut net, &mut gus, &chat, "bis später");
    poll_ok(&mut net, &mut ada);
    // After the end, what a guest still sends is not shown.
    send_at(&mut net, &mut gus, &chat, "immer noch da", until + 1);
    let got = poll_at(&mut net, &mut ada, until + 1);
    assert!(matches!(&got[..], [Ok(Incoming::Other)]), "{got:?}");
    assert_eq!(texts(&mut ada, &chat), ["bis später"]);
    // Gus's own device deletes the group.
    assert_eq!(
        gus.m().sweep(until + 1).unwrap(),
        std::slice::from_ref(&chat)
    );
    assert!(gus.m().chats().unwrap().is_empty());
    // An admin's device removes him for good; a guest is no admin.
    poll_at(&mut net, &mut olga, until + 1);
    let Tidy::Commit(p) = olga.m().tidy_group(&chat, until + 1).unwrap() else {
        panic!("nothing to tidy")
    };
    assert!(commit_at(&mut net, &mut olga, &p, until + 1));
    poll_at(&mut net, &mut ada, until + 1);
    assert_eq!(users(&mut ada, &chat), ["olga", "ada"]);
    assert_eq!(
        olga.m().tidy_group(&chat, until + 2).unwrap(),
        Tidy::Nothing
    );
}

#[test]
fn g07_leaving_and_handing_on() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    let chat = group_of(&mut net, &mut olga, "Band", &mut [&mut ada, &mut max]);
    let p = olga.m().set_admin(&chat, "ada", true).unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);

    // Max leaves: gone on his side at once.
    assert!(max.m().leave_prepare(&chat).unwrap().is_none());
    let msg = max.m().leave_group(&chat, NOW).unwrap();
    out(&mut net, &mut max, msg);
    assert_eq!(max.m().chat(&chat).unwrap().state, ChatState::Left);
    let got = poll_ok(&mut net, &mut ada);
    assert!(matches!(&got[..], [Incoming::Group { .. }]), "{got:?}");
    assert!(
        ada.m()
            .group_view(&chat)
            .unwrap()
            .members
            .iter()
            .any(|m| m.user == "max" && m.leaving)
    );
    // Ada's device removes his devices for good.
    let Tidy::Commit(p) = ada.m().tidy_group(&chat, NOW).unwrap() else {
        panic!("Ada has nothing to tidy")
    };
    assert!(commit(&mut net, &mut ada, &p));
    poll_ok(&mut net, &mut olga);
    assert_eq!(users(&mut olga, &chat), ["olga", "ada"]);

    // Olga cannot just walk off: she hands the group on first.
    assert!(matches!(
        olga.m().leave_group(&chat, NOW),
        Err(CoreError::NotAllowed(_))
    ));
    let p = olga.m().leave_prepare(&chat).unwrap().unwrap();
    assert_eq!(p.committers.as_deref(), Some(&["ada".to_string()][..]));
    assert!(commit(&mut net, &mut olga, &p));
    let msg = olga.m().leave_group(&chat, NOW).unwrap();
    out(&mut net, &mut olga, msg);
    poll_ok(&mut net, &mut ada);
    assert_eq!(ada.m().group_view(&chat).unwrap().me, Role::Owner);
    let Tidy::Commit(p) = ada.m().tidy_group(&chat, NOW).unwrap() else {
        panic!("nothing to tidy")
    };
    assert!(commit(&mut net, &mut ada, &p));
    assert_eq!(users(&mut ada, &chat), ["ada"]);
    // The last one leaves without handing on.
    assert!(ada.m().leave_prepare(&chat).unwrap().is_none());
    ada.m().leave_group(&chat, NOW).unwrap();
    ada.m().delete_chat(&chat).unwrap();
    assert!(ada.m().chats().unwrap().is_empty());
}

#[test]
fn g08_a_group_vanishes_for_everyone_and_closing_deletes_it() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    let chat = group_of(&mut net, &mut olga, "Eintag", &mut [&mut ada, &mut max]);
    let p = olga.m().set_group_end(&chat, Some(NOW + 600), NOW).unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);
    assert_eq!(ada.m().chat_end(&chat).unwrap(), Some(NOW + 600));
    assert!(notes(&mut ada, &chat).contains(&"ends:".to_string()));
    // Out of range is refused.
    assert!(olga.m().set_group_end(&chat, Some(NOW - 1), NOW).is_err());
    for p in [&mut olga, &mut ada, &mut max] {
        assert!(p.m().sweep(NOW + 599).unwrap().is_empty());
        assert_eq!(p.m().sweep(NOW + 601).unwrap(), std::slice::from_ref(&chat));
        assert!(p.m().chats().unwrap().is_empty());
    }

    // Closing: gone on every device as soon as the change arrives.
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let chat = group_of(&mut net, &mut olga, "Kurz", &mut [&mut ada]);
    assert!(matches!(
        ada.m().close_group(&chat),
        Err(CoreError::NotAllowed(_))
    ));
    let p = olga.m().close_group(&chat).unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    assert!(olga.m().chats().unwrap().is_empty());
    poll_ok(&mut net, &mut ada);
    assert!(ada.m().chats().unwrap().is_empty());
}

#[test]
fn g09_strict_mode_notices_a_changed_key_in_a_group() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut max = person("max");
    let chat = group_of(&mut net, &mut olga, "Verein", &mut [&mut max]);
    let code = olga
        .m()
        .create_group_invite(&chat, false, NOW + 3600, NOW)
        .unwrap();
    let mut ada = person("ada");
    let seen = join_with(&mut net, &mut olga, &mut ada, &code, NOW);
    admit_all(&mut net, &mut olga, &seen);
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);
    let p = olga.m().kick(&chat, "ada").unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    poll_ok(&mut net, &mut max);
    poll_ok(&mut net, &mut ada);
    // Someone else under the name "ada" (a new key) joins the group; the
    // old account is gone from the server.
    net.unregister(&ada.id());
    let mut other = person("ada");
    let seen = join_with(&mut net, &mut olga, &mut other, &code, NOW);
    admit_all(&mut net, &mut olga, &seen);
    poll_ok(&mut net, &mut other);
    poll_ok(&mut net, &mut max);
    let v = max.m().group_view(&chat).unwrap();
    assert!(v.members.iter().any(|m| m.user == "ada" && m.key_changed));
    let r = max.m().send(&chat, "hallo?", NOW);
    assert!(matches!(r, Err(CoreError::Blocked(_))), "{r:?}");
    // After checking, Max takes the new key.
    max.m().accept_new_key(&chat, "ada").unwrap();
    send(&mut net, &mut max, &chat, "hallo neue Ada");
    poll_ok(&mut net, &mut other);
    assert_eq!(texts(&mut other, &chat), ["hallo neue Ada"]);
    // Contacts keep their key: Olga cannot add a contact whose key was
    // swapped on the way.
    let mut bob = person("bob");
    befriend(&mut olga, &mut bob);
    let mut fake = person("bob");
    publish(&mut net, &mut fake, 1);
    let offers = net.fetch("bob");
    let r = olga.m().add_to_group(&chat, &offers);
    assert!(matches!(r, Err(CoreError::Blocked(_))), "{r:?}");
}

#[test]
fn g10_the_server_learns_no_names_and_no_texts() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let chat = group_of(&mut net, &mut olga, "Geheimer Kreis", &mut [&mut ada]);
    let p = olga.m().set_admin(&chat, "ada", true).unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    let p = olga.m().rename_group(&chat, "Noch geheimer").unwrap();
    assert!(commit(&mut net, &mut olga, &p));
    send(&mut net, &mut olga, &chat, "Treffpunkt Bahnhof");
    poll_ok(&mut net, &mut ada);
    for needle in [
        "Geheimer Kreis".as_bytes(),
        b"Noch geheimer",
        b"Treffpunkt",
        b"admins",
        b"owner",
    ] {
        assert!(
            !net.log
                .iter()
                .any(|m| m.windows(needle.len()).any(|w| w == needle)),
            "{} reached the server",
            String::from_utf8_lossy(needle)
        );
    }
    // A member's database is encrypted too.
    ada.m = None;
    let file = std::fs::read(&ada.path).unwrap();
    assert!(!file.windows(14).any(|w| w == b"Geheimer Kreis"));
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
    let dir = TempDir::new();
    let path = dir.0.join(CHATS_FILE);
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).unwrap();
    let m = Messenger::open(&path, &key, &first.account, &identity, &signed.list, NOW).unwrap();
    Phone {
        _dir: dir,
        path,
        key,
        account: Rc::clone(&first.account),
        identity,
        list: signed.list,
        m: Some(m),
    }
}

#[test]
fn g11_a_member_who_joined_through_the_reserve_package_gets_new_keys() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    befriend(&mut olga, &mut ada);
    befriend(&mut olga, &mut max);
    publish(&mut net, &mut ada, 2);
    // Max has only his last-resort package left on the server.
    publish(&mut net, &mut max, 0);
    let offers = [net.fetch("ada"), net.fetch("max")].concat();
    let (chat, p) = olga
        .m()
        .create_group("Reserve", None, &offers, NOW)
        .unwrap();
    assert!(commit(&mut net, &mut olga, &p.unwrap()));
    poll_ok(&mut net, &mut ada);
    let got = poll_ok(&mut net, &mut max);
    assert!(
        matches!(
            &got[..],
            [Incoming::Chat {
                update_keys: true,
                ..
            }]
        ),
        "{got:?}"
    );
    // Max may not change the group himself: he asks, Olga's change
    // carries it.
    assert!(matches!(
        max.m().update_keys(&chat.id),
        Err(CoreError::NotAllowed(_))
    ));
    let ask = max.m().propose_update(&chat.id).unwrap();
    out(&mut net, &mut max, ask);
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut olga);
    let Tidy::Commit(p) = olga.m().tidy_group(&chat.id, NOW).unwrap() else {
        panic!("the update is not carried")
    };
    assert!(p.welcome.is_none() && p.removed.is_empty());
    assert!(commit(&mut net, &mut olga, &p));
    poll_ok(&mut net, &mut max);
    poll_ok(&mut net, &mut ada);
    assert_eq!(olga.m().tidy_group(&chat.id, NOW).unwrap(), Tidy::Nothing);
    send(&mut net, &mut max, &chat.id, "mit neuen Schlüsseln");
    poll_ok(&mut net, &mut ada);
    assert_eq!(texts(&mut ada, &chat.id), ["mit neuen Schlüsseln"]);
}

#[test]
fn g12_a_member_asks_the_admins_to_remove_its_withdrawn_device() {
    let mut net = Net::default();
    let mut olga = person("olga");
    let mut ada = person("ada");
    let mut max = person("max");
    let mut laptop = second_device(&mut max);
    befriend(&mut olga, &mut ada);
    befriend(&mut olga, &mut max);
    publish(&mut net, &mut ada, 2);
    publish(&mut net, &mut max, 2);
    publish(&mut net, &mut laptop, 2);
    let offers = [net.fetch("ada"), net.fetch("max")].concat();
    let (chat, p) = olga.m().create_group("Geräte", None, &offers, NOW).unwrap();
    let chat = chat.id;
    assert!(commit(&mut net, &mut olga, &p.unwrap()));
    poll_ok(&mut net, &mut ada);
    poll_ok(&mut net, &mut max);
    // The laptop does not know Olga: for it the group is a request.
    poll_ok(&mut net, &mut laptop);
    assert_eq!(laptop.m().chat(&chat).unwrap().state, ChatState::Request);
    laptop.m().accept_group(&chat).unwrap();
    assert_eq!(users(&mut laptop, &chat), ["olga", "ada", "max"]);
    assert_eq!(
        ada.m().group_view(&chat).unwrap().members[2].devices.len(),
        2
    );

    // Max withdraws the laptop. His phone may not change the group: it
    // asks, and Olga's device removes the laptop.
    let signed = SignedDeviceList {
        list: max.list.clone(),
        signature: String::new(),
    }
    .without_device(&max.account, &laptop.identity.id)
    .unwrap();
    max.list = signed.list.clone();
    max.m().apply_device_list(&signed.list).unwrap();
    assert!(matches!(
        max.m().send(&chat, "x", NOW),
        Err(CoreError::Withdrawn(_))
    ));
    let Tidy::Message { bytes, to } = max.m().tidy_group(&chat, NOW).unwrap() else {
        panic!("Max does not ask")
    };
    out(&mut net, &mut max, (bytes, to));
    // Asked once is enough.
    assert_eq!(max.m().tidy_group(&chat, NOW).unwrap(), Tidy::Nothing);
    poll_ok(&mut net, &mut ada);
    let got = poll_ok(&mut net, &mut olga);
    assert!(matches!(&got[..], [Incoming::Group { .. }]), "{got:?}");
    let Tidy::Commit(p) = olga.m().tidy_group(&chat, NOW).unwrap() else {
        panic!("Olga removes nothing")
    };
    assert_eq!(p.removed, [format!("max:{}", laptop.identity.id)]);
    assert!(commit(&mut net, &mut olga, &p));
    poll_ok(&mut net, &mut max);
    poll_ok(&mut net, &mut ada);
    assert_eq!(
        ada.m().group_view(&chat).unwrap().members[2].devices.len(),
        1
    );
    send(&mut net, &mut max, &chat, "wieder da");
    poll_ok(&mut net, &mut ada);
    assert_eq!(texts(&mut ada, &chat), ["wieder da"]);
}
