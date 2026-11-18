// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! In-memory delivery service for the attack tests.
//!
//! It behaves like the real Trair server would, and it can be switched
//! into attack modes: inject, drop, swap key packages, accept two commits
//! for the same epoch. Everything it ever transported is kept in `log`,
//! so tests can prove that no plaintext reached it.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use trair_core::{Account, CoreError, Device, Event, KeyPackageOffer, Outgoing};

#[derive(Default)]
struct Directory {
    one_time: VecDeque<Vec<u8>>,
    last_resort: Option<Vec<u8>>,
}

/// Error from a test step: either the core or the server said no.
#[derive(Debug)]
pub enum StepError {
    Core(CoreError),
    Server(String),
}

impl From<CoreError> for StepError {
    fn from(e: CoreError) -> Self {
        StepError::Core(e)
    }
}

pub struct Net {
    honest: bool,
    dir: BTreeMap<String, Directory>,
    swaps: HashMap<String, String>,
    boxes: HashMap<String, VecDeque<Vec<u8>>>,
    epochs: HashMap<String, u64>,
    drops: HashSet<(String, String)>,
    pub log: Vec<Vec<u8>>,
}

impl Default for Net {
    fn default() -> Self {
        Net {
            honest: true,
            dir: BTreeMap::new(),
            swaps: HashMap::new(),
            boxes: HashMap::new(),
            epochs: HashMap::new(),
            drops: HashSet::new(),
            log: Vec::new(),
        }
    }
}

impl Net {
    /// Register a device under its own id with one-time key packages and
    /// a last-resort package.
    pub fn register(&mut self, dev: &mut Device, one_time: usize) {
        let id = dev.id().to_string();
        self.register_as(&id, dev, one_time);
    }

    /// Register a device under any directory name (used by the attacker,
    /// whose certificate claims a different id).
    pub fn register_as(&mut self, name: &str, dev: &mut Device, one_time: usize) {
        let entry = self.dir.entry(name.to_string()).or_default();
        entry.one_time.extend(dev.key_packages(one_time).unwrap());
        entry.last_resort = Some(dev.last_resort_key_package().unwrap());
        self.boxes.entry(dev.id().to_string()).or_default();
    }

    pub fn publish(&mut self, name: &str, dev: &Device, n: usize) {
        let entry = self.dir.entry(name.to_string()).or_default();
        entry.one_time.extend(dev.key_packages(n).unwrap());
    }

    /// One-time packages left for a device.
    pub fn available(&self, name: &str) -> usize {
        self.dir.get(name).map(|d| d.one_time.len()).unwrap_or(0)
    }

    /// Key packages for every device of a user. Uses a one-time package if
    /// there is one, else the last-resort package (which stays).
    pub fn fetch(&mut self, user: &str) -> Vec<KeyPackageOffer> {
        let prefix = format!("{user}:");
        let names: Vec<String> = self
            .dir
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        let mut out = Vec::new();
        for name in names {
            let source = self
                .swaps
                .get(&name)
                .cloned()
                .unwrap_or_else(|| name.clone());
            let Some(entry) = self.dir.get_mut(&source) else {
                continue;
            };
            let bytes = match entry.one_time.pop_front() {
                Some(b) => b,
                None => match &entry.last_resort {
                    Some(b) => b.clone(),
                    None => continue,
                },
            };
            out.push(KeyPackageOffer {
                claimed_device: name,
                bytes,
            });
        }
        out
    }

    fn deliver(&mut self, from: &str, to: &str, bytes: &[u8]) {
        if self.drops.contains(&(from.to_string(), to.to_string())) {
            return;
        }
        self.boxes
            .entry(to.to_string())
            .or_default()
            .push_back(bytes.to_vec());
    }

    /// Accept or refuse a commit. An honest server refuses a second commit
    /// for the same epoch.
    pub fn commit(
        &mut self,
        from: &str,
        out: &Outgoing,
        recipients: &[String],
    ) -> Result<(), String> {
        if self.honest
            && let Some(&e) = self.epochs.get(&out.group)
            && e != out.epoch
        {
            return Err(format!("409 wrong epoch: server {e}, commit {}", out.epoch));
        }
        self.epochs.insert(out.group.clone(), out.epoch + 1);
        self.log.push(out.commit.clone());
        for to in recipients {
            self.deliver(from, to, &out.commit);
        }
        if let Some(w) = &out.welcome {
            self.log.push(w.clone());
            for m in &out.added {
                self.deliver(from, &m.id, w);
            }
        }
        Ok(())
    }

    pub fn send(&mut self, from: &str, recipients: &[String], bytes: &[u8]) {
        self.log.push(bytes.to_vec());
        for to in recipients {
            self.deliver(from, to, bytes);
        }
    }

    /// The operator pushes arbitrary bytes into someone's inbox.
    pub fn inject(&mut self, to: &str, bytes: &[u8]) {
        self.boxes
            .entry(to.to_string())
            .or_default()
            .push_back(bytes.to_vec());
    }

    pub fn inbox(&mut self, id: &str) -> Vec<Vec<u8>> {
        self.boxes
            .entry(id.to_string())
            .or_default()
            .drain(..)
            .collect()
    }

    pub fn drop_between(&mut self, from: &str, to: &str) {
        self.drops.insert((from.to_string(), to.to_string()));
    }

    pub fn clear_drops(&mut self) {
        self.drops.clear();
    }

    pub fn set_honest(&mut self, on: bool) {
        self.honest = on;
    }

    /// Hand out `source`'s key packages whenever someone asks for `target`.
    pub fn swap(&mut self, target: &str, source: &str) {
        self.swaps.insert(target.to_string(), source.to_string());
    }
}

/// A person with one device.
pub fn person(user: &str, device: &str) -> (Account, Device) {
    let acc = Account::generate(user).unwrap();
    let dev = Device::new(&acc, device).unwrap();
    (acc, dev)
}

/// Add all devices of `users` to a group and run the commit through the server.
pub fn add(
    net: &mut Net,
    dev: &mut Device,
    group: &str,
    users: &[&str],
) -> Result<Outgoing, StepError> {
    let offers: Vec<KeyPackageOffer> = users.iter().flat_map(|u| net.fetch(u)).collect();
    let recipients = dev.recipients(group)?;
    let out = dev.add_members(group, &offers)?;
    finish(net, dev, &out, &recipients)?;
    Ok(out)
}

pub fn remove(
    net: &mut Net,
    dev: &mut Device,
    group: &str,
    user: &str,
) -> Result<Outgoing, StepError> {
    let recipients = dev.recipients(group)?;
    let out = dev.remove_user(group, user)?;
    finish(net, dev, &out, &recipients)?;
    Ok(out)
}

pub fn update(net: &mut Net, dev: &mut Device, group: &str) -> Result<Outgoing, StepError> {
    let recipients = dev.recipients(group)?;
    let out = dev.self_update(group)?;
    finish(net, dev, &out, &recipients)?;
    Ok(out)
}

fn finish(
    net: &mut Net,
    dev: &mut Device,
    out: &Outgoing,
    recipients: &[String],
) -> Result<(), StepError> {
    match net.commit(dev.id(), out, recipients) {
        Ok(()) => {
            dev.confirm(&out.group)?;
            Ok(())
        }
        Err(e) => {
            dev.discard(&out.group)?;
            Err(StepError::Server(e))
        }
    }
}

pub fn send(
    net: &mut Net,
    dev: &mut Device,
    group: &str,
    text: &str,
) -> Result<Vec<u8>, StepError> {
    let recipients = dev.recipients(group)?;
    let bytes = dev.encrypt(group, text)?;
    net.send(dev.id(), &recipients, &bytes);
    Ok(bytes)
}

pub fn poll(net: &mut Net, dev: &mut Device) -> Vec<Result<Event, CoreError>> {
    let id = dev.id().to_string();
    net.inbox(&id).iter().map(|b| dev.receive(b)).collect()
}

/// Poll and expect only successful events.
pub fn poll_ok(net: &mut Net, dev: &mut Device) -> Vec<Event> {
    poll(net, dev)
        .into_iter()
        .map(|r| r.unwrap_or_else(|e| panic!("a device could not process a message: {e}")))
        .collect()
}

pub fn member_ids(dev: &Device, group: &str) -> Vec<String> {
    let mut ids: Vec<String> = dev
        .members(group)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    ids.sort();
    ids
}
