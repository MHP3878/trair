// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Communities (M8, step 3a): Discord-like servers with categories, open
//! channels, roles and rights, rules, invitations, kicking, banning,
//! reports and a log of what moderators did.
//!
//! Open channels are readable on the server on purpose (decision 9 of the
//! planning: moderation needs it), and the app says so. Every message in
//! them is signed by the sender's device, so the server can read and
//! delete it, but not write one in someone's name or change it.
//!
//! Closed channels (step 3b) are MLS groups: the server never sees their
//! texts. It names the group (`grp`) and decides who keeps it: changes of
//! that group are taken only from members who may manage channels (the
//! keepers), whatever the change itself claims. Keepers ask the server who
//! may read the channel and add or remove them.
//!
//! The server decides every right:
//! - The owner has all rights. Everyone else has the rights of the role
//!   "@alle" (id 0) and of their roles.
//! - A role's position says how high it stands; a member's power is that of
//!   their highest role. Nobody acts on someone as high or higher, or on
//!   the owner, and nobody gives rights or a position they do not have.
//! - A channel can be limited to roles for reading and for writing; who
//!   manages channels sees and writes everywhere.
//! - Members write only after accepting the current rules.
//!
//! Reports inside a community go to members with the right "Meldungen";
//! reports to the Trair team are seen only by accounts with a valid team
//! badge (or the team tool on the server's computer).
//!
//! Everything goes through `POST /v1/community` (`{"op": …}`); changes
//! arrive by long poll at `GET /v1/communities/sync`.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use trair_core::badges::{self, Badge, BadgeKind};
use trair_core::proofs::{OpenMessage, verify_closed_message, verify_open_message};

use crate::{ApiError, MAX_WAIT, Server, Session, bad, internal, load_account, random_hex};

/// Rights, as bits.
pub mod perm {
    /// Name, description, rules.
    pub const MANAGE_COMMUNITY: u32 = 1;
    /// Create, change and delete channels; see and write in all of them.
    pub const MANAGE_CHANNELS: u32 = 2;
    /// Create and change roles below the own one, give them to others.
    pub const MANAGE_ROLES: u32 = 4;
    /// Remove members (they may come back with a new invitation).
    pub const KICK: u32 = 8;
    /// Remove members for good.
    pub const BAN: u32 = 16;
    /// Delete other people's messages.
    pub const DELETE_MESSAGES: u32 = 32;
    /// Make invitations.
    pub const INVITE: u32 = 64;
    /// See and handle reports.
    pub const REPORTS: u32 = 128;
    /// See the log of moderator actions.
    pub const LOG: u32 = 256;
    /// Everything.
    pub const ALL: u32 = 511;
}

/// Most communities one account owns.
pub const MAX_OWNED: i64 = 20;
/// Most channels per community.
pub const MAX_CHANNELS: i64 = 100;
/// Most roles per community.
pub const MAX_ROLES: i64 = 50;
/// Most members per community.
pub const MAX_MEMBERS: i64 = 10_000;
/// Longest message, in characters.
pub const MAX_TEXT_CHARS: usize = 4000;
/// Longest description.
pub const MAX_ABOUT_CHARS: usize = 500;
/// Longest rules text.
pub const MAX_RULES_CHARS: usize = 4000;
/// Longest channel, category or role name.
pub const MAX_NAME_CHARS: usize = 32;
/// Longest channel topic.
pub const MAX_TOPIC_CHARS: usize = 200;
/// Longest reason (report, kick, ban) or note.
pub const MAX_REASON_CHARS: usize = 1000;
/// Largest evidence a reporter attaches (forwarded encrypted messages).
pub const MAX_EVIDENCE_BYTES: usize = 16 * 1024;
/// Longest time an invitation works (30 days).
pub const MAX_INVITE_SECS: u64 = 30 * 86_400;
/// Messages per account and minute.
pub const POSTS_PER_MINUTE: usize = 30;
/// Reports per account and hour.
pub const REPORTS_PER_HOUR: usize = 20;
/// How far a message's own time may be from the server's.
pub const MAX_CLOCK_SKEW: u64 = 300;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS c_communities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    about TEXT NOT NULL,
    rules TEXT NOT NULL,
    rules_rev INTEGER NOT NULL,
    owner TEXT NOT NULL,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS c_roles (
    community TEXT NOT NULL,
    id INTEGER NOT NULL,
    name TEXT NOT NULL,
    color INTEGER NOT NULL,
    position INTEGER NOT NULL,
    perms INTEGER NOT NULL,
    PRIMARY KEY (community, id)
);
CREATE TABLE IF NOT EXISTS c_members (
    community TEXT NOT NULL,
    user TEXT NOT NULL,
    joined INTEGER NOT NULL,
    rules_rev INTEGER NOT NULL,
    PRIMARY KEY (community, user)
);
CREATE TABLE IF NOT EXISTS c_member_roles (
    community TEXT NOT NULL,
    user TEXT NOT NULL,
    role INTEGER NOT NULL,
    PRIMARY KEY (community, user, role)
);
CREATE TABLE IF NOT EXISTS c_channels (
    community TEXT NOT NULL,
    id TEXT NOT NULL,
    category TEXT NOT NULL,
    name TEXT NOT NULL,
    topic TEXT NOT NULL,
    position INTEGER NOT NULL,
    read_roles TEXT,
    write_roles TEXT,
    closed INTEGER NOT NULL DEFAULT 0,
    grp TEXT,
    PRIMARY KEY (community, id)
);
CREATE TABLE IF NOT EXISTS c_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    community TEXT NOT NULL,
    channel TEXT NOT NULL,
    user TEXT NOT NULL,
    device TEXT NOT NULL,
    text TEXT NOT NULL,
    sent INTEGER NOT NULL,
    sig TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS c_messages_of ON c_messages (community, channel, id);
CREATE TABLE IF NOT EXISTS c_invites (
    id TEXT PRIMARY KEY,
    code_hash TEXT NOT NULL UNIQUE,
    community TEXT NOT NULL,
    creator TEXT NOT NULL,
    uses_left INTEGER,
    ends INTEGER NOT NULL,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS c_bans (
    community TEXT NOT NULL,
    user TEXT NOT NULL,
    by TEXT NOT NULL,
    reason TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (community, user)
);
CREATE TABLE IF NOT EXISTS c_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team INTEGER NOT NULL,
    community TEXT,
    reporter TEXT NOT NULL,
    target TEXT,
    channel TEXT,
    message INTEGER,
    snapshot TEXT NOT NULL,
    reason TEXT NOT NULL,
    at INTEGER NOT NULL,
    status TEXT NOT NULL,
    handled_by TEXT,
    note TEXT NOT NULL,
    handled_at INTEGER
);
CREATE TABLE IF NOT EXISTS c_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    community TEXT NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    detail TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS c_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    community TEXT NOT NULL,
    kind TEXT NOT NULL,
    channel TEXT NOT NULL,
    ref INTEGER NOT NULL,
    target TEXT NOT NULL,
    at INTEGER NOT NULL
);
";

// ------------------------------------------------------------------ rules

/// A role of a community.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    /// 0 is "@alle" (everyone).
    pub id: i64,
    /// Name.
    pub name: String,
    /// Colour, 0xAARRGGBB.
    pub color: u32,
    /// Higher stands above lower.
    pub position: i64,
    /// Rights (see [`perm`]).
    pub perms: u32,
}

/// A member as the rules see them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// Owns the community.
    pub owner: bool,
    /// Role ids (without 0).
    pub roles: Vec<i64>,
}

/// The rights of a member.
pub fn perms_of(m: &Standing, roles: &[Role]) -> u32 {
    if m.owner {
        return perm::ALL;
    }
    roles
        .iter()
        .filter(|r| r.id == 0 || m.roles.contains(&r.id))
        .fold(0, |p, r| p | r.perms)
}

/// How high a member stands: the owner above all, else the highest role.
pub fn power_of(m: &Standing, roles: &[Role]) -> i64 {
    if m.owner {
        return i64::MAX;
    }
    roles
        .iter()
        .filter(|r| m.roles.contains(&r.id))
        .map(|r| r.position)
        .max()
        .unwrap_or(0)
}

/// May `actor` act on `target` (kick, ban, delete messages, change roles)?
pub fn above(actor: &Standing, target: &Standing, roles: &[Role]) -> bool {
    !target.owner && power_of(actor, roles) > power_of(target, roles)
}

/// May `actor` create or change a role to these values?
pub fn may_shape_role(actor: &Standing, roles: &[Role], position: i64, perms: u32) -> bool {
    let mine = perms_of(actor, roles);
    mine & perm::MANAGE_ROLES != 0
        && position > 0
        && position < power_of(actor, roles)
        && perms & !mine == 0
        && perms & !perm::ALL == 0
}

/// A channel's limits: `None` = everyone, else these roles (0 = everyone
/// listed explicitly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    /// Who sees the channel.
    pub read: Option<Vec<i64>>,
    /// Who writes in it.
    pub write: Option<Vec<i64>>,
}

fn listed(m: &Standing, list: &Option<Vec<i64>>) -> bool {
    match list {
        None => true,
        Some(ids) => ids.iter().any(|id| *id == 0 || m.roles.contains(id)),
    }
}

/// May `m` see the channel?
pub fn can_read(m: &Standing, roles: &[Role], a: &Access) -> bool {
    perms_of(m, roles) & perm::MANAGE_CHANNELS != 0 || listed(m, &a.read)
}

/// May `m` write in the channel (rules aside)?
pub fn can_write(m: &Standing, roles: &[Role], a: &Access) -> bool {
    perms_of(m, roles) & perm::MANAGE_CHANNELS != 0 || (listed(m, &a.read) && listed(m, &a.write))
}

// ------------------------------------------------------------- the store

fn err(status: StatusCode, code: &'static str) -> ApiError {
    ApiError(status, code)
}

fn forbidden() -> ApiError {
    err(StatusCode::FORBIDDEN, "not_allowed")
}

fn not_found() -> ApiError {
    err(StatusCode::NOT_FOUND, "not_found")
}

fn sql(e: rusqlite::Error) -> ApiError {
    internal(e)
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// A short single-line name: trimmed, no control characters.
fn clean_name(text: &str, max: usize) -> Result<String, ApiError> {
    let t = text.trim();
    if t.is_empty() || chars(t) > max || t.chars().any(char::is_control) {
        return Err(bad("bad_name"));
    }
    Ok(t.to_string())
}

/// A longer text (description, rules, topic): newlines allowed.
fn clean_text(text: &str, max: usize) -> Result<String, ApiError> {
    let t = text.trim();
    if chars(t) > max || t.chars().any(|c| c.is_control() && c != '\n') {
        return Err(bad("bad_text"));
    }
    Ok(t.to_string())
}

struct Community {
    name: String,
    about: String,
    rules: String,
    rules_rev: i64,
    owner: String,
    created: i64,
}

fn community(c: &Connection, id: &str) -> Result<Community, ApiError> {
    c.query_row(
        "SELECT name, about, rules, rules_rev, owner, created FROM c_communities WHERE id = ?1",
        [id],
        |r| {
            Ok(Community {
                name: r.get(0)?,
                about: r.get(1)?,
                rules: r.get(2)?,
                rules_rev: r.get(3)?,
                owner: r.get(4)?,
                created: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(sql)?
    .ok_or_else(not_found)
}

fn roles(c: &Connection, id: &str) -> Result<Vec<Role>, ApiError> {
    let mut st = c
        .prepare(
            "SELECT id, name, color, position, perms FROM c_roles WHERE community = ?1
             ORDER BY position DESC, id",
        )
        .map_err(sql)?;
    let rows = st
        .query_map([id], |r| {
            Ok(Role {
                id: r.get(0)?,
                name: r.get(1)?,
                color: u32::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                position: r.get(3)?,
                perms: u32::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
            })
        })
        .map_err(sql)?;
    rows.collect::<rusqlite::Result<_>>().map_err(sql)
}

/// A member's standing, or `None` if `user` is no member.
fn standing(
    c: &Connection,
    id: &str,
    owner: &str,
    user: &str,
) -> Result<Option<Standing>, ApiError> {
    let member: Option<i64> = c
        .query_row(
            "SELECT joined FROM c_members WHERE community = ?1 AND user = ?2",
            params![id, user],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    if member.is_none() {
        return Ok(None);
    }
    let mut st = c
        .prepare("SELECT role FROM c_member_roles WHERE community = ?1 AND user = ?2 ORDER BY role")
        .map_err(sql)?;
    let ids = st
        .query_map(params![id, user], |r| r.get(0))
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<i64>>>()
        .map_err(sql)?;
    Ok(Some(Standing {
        owner: owner == user,
        roles: ids,
    }))
}

/// Who is asking, as a member of the community.
struct Actor {
    id: String,
    user: String,
    c: Community,
    roles: Vec<Role>,
    me: Standing,
}

impl Actor {
    fn load(conn: &Connection, id: &str, user: &str) -> Result<Actor, ApiError> {
        let c = community(conn, id)?;
        let roles = roles(conn, id)?;
        let me = standing(conn, id, &c.owner, user)?.ok_or_else(forbidden)?;
        Ok(Actor {
            id: id.to_string(),
            user: user.to_string(),
            c,
            roles,
            me,
        })
    }

    fn perms(&self) -> u32 {
        perms_of(&self.me, &self.roles)
    }

    fn need(&self, p: u32) -> Result<(), ApiError> {
        if self.perms() & p == p {
            Ok(())
        } else {
            Err(forbidden())
        }
    }

    fn power(&self) -> i64 {
        power_of(&self.me, &self.roles)
    }
}

fn parse_ids(text: Option<String>) -> Option<Vec<i64>> {
    text.and_then(|t| serde_json::from_str(&t).ok())
}

struct Channel {
    id: String,
    category: String,
    name: String,
    topic: String,
    position: i64,
    access: Access,
    /// End-to-end encrypted: an MLS group, `grp` its id.
    closed: bool,
    grp: Option<String>,
}

fn channels(c: &Connection, id: &str) -> Result<Vec<Channel>, ApiError> {
    let mut st = c
        .prepare(
            "SELECT id, category, name, topic, position, read_roles, write_roles, closed, grp
             FROM c_channels WHERE community = ?1 ORDER BY position, name",
        )
        .map_err(sql)?;
    let rows = st
        .query_map([id], |r| {
            Ok(Channel {
                id: r.get(0)?,
                category: r.get(1)?,
                name: r.get(2)?,
                topic: r.get(3)?,
                position: r.get(4)?,
                access: Access {
                    read: parse_ids(r.get(5)?),
                    write: parse_ids(r.get(6)?),
                },
                closed: r.get::<_, i64>(7)? != 0,
                grp: r.get(8)?,
            })
        })
        .map_err(sql)?;
    rows.collect::<rusqlite::Result<_>>().map_err(sql)
}

fn channel(c: &Connection, id: &str, ch: &str) -> Result<Channel, ApiError> {
    channels(c, id)?
        .into_iter()
        .find(|x| x.id == ch)
        .ok_or_else(not_found)
}

fn rules_ok(conn: &Connection, a: &Actor) -> Result<bool, ApiError> {
    if a.c.rules.is_empty() || a.me.owner {
        return Ok(true);
    }
    let accepted: i64 = conn
        .query_row(
            "SELECT rules_rev FROM c_members WHERE community = ?1 AND user = ?2",
            params![a.id, a.user],
            |r| r.get(0),
        )
        .map_err(sql)?;
    Ok(accepted >= a.c.rules_rev)
}

fn event(
    c: &Connection,
    id: &str,
    kind: &str,
    ch: &str,
    r: i64,
    target: &str,
    now: u64,
) -> Result<(), ApiError> {
    c.execute(
        "INSERT INTO c_events (community, kind, channel, ref, target, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, kind, ch, r, target, now as i64],
    )
    .map_err(sql)?;
    Ok(())
}

fn log(
    c: &Connection,
    id: &str,
    actor: &str,
    action: &str,
    target: &str,
    detail: &str,
    now: u64,
) -> Result<(), ApiError> {
    c.execute(
        "INSERT INTO c_log (community, actor, action, target, detail, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, actor, action, target, detail, now as i64],
    )
    .map_err(sql)?;
    Ok(())
}

fn members_of(c: &Connection, id: &str) -> Result<Vec<String>, ApiError> {
    let mut st = c
        .prepare("SELECT user FROM c_members WHERE community = ?1 ORDER BY joined, user")
        .map_err(sql)?;
    let rows = st.query_map([id], |r| r.get(0)).map_err(sql)?;
    rows.collect::<rusqlite::Result<_>>().map_err(sql)
}

fn remove_member(c: &Connection, id: &str, user: &str) -> Result<(), ApiError> {
    for table in ["c_members", "c_member_roles"] {
        c.execute(
            &format!("DELETE FROM {table} WHERE community = ?1 AND user = ?2"),
            params![id, user],
        )
        .map_err(sql)?;
    }
    Ok(())
}

fn delete_community(c: &Connection, id: &str, now: u64) -> Result<(), ApiError> {
    for user in members_of(c, id)? {
        event(c, id, "removed", "", 0, &user, now)?;
    }
    for table in [
        "c_roles",
        "c_members",
        "c_member_roles",
        "c_channels",
        "c_messages",
        "c_invites",
        "c_bans",
        "c_log",
    ] {
        c.execute(&format!("DELETE FROM {table} WHERE community = ?1"), [id])
            .map_err(sql)?;
    }
    c.execute(
        "DELETE FROM c_reports WHERE community = ?1 AND team = 0",
        [id],
    )
    .map_err(sql)?;
    c.execute("DELETE FROM c_communities WHERE id = ?1", [id])
        .map_err(sql)?;
    Ok(())
}

/// Who keeps the closed channels of a community: the owner and every
/// member who may manage channels.
fn keepers(c: &Connection, id: &str) -> Result<Vec<String>, ApiError> {
    let k = community(c, id)?;
    let rs = roles(c, id)?;
    let mut out = Vec::new();
    for m in members_of(c, id)? {
        if let Some(st) = standing(c, id, &k.owner, &m)?
            && perms_of(&st, &rs) & perm::MANAGE_CHANNELS != 0
        {
            out.push(m);
        }
    }
    out.sort();
    Ok(out)
}

/// For the MLS group of a closed channel: the accounts that may change it
/// now. `None` for any other group.
pub(crate) fn keepers_of_group(
    c: &Connection,
    group: &str,
) -> rusqlite::Result<Option<Vec<String>>> {
    let id: Option<String> = c
        .query_row(
            "SELECT community FROM c_channels WHERE grp = ?1",
            [group],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => keepers(c, &id)
            .map(Some)
            .map_err(|_| rusqlite::Error::InvalidQuery),
        None => Ok(None),
    }
}

/// A device's public key from an account's stored device list (also of
/// a device withdrawn since).
fn device_key(c: &Connection, user: &str, device: &str) -> Result<Option<Vec<u8>>, ApiError> {
    let list: Option<String> = c
        .query_row(
            "SELECT device_list FROM accounts WHERE user = ?1",
            [user],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    let Some(list) = list else { return Ok(None) };
    let list: trair_core::SignedDeviceList = serde_json::from_str(&list).map_err(internal)?;
    Ok(list
        .list
        .devices
        .iter()
        .chain(&list.list.withdrawn)
        .find(|d| d.id == device)
        .and_then(|d| hex::decode(&d.key).ok()))
}

/// An account is deleted: it leaves every community, its messages in
/// open channels go, and a community it owned goes to the member who
/// stands highest (or is deleted when nobody is left).
pub(crate) fn forget_user(c: &Connection, user: &str, now: u64) -> rusqlite::Result<()> {
    let ids: Vec<(String, String)> = {
        let mut st = c.prepare(
            "SELECT m.community, k.owner FROM c_members m JOIN c_communities k ON k.id = m.community
             WHERE m.user = ?1",
        )?;
        let rows = st.query_map([user], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (id, owner) in ids {
        let inner = || -> Result<(), ApiError> {
            if owner == user {
                let rs = roles(c, &id)?;
                let mut best: Option<(i64, String)> = None;
                for m in members_of(c, &id)?.into_iter().filter(|m| m != user) {
                    let s = standing(c, &id, &owner, &m)?.unwrap_or(Standing {
                        owner: false,
                        roles: Vec::new(),
                    });
                    let p = power_of(&s, &rs);
                    // members_of is in joining order: the earliest wins a tie.
                    if best.as_ref().is_none_or(|(bp, _)| p > *bp) {
                        best = Some((p, m));
                    }
                }
                let Some((_, next)) = best else {
                    return delete_community(c, &id, now);
                };
                c.execute(
                    "UPDATE c_communities SET owner = ?2 WHERE id = ?1",
                    params![id, next],
                )
                .map_err(sql)?;
                log(c, &id, user, "successor", &next, "", now)?;
            }
            remove_member(c, &id, user)?;
            event(c, &id, "changed", "", 0, "", now)?;
            Ok(())
        };
        // The only failures here are the database's own.
        inner().map_err(|_| rusqlite::Error::InvalidQuery)?;
    }
    c.execute("DELETE FROM c_messages WHERE user = ?1", [user])?;
    c.execute("DELETE FROM c_bans WHERE user = ?1", [user])?;
    c.execute("DELETE FROM c_invites WHERE creator = ?1", [user])?;
    Ok(())
}

/// How long the log of moderator actions is kept (180 days).
pub const LOG_KEEP: u64 = 180 * 86_400;
/// How long a handled report is kept (90 days); open ones stay.
pub const REPORT_KEEP: u64 = 90 * 86_400;

/// Delete events older than `keep` seconds, the log after [`LOG_KEEP`],
/// handled reports after [`REPORT_KEEP`] and invitations that ran out.
pub(crate) fn cleanup(c: &Connection, now: u64, keep: u64) -> rusqlite::Result<usize> {
    let before = |secs: u64| now.saturating_sub(secs) as i64;
    c.execute("DELETE FROM c_log WHERE at <= ?1", [before(LOG_KEEP)])?;
    c.execute(
        "DELETE FROM c_reports WHERE status != 'open' AND handled_at <= ?1",
        [before(REPORT_KEEP)],
    )?;
    c.execute(
        "DELETE FROM c_invites WHERE ends <= ?1 OR uses_left = 0",
        [now as i64],
    )?;
    c.execute("DELETE FROM c_events WHERE at <= ?1", [before(keep)])
}

// ---------------------------------------------------------------- requests

/// One request to `POST /v1/community`.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Op {
    Create {
        name: String,
    },
    List {},
    Info {
        community: String,
    },
    Edit {
        community: String,
        name: String,
        about: String,
        rules: String,
    },
    AcceptRules {
        community: String,
        rev: i64,
    },
    CreateChannel {
        community: String,
        name: String,
        category: String,
        #[serde(default)]
        topic: String,
        read_roles: Option<Vec<i64>>,
        write_roles: Option<Vec<i64>>,
        #[serde(default)]
        closed: bool,
    },
    ChannelMembers {
        community: String,
        channel: String,
    },
    EditChannel {
        community: String,
        channel: String,
        name: String,
        category: String,
        topic: String,
        position: i64,
        read_roles: Option<Vec<i64>>,
        write_roles: Option<Vec<i64>>,
    },
    DeleteChannel {
        community: String,
        channel: String,
    },
    CreateRole {
        community: String,
        name: String,
        color: u32,
        perms: u32,
    },
    EditRole {
        community: String,
        role: i64,
        name: String,
        color: u32,
        position: i64,
        perms: u32,
    },
    DeleteRole {
        community: String,
        role: i64,
    },
    SetRoles {
        community: String,
        user: String,
        roles: Vec<i64>,
    },
    Kick {
        community: String,
        user: String,
        #[serde(default)]
        reason: String,
    },
    Ban {
        community: String,
        user: String,
        #[serde(default)]
        reason: String,
    },
    Unban {
        community: String,
        user: String,
    },
    Bans {
        community: String,
    },
    CreateInvite {
        community: String,
        minutes: u64,
        uses: Option<u32>,
    },
    Invites {
        community: String,
    },
    RevokeInvite {
        community: String,
        id: String,
    },
    Join {
        code: String,
    },
    Leave {
        community: String,
    },
    Transfer {
        community: String,
        user: String,
    },
    Delete {
        community: String,
    },
    Post {
        community: String,
        channel: String,
        text: String,
        sent: u64,
        sig: String,
    },
    Messages {
        community: String,
        channel: String,
        before: Option<i64>,
        limit: Option<u32>,
    },
    DeleteMessage {
        community: String,
        id: i64,
    },
    Report {
        community: Option<String>,
        channel: Option<String>,
        message: Option<i64>,
        user: Option<String>,
        reason: String,
        #[serde(default)]
        team: bool,
        evidence: Option<Value>,
    },
    Reports {
        community: String,
        #[serde(default)]
        all: bool,
    },
    HandleReport {
        community: String,
        id: i64,
        status: String,
        #[serde(default)]
        note: String,
    },
    Log {
        community: String,
        before: Option<i64>,
    },
    TeamReports {
        badge: Value,
        #[serde(default)]
        all: bool,
    },
    TeamHandle {
        badge: Value,
        id: i64,
        status: String,
        #[serde(default)]
        note: String,
    },
}

/// Invitation codes: `trairc:` and 20 hex characters. Only the hash is
/// stored.
pub const INVITE_PREFIX: &str = "trairc:";

fn code_hash(code: &str) -> String {
    hex::encode(Sha256::digest(code.trim().to_ascii_lowercase().as_bytes()))
}

fn role_json(r: &Role) -> Value {
    json!({"id": r.id, "name": r.name, "color": r.color, "position": r.position, "perms": r.perms})
}

fn report_json(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let snapshot: String = r.get(7)?;
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "community": r.get::<_, Option<String>>(1)?,
        "reporter": r.get::<_, String>(2)?,
        "target": r.get::<_, Option<String>>(3)?,
        "channel": r.get::<_, Option<String>>(4)?,
        "message": r.get::<_, Option<i64>>(5)?,
        "reason": r.get::<_, String>(6)?,
        "snapshot": serde_json::from_str::<Value>(&snapshot).unwrap_or(Value::Null),
        "at": r.get::<_, i64>(8)?,
        "status": r.get::<_, String>(9)?,
        "handled_by": r.get::<_, Option<String>>(10)?,
        "note": r.get::<_, String>(11)?,
    }))
}

const REPORT_COLUMNS: &str = "id, community, reporter, target, channel, message, reason, snapshot, at, status, handled_by, note";

/// Who should hear of a change: members and anyone named.
struct Touched(BTreeSet<String>);

impl Touched {
    fn new() -> Self {
        Touched(BTreeSet::new())
    }

    fn members(&mut self, c: &Connection, id: &str) -> Result<(), ApiError> {
        self.0.extend(members_of(c, id)?);
        Ok(())
    }
}

fn is_team(s: &Server, user: &str, badge: &Value, now: u64) -> Result<(), ApiError> {
    let badge: Badge = serde_json::from_value(badge.clone()).map_err(|_| bad("bad_badge"))?;
    let (key, _, _) = load_account(s, user)?;
    let keys: Vec<&str> = s.team_keys.iter().map(String::as_str).collect();
    match badges::verify(&badge, &key, now, &keys) {
        Ok(v) if v.kind == BadgeKind::Team => Ok(()),
        _ => Err(forbidden()),
    }
}

/// `POST /v1/community`.
pub(crate) async fn op(
    State(s): State<Arc<Server>>,
    me: Session,
    Json(r): Json<Op>,
) -> Result<Json<Value>, ApiError> {
    let now = s.now();
    // Bots join communities only through the right "add bots" (step 4b).
    if crate::bots::is_bot(&s.db().conn, &me.user).map_err(internal)? {
        return Err(ApiError(StatusCode::FORBIDDEN, "bot"));
    }
    // Checks that need the account outside the database lock.
    let device_entry = if let Op::Post { .. } = &r {
        let (_, list, _) = load_account(&s, &me.user)?;
        list.list
            .devices
            .iter()
            .find(|d| d.id == me.device)
            .cloned()
            .ok_or_else(forbidden)?
            .into()
    } else {
        None
    };
    match &r {
        Op::TeamReports { badge, .. } | Op::TeamHandle { badge, .. } => {
            is_team(&s, &me.user, badge, now)?;
        }
        Op::Post { .. } if !s.allow_user("post", &me.user, POSTS_PER_MINUTE, 60) => {
            return Err(err(StatusCode::TOO_MANY_REQUESTS, "slow_down"));
        }
        Op::Report { .. } if !s.allow_user("report", &me.user, REPORTS_PER_HOUR, 3600) => {
            return Err(err(StatusCode::TOO_MANY_REQUESTS, "slow_down"));
        }
        _ => {}
    }
    let mut touched = Touched::new();
    let answer = {
        let db = s.db();
        let tx = db.conn.unchecked_transaction().map_err(sql)?;
        let out = run(&tx, &me, r, now, device_entry, &mut touched)?;
        tx.commit().map_err(sql)?;
        out
    };
    s.notify_users(&touched.0);
    Ok(Json(answer))
}

#[allow(clippy::too_many_lines)]
fn run(
    c: &Connection,
    me: &Session,
    r: Op,
    now: u64,
    device: Option<trair_core::devices::DeviceEntry>,
    touched: &mut Touched,
) -> Result<Value, ApiError> {
    let user = me.user.as_str();
    match r {
        Op::Create { name } => {
            badges::validate_community_name(name.trim()).map_err(|_| bad("bad_name"))?;
            let owned: i64 = c
                .query_row(
                    "SELECT count(*) FROM c_communities WHERE owner = ?1",
                    [user],
                    |r| r.get(0),
                )
                .map_err(sql)?;
            if owned >= MAX_OWNED {
                return Err(err(StatusCode::TOO_MANY_REQUESTS, "too_many"));
            }
            let id = random_hex::<16>()?;
            c.execute(
                "INSERT INTO c_communities (id, name, about, rules, rules_rev, owner, created)
                 VALUES (?1, ?2, '', '', 0, ?3, ?4)",
                params![id, name.trim(), user, now as i64],
            )
            .map_err(sql)?;
            for (rid, rname, color, pos, perms) in [
                (0_i64, "@alle", 0xFF9A98AA_u32, 0_i64, 0_u32),
                (1, "Admin", 0xFFFF8A98, 20, perm::ALL),
                (
                    2,
                    "Moderator",
                    0xFF5EE6B0,
                    10,
                    perm::KICK | perm::DELETE_MESSAGES | perm::REPORTS | perm::LOG,
                ),
            ] {
                c.execute(
                    "INSERT INTO c_roles (community, id, name, color, position, perms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, rid, rname, i64::from(color), pos, i64::from(perms)],
                )
                .map_err(sql)?;
            }
            for (pos, (cid, name, topic, write)) in [
                ("regeln", "regeln", "Bitte einmal lesen", Some("[]")),
                ("allgemein", "allgemein", "Für alles", None),
            ]
            .iter()
            .enumerate()
            {
                c.execute(
                    "INSERT INTO c_channels (community, id, category, name, topic, position, read_roles, write_roles)
                     VALUES (?1, ?2, 'Allgemein', ?3, ?4, ?5, NULL, ?6)",
                    params![id, cid, name, topic, pos as i64, write],
                )
                .map_err(sql)?;
            }
            c.execute(
                "INSERT INTO c_members (community, user, joined, rules_rev) VALUES (?1, ?2, ?3, 0)",
                params![id, user, now as i64],
            )
            .map_err(sql)?;
            touched.0.insert(user.to_string());
            Ok(json!({"ok": true, "community": id}))
        }
        Op::List {} => {
            let mut st = c
                .prepare(
                    "SELECT k.id, k.name, k.owner FROM c_communities k
                     JOIN c_members m ON m.community = k.id WHERE m.user = ?1 ORDER BY m.joined",
                )
                .map_err(sql)?;
            let rows = st
                .query_map([user], |r| {
                    Ok(
                        json!({"id": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?,
                        "owner": r.get::<_, String>(2)?}),
                    )
                })
                .map_err(sql)?;
            let list: Vec<Value> = rows.collect::<rusqlite::Result<_>>().map_err(sql)?;
            let last: i64 = c
                .query_row("SELECT coalesce(max(seq), 0) FROM c_events", [], |r| {
                    r.get(0)
                })
                .map_err(sql)?;
            Ok(json!({"ok": true, "communities": list, "seq": last}))
        }
        Op::Info { community: id } => {
            let a = Actor::load(c, &id, user)?;
            let ok = rules_ok(c, &a)?;
            let chans: Vec<Value> = channels(c, &id)?
                .into_iter()
                .filter(|ch| can_read(&a.me, &a.roles, &ch.access))
                .map(|ch| {
                    json!({
                        "id": ch.id, "category": ch.category, "name": ch.name, "topic": ch.topic,
                        "position": ch.position, "read_roles": ch.access.read,
                        "write_roles": ch.access.write,
                        "can_write": ok && can_write(&a.me, &a.roles, &ch.access),
                        "closed": ch.closed, "group": ch.grp,
                    })
                })
                .collect();
            let mut members = Vec::new();
            for m in members_of(c, &id)? {
                let st = standing(c, &id, &a.c.owner, &m)?.unwrap_or(Standing {
                    owner: false,
                    roles: Vec::new(),
                });
                members.push(json!({"user": m, "roles": st.roles, "owner": st.owner,
                    "power": power_of(&st, &a.roles).min(i64::from(i32::MAX))}));
            }
            Ok(json!({
                "ok": true,
                "community": {"id": id, "name": a.c.name, "about": a.c.about, "rules": a.c.rules,
                    "rules_rev": a.c.rules_rev, "owner": a.c.owner, "created": a.c.created},
                "me": {"perms": a.perms(), "power": a.power().min(i64::from(i32::MAX)),
                    "roles": a.me.roles, "owner": a.me.owner, "rules_ok": ok},
                "roles": a.roles.iter().map(role_json).collect::<Vec<_>>(),
                "channels": chans,
                "members": members,
            }))
        }
        Op::Edit {
            community: id,
            name,
            about,
            rules,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::MANAGE_COMMUNITY)?;
            badges::validate_community_name(name.trim()).map_err(|_| bad("bad_name"))?;
            let about = clean_text(&about, MAX_ABOUT_CHARS)?;
            let rules = clean_text(&rules, MAX_RULES_CHARS)?;
            let rev = if rules == a.c.rules {
                a.c.rules_rev
            } else {
                a.c.rules_rev + 1
            };
            c.execute(
                "UPDATE c_communities SET name = ?2, about = ?3, rules = ?4, rules_rev = ?5 WHERE id = ?1",
                params![id, name.trim(), about, rules, rev],
            )
            .map_err(sql)?;
            let what = if rev == a.c.rules_rev {
                "edit"
            } else {
                "rules"
            };
            log(c, &id, user, what, "", name.trim(), now)?;
            event(c, &id, "changed", "", 0, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true, "rules_rev": rev}))
        }
        Op::AcceptRules { community: id, rev } => {
            let a = Actor::load(c, &id, user)?;
            if rev != a.c.rules_rev {
                return Err(err(StatusCode::CONFLICT, "rules_changed"));
            }
            c.execute(
                "UPDATE c_members SET rules_rev = ?3 WHERE community = ?1 AND user = ?2",
                params![id, user, rev],
            )
            .map_err(sql)?;
            touched.0.insert(user.to_string());
            Ok(json!({"ok": true}))
        }
        Op::CreateChannel {
            community: id,
            name,
            category,
            topic,
            read_roles,
            write_roles,
            closed,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::MANAGE_CHANNELS)?;
            let name = clean_name(&name, MAX_NAME_CHARS)?;
            let category = clean_name(&category, MAX_NAME_CHARS)?;
            let topic = clean_text(&topic, MAX_TOPIC_CHARS)?;
            check_role_ids(&a, &read_roles)?;
            check_role_ids(&a, &write_roles)?;
            let n: i64 = c
                .query_row(
                    "SELECT count(*) FROM c_channels WHERE community = ?1",
                    [&id],
                    |r| r.get(0),
                )
                .map_err(sql)?;
            if n >= MAX_CHANNELS {
                return Err(err(StatusCode::TOO_MANY_REQUESTS, "too_many"));
            }
            let ch = random_hex::<4>()?;
            // A closed channel gets the id of its MLS group now, so the
            // server knows whose changes to take before the first one.
            let grp = if closed {
                Some(random_hex::<16>()?)
            } else {
                None
            };
            c.execute(
                "INSERT INTO c_channels (community, id, category, name, topic, position, read_roles, write_roles,
                   closed, grp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    ch,
                    category,
                    name,
                    topic,
                    n,
                    ids_text(&read_roles),
                    ids_text(&write_roles),
                    closed,
                    grp
                ],
            )
            .map_err(sql)?;
            log(
                c,
                &id,
                user,
                "channel_create",
                &name,
                if closed { "closed" } else { "" },
                now,
            )?;
            event(c, &id, "changed", &ch, 0, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true, "channel": ch, "group": grp}))
        }
        Op::ChannelMembers {
            community: id,
            channel: ch,
        } => {
            let a = Actor::load(c, &id, user)?;
            let chan = channel(c, &id, &ch)?;
            if !can_read(&a.me, &a.roles, &chan.access) {
                return Err(forbidden());
            }
            let mut readers = Vec::new();
            for m in members_of(c, &id)? {
                if let Some(st) = standing(c, &id, &a.c.owner, &m)?
                    && can_read(&st, &a.roles, &chan.access)
                {
                    readers.push(m);
                }
            }
            readers.sort();
            // Has a keeper made the MLS group yet (its first change taken)?
            let started: bool = match &chan.grp {
                Some(g) => c
                    .query_row(
                        "SELECT next_epoch > 0 FROM groups WHERE id = ?1",
                        [g],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql)?
                    .unwrap_or(false),
                None => false,
            };
            Ok(
                json!({"ok": true, "readers": readers, "keepers": keepers(c, &id)?,
                "group": chan.grp, "closed": chan.closed, "started": started}),
            )
        }
        Op::EditChannel {
            community: id,
            channel: ch,
            name,
            category,
            topic,
            position,
            read_roles,
            write_roles,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::MANAGE_CHANNELS)?;
            channel(c, &id, &ch)?;
            let name = clean_name(&name, MAX_NAME_CHARS)?;
            let category = clean_name(&category, MAX_NAME_CHARS)?;
            let topic = clean_text(&topic, MAX_TOPIC_CHARS)?;
            check_role_ids(&a, &read_roles)?;
            check_role_ids(&a, &write_roles)?;
            c.execute(
                "UPDATE c_channels SET name = ?3, category = ?4, topic = ?5, position = ?6,
                   read_roles = ?7, write_roles = ?8 WHERE community = ?1 AND id = ?2",
                params![
                    id,
                    ch,
                    name,
                    category,
                    topic,
                    position,
                    ids_text(&read_roles),
                    ids_text(&write_roles)
                ],
            )
            .map_err(sql)?;
            log(c, &id, user, "channel_edit", &name, "", now)?;
            event(c, &id, "changed", &ch, 0, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true}))
        }
        Op::DeleteChannel {
            community: id,
            channel: ch,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::MANAGE_CHANNELS)?;
            let old = channel(c, &id, &ch)?;
            c.execute(
                "DELETE FROM c_channels WHERE community = ?1 AND id = ?2",
                params![id, ch],
            )
            .map_err(sql)?;
            c.execute(
                "DELETE FROM c_messages WHERE community = ?1 AND channel = ?2",
                params![id, ch],
            )
            .map_err(sql)?;
            log(c, &id, user, "channel_delete", &old.name, "", now)?;
            event(c, &id, "changed", &ch, 0, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true}))
        }
        Op::CreateRole {
            community: id,
            name,
            color,
            perms,
        } => {
            let a = Actor::load(c, &id, user)?;
            let name = clean_name(&name, MAX_NAME_CHARS)?;
            if i64::try_from(a.roles.len()).unwrap_or(i64::MAX) >= MAX_ROLES {
                return Err(err(StatusCode::TOO_MANY_REQUESTS, "too_many"));
            }
            // Just below the creator (above everything for the owner).
            let position = if a.me.owner {
                a.roles.iter().map(|r| r.position).max().unwrap_or(0) + 10
            } else {
                a.power() - 1
            };
            if !may_shape_role(&a.me, &a.roles, position, perms) {
                return Err(forbidden());
            }
            let rid = a.roles.iter().map(|r| r.id).max().unwrap_or(0) + 1;
            c.execute(
                "INSERT INTO c_roles (community, id, name, color, position, perms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, rid, name, i64::from(color), position, i64::from(perms)],
            )
            .map_err(sql)?;
            log(c, &id, user, "role_create", &name, &perms.to_string(), now)?;
            event(c, &id, "changed", "", 0, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true, "role": rid}))
        }
        Op::EditRole {
            community: id,
            role,
            name,
            color,
            position,
            perms,
        } => {
            let a = Actor::load(c, &id, user)?;
            let old = a
                .roles
                .iter()
                .find(|r| r.id == role)
                .cloned()
                .ok_or_else(not_found)?;
            let name = clean_name(&name, MAX_NAME_CHARS)?;
            if role == 0 {
                // "@alle": only its rights change, by someone allowed to.
                let mine = a.perms();
                if mine & perm::MANAGE_ROLES == 0 || perms & !mine != 0 || perms & !perm::ALL != 0 {
                    return Err(forbidden());
                }
                c.execute(
                    "UPDATE c_roles SET perms = ?3 WHERE community = ?1 AND id = 0",
                    params![id, role, i64::from(perms)],
                )
                .map_err(sql)?;
            } else {
                // The role itself and where it goes must lie below the actor.
                if old.position >= a.power() || !may_shape_role(&a.me, &a.roles, position, perms) {
                    return Err(forbidden());
                }
                // Rights it had and the actor lacks stay untouched only if
                // unchanged: nobody takes away what they could not give.
                if (old.perms ^ perms) & !a.perms() != 0 {
                    return Err(forbidden());
                }
                c.execute(
                    "UPDATE c_roles SET name = ?3, color = ?4, position = ?5, perms = ?6
                     WHERE community = ?1 AND id = ?2",
                    params![id, role, name, i64::from(color), position, i64::from(perms)],
                )
                .map_err(sql)?;
            }
            log(
                c,
                &id,
                user,
                "role_edit",
                &old.name,
                &perms.to_string(),
                now,
            )?;
            event(c, &id, "changed", "", 0, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true}))
        }
        Op::DeleteRole {
            community: id,
            role,
        } => {
            let a = Actor::load(c, &id, user)?;
            let old = a
                .roles
                .iter()
                .find(|r| r.id == role)
                .cloned()
                .ok_or_else(not_found)?;
            if role == 0 || a.perms() & perm::MANAGE_ROLES == 0 || old.position >= a.power() {
                return Err(forbidden());
            }
            c.execute(
                "DELETE FROM c_roles WHERE community = ?1 AND id = ?2",
                params![id, role],
            )
            .map_err(sql)?;
            c.execute(
                "DELETE FROM c_member_roles WHERE community = ?1 AND role = ?2",
                params![id, role],
            )
            .map_err(sql)?;
            log(c, &id, user, "role_delete", &old.name, "", now)?;
            event(c, &id, "changed", "", 0, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true}))
        }
        Op::SetRoles {
            community: id,
            user: target,
            roles: wanted,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::MANAGE_ROLES)?;
            let t = standing(c, &id, &a.c.owner, &target)?.ok_or_else(not_found)?;
            if target != user && !above(&a.me, &t, &a.roles) {
                return Err(forbidden());
            }
            let wanted: BTreeSet<i64> = wanted.into_iter().collect();
            let have: BTreeSet<i64> = t.roles.iter().copied().collect();
            let power = a.power();
            for r in wanted.symmetric_difference(&have) {
                let role = a.roles.iter().find(|x| x.id == *r).ok_or_else(not_found)?;
                if role.id == 0 || role.position >= power {
                    return Err(forbidden());
                }
            }
            c.execute(
                "DELETE FROM c_member_roles WHERE community = ?1 AND user = ?2",
                params![id, target],
            )
            .map_err(sql)?;
            for r in &wanted {
                c.execute(
                    "INSERT INTO c_member_roles (community, user, role) VALUES (?1, ?2, ?3)",
                    params![id, target, r],
                )
                .map_err(sql)?;
            }
            let detail = wanted
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            log(c, &id, user, "roles", &target, &detail, now)?;
            event(c, &id, "changed", "", 0, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true}))
        }
        Op::Kick {
            community: id,
            user: target,
            reason,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::KICK)?;
            let reason = clean_text(&reason, MAX_REASON_CHARS)?;
            let t = standing(c, &id, &a.c.owner, &target)?.ok_or_else(not_found)?;
            if !above(&a.me, &t, &a.roles) {
                return Err(forbidden());
            }
            touched.members(c, &id)?;
            remove_member(c, &id, &target)?;
            log(c, &id, user, "kick", &target, &reason, now)?;
            event(c, &id, "removed", "", 0, &target, now)?;
            event(c, &id, "changed", "", 0, "", now)?;
            Ok(json!({"ok": true}))
        }
        Op::Ban {
            community: id,
            user: target,
            reason,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::BAN)?;
            let reason = clean_text(&reason, MAX_REASON_CHARS)?;
            if target == user {
                return Err(forbidden());
            }
            let exists: Option<String> = c
                .query_row(
                    "SELECT user FROM accounts WHERE user = ?1",
                    [&target],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)?;
            if exists.is_none() {
                return Err(not_found());
            }
            if let Some(t) = standing(c, &id, &a.c.owner, &target)? {
                if !above(&a.me, &t, &a.roles) {
                    return Err(forbidden());
                }
                touched.members(c, &id)?;
                remove_member(c, &id, &target)?;
                event(c, &id, "removed", "", 0, &target, now)?;
            } else if target == a.c.owner {
                return Err(forbidden());
            }
            c.execute(
                "INSERT INTO c_bans (community, user, by, reason, at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (community, user) DO UPDATE SET by = excluded.by, reason = excluded.reason, at = excluded.at",
                params![id, target, user, reason, now as i64],
            )
            .map_err(sql)?;
            log(c, &id, user, "ban", &target, &reason, now)?;
            event(c, &id, "changed", "", 0, "", now)?;
            Ok(json!({"ok": true}))
        }
        Op::Unban {
            community: id,
            user: target,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::BAN)?;
            let n = c
                .execute(
                    "DELETE FROM c_bans WHERE community = ?1 AND user = ?2",
                    params![id, target],
                )
                .map_err(sql)?;
            if n == 0 {
                return Err(not_found());
            }
            log(c, &id, user, "unban", &target, "", now)?;
            Ok(json!({"ok": true}))
        }
        Op::Bans { community: id } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::BAN)?;
            let mut st = c
                .prepare(
                    "SELECT user, by, reason, at FROM c_bans WHERE community = ?1 ORDER BY at DESC",
                )
                .map_err(sql)?;
            let rows = st
                .query_map([&id], |r| {
                    Ok(
                        json!({"user": r.get::<_, String>(0)?, "by": r.get::<_, String>(1)?,
                        "reason": r.get::<_, String>(2)?, "at": r.get::<_, i64>(3)?}),
                    )
                })
                .map_err(sql)?;
            let list: Vec<Value> = rows.collect::<rusqlite::Result<_>>().map_err(sql)?;
            Ok(json!({"ok": true, "bans": list}))
        }
        Op::CreateInvite {
            community: id,
            minutes,
            uses,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::INVITE)?;
            let secs = minutes.saturating_mul(60);
            if secs == 0
                || secs > MAX_INVITE_SECS
                || uses == Some(0)
                || uses.is_some_and(|u| u > 10_000)
            {
                return Err(bad("bad_invite"));
            }
            let code = format!("{INVITE_PREFIX}{}", random_hex::<10>()?);
            let hash = code_hash(&code);
            let iid = hash[..8].to_string();
            c.execute(
                "INSERT INTO c_invites (id, code_hash, community, creator, uses_left, ends, created)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![iid, hash, id, user, uses.map(i64::from), (now + secs) as i64, now as i64],
            )
            .map_err(sql)?;
            log(c, &id, user, "invite", &iid, "", now)?;
            Ok(json!({"ok": true, "code": code, "id": iid, "ends": now + secs, "uses": uses}))
        }
        Op::Invites { community: id } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::INVITE)?;
            let all = a.perms() & perm::MANAGE_COMMUNITY != 0;
            let mut st = c
                .prepare(
                    "SELECT id, creator, uses_left, ends FROM c_invites
                     WHERE community = ?1 AND ends > ?2 AND (?3 OR creator = ?4) ORDER BY created",
                )
                .map_err(sql)?;
            let rows = st
                .query_map(params![id, now as i64, all, user], |r| {
                    Ok(
                        json!({"id": r.get::<_, String>(0)?, "creator": r.get::<_, String>(1)?,
                        "uses": r.get::<_, Option<i64>>(2)?, "ends": r.get::<_, i64>(3)?}),
                    )
                })
                .map_err(sql)?;
            let list: Vec<Value> = rows.collect::<rusqlite::Result<_>>().map_err(sql)?;
            Ok(json!({"ok": true, "invites": list}))
        }
        Op::RevokeInvite {
            community: id,
            id: iid,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::INVITE)?;
            let all = a.perms() & perm::MANAGE_COMMUNITY != 0;
            let n = c
                .execute(
                    "DELETE FROM c_invites WHERE community = ?1 AND id = ?2 AND (?3 OR creator = ?4)",
                    params![id, iid, all, user],
                )
                .map_err(sql)?;
            if n == 0 {
                return Err(not_found());
            }
            log(c, &id, user, "invite_revoke", &iid, "", now)?;
            Ok(json!({"ok": true}))
        }
        Op::Join { code } => {
            let hash = code_hash(&code);
            let inv: Option<(String, Option<i64>, i64)> = c
                .query_row(
                    "SELECT community, uses_left, ends FROM c_invites WHERE code_hash = ?1",
                    [&hash],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(sql)?;
            let Some((id, uses, ends)) = inv else {
                return Err(err(StatusCode::NOT_FOUND, "bad_invite"));
            };
            if ends <= now as i64 || uses == Some(0) {
                return Err(err(StatusCode::GONE, "invite_expired"));
            }
            let banned: Option<String> = c
                .query_row(
                    "SELECT user FROM c_bans WHERE community = ?1 AND user = ?2",
                    params![id, user],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)?;
            if banned.is_some() {
                return Err(err(StatusCode::FORBIDDEN, "banned"));
            }
            let k = community(c, &id)?;
            if standing(c, &id, &k.owner, user)?.is_some() {
                return Ok(json!({"ok": true, "community": id, "already": true}));
            }
            let n = members_of(c, &id)?.len();
            if i64::try_from(n).unwrap_or(i64::MAX) >= MAX_MEMBERS {
                return Err(err(StatusCode::TOO_MANY_REQUESTS, "full"));
            }
            c.execute(
                "INSERT INTO c_members (community, user, joined, rules_rev) VALUES (?1, ?2, ?3, 0)",
                params![id, user, now as i64],
            )
            .map_err(sql)?;
            if uses.is_some() {
                c.execute(
                    "UPDATE c_invites SET uses_left = uses_left - 1 WHERE code_hash = ?1",
                    [&hash],
                )
                .map_err(sql)?;
            }
            event(c, &id, "joined", "", 0, user, now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true, "community": id}))
        }
        Op::Leave { community: id } => {
            let a = Actor::load(c, &id, user)?;
            touched.members(c, &id)?;
            if a.me.owner {
                if members_of(c, &id)?.len() > 1 {
                    return Err(err(StatusCode::CONFLICT, "hand_over_first"));
                }
                delete_community(c, &id, now)?;
                return Ok(json!({"ok": true, "deleted": true}));
            }
            remove_member(c, &id, user)?;
            event(c, &id, "left", "", 0, user, now)?;
            event(c, &id, "removed", "", 0, user, now)?;
            Ok(json!({"ok": true}))
        }
        Op::Transfer {
            community: id,
            user: target,
        } => {
            let a = Actor::load(c, &id, user)?;
            if !a.me.owner || target == user {
                return Err(forbidden());
            }
            standing(c, &id, &a.c.owner, &target)?.ok_or_else(not_found)?;
            c.execute(
                "UPDATE c_communities SET owner = ?2 WHERE id = ?1",
                params![id, target],
            )
            .map_err(sql)?;
            log(c, &id, user, "transfer", &target, "", now)?;
            event(c, &id, "changed", "", 0, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true}))
        }
        Op::Delete { community: id } => {
            let a = Actor::load(c, &id, user)?;
            if !a.me.owner {
                return Err(forbidden());
            }
            touched.members(c, &id)?;
            delete_community(c, &id, now)?;
            Ok(json!({"ok": true}))
        }
        Op::Post {
            community: id,
            channel: ch,
            text,
            sent,
            sig,
        } => {
            let a = Actor::load(c, &id, user)?;
            let chan = channel(c, &id, &ch)?;
            if chan.closed {
                return Err(bad("closed_channel"));
            }
            if !can_write(&a.me, &a.roles, &chan.access) {
                return Err(forbidden());
            }
            if !rules_ok(c, &a)? {
                return Err(err(StatusCode::FORBIDDEN, "accept_rules"));
            }
            if text.trim().is_empty() || chars(&text) > MAX_TEXT_CHARS {
                return Err(bad("bad_text"));
            }
            if sent.abs_diff(now) > MAX_CLOCK_SKEW {
                return Err(bad("clock"));
            }
            let entry = device.ok_or_else(forbidden)?;
            let signature = hex::decode(&sig).map_err(|_| bad("bad_signature"))?;
            let m = OpenMessage {
                community: &id,
                channel: &ch,
                user,
                device: &me.device,
                sent,
                text: &text,
            };
            verify_open_message(&entry, &m, &signature)
                .map_err(|_| err(StatusCode::FORBIDDEN, "bad_signature"))?;
            c.execute(
                "INSERT INTO c_messages (community, channel, user, device, text, sent, sig, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, ch, user, me.device, text, sent as i64, sig, now as i64],
            )
            .map_err(sql)?;
            let mid = c.last_insert_rowid();
            event(c, &id, "message", &ch, mid, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true, "id": mid, "at": now}))
        }
        Op::Messages {
            community: id,
            channel: ch,
            before,
            limit,
        } => {
            let a = Actor::load(c, &id, user)?;
            let chan = channel(c, &id, &ch)?;
            if chan.closed {
                return Err(bad("closed_channel"));
            }
            if !can_read(&a.me, &a.roles, &chan.access) {
                return Err(forbidden());
            }
            let limit = i64::from(limit.unwrap_or(100).clamp(1, 200));
            let mut st = c
                .prepare(
                    "SELECT id, user, device, text, sent, sig, at FROM c_messages
                     WHERE community = ?1 AND channel = ?2 AND id < ?3 ORDER BY id DESC LIMIT ?4",
                )
                .map_err(sql)?;
            let rows = st
                .query_map(
                    params![id, ch, before.unwrap_or(i64::MAX), limit],
                    message_json,
                )
                .map_err(sql)?;
            let mut list: Vec<Value> = rows.collect::<rusqlite::Result<_>>().map_err(sql)?;
            list.reverse();
            Ok(json!({"ok": true, "messages": list}))
        }
        Op::DeleteMessage {
            community: id,
            id: mid,
        } => {
            let a = Actor::load(c, &id, user)?;
            let row: Option<(String, String)> = c
                .query_row(
                    "SELECT user, channel FROM c_messages WHERE community = ?1 AND id = ?2",
                    params![id, mid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(sql)?;
            let (author, ch) = row.ok_or_else(not_found)?;
            if author != user {
                a.need(perm::DELETE_MESSAGES)?;
                if let Some(t) = standing(c, &id, &a.c.owner, &author)?
                    && !above(&a.me, &t, &a.roles)
                {
                    return Err(forbidden());
                }
                let chan = channel(c, &id, &ch)?;
                log(c, &id, user, "message_delete", &author, &chan.name, now)?;
            }
            c.execute(
                "DELETE FROM c_messages WHERE community = ?1 AND id = ?2",
                params![id, mid],
            )
            .map_err(sql)?;
            event(c, &id, "deleted", &ch, mid, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true}))
        }
        Op::Report {
            community,
            channel: ch,
            message,
            user: target,
            reason,
            team,
            evidence,
        } => {
            let reason = clean_text(&reason, MAX_REASON_CHARS)?;
            if reason.is_empty() {
                return Err(bad("reason_needed"));
            }
            let mut snapshot = json!(null);
            let mut target = target;
            let mut ch = ch;
            let mut closed_evidence = false;
            if let Some(id) = &community {
                let a = Actor::load(c, id, user)?;
                if let Some(mid) = message {
                    let row: Option<(String, String, String, i64)> = c
                        .query_row(
                            "SELECT user, channel, text, sent FROM c_messages WHERE community = ?1 AND id = ?2",
                            params![id, mid],
                            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                        )
                        .optional()
                        .map_err(sql)?;
                    let (author, mch, text, sent) = row.ok_or_else(not_found)?;
                    let chan = channel(c, id, &mch)?;
                    if !can_read(&a.me, &a.roles, &chan.access) {
                        return Err(forbidden());
                    }
                    snapshot =
                        json!({"user": author, "channel": chan.name, "text": text, "sent": sent});
                    target = Some(author);
                    ch = Some(mch);
                } else if let (Some(cid), Some(e)) = (&ch, &evidence) {
                    // A text from a closed channel: the server never had it,
                    // so the reporter hands it over, and its device
                    // signature shows it was written like that.
                    let chan = channel(c, id, cid)?;
                    if chan.closed {
                        if !can_read(&a.me, &a.roles, &chan.access) {
                            return Err(forbidden());
                        }
                        let t = closed_text(c, id, cid, e)?;
                        snapshot = json!({"user": t.0, "channel": chan.name, "text": t.1,
                            "sent": t.2, "signed": true});
                        target = Some(t.0);
                        closed_evidence = true;
                    }
                }
            } else if message.is_some() || ch.is_some() {
                return Err(bad("community_needed"));
            }
            if !team && community.is_none() {
                return Err(bad("community_needed"));
            }
            if let Some(e) = evidence.filter(|_| !closed_evidence) {
                if !team
                    || serde_json::to_vec(&e).map_or(usize::MAX, |b| b.len()) > MAX_EVIDENCE_BYTES
                {
                    return Err(bad("bad_evidence"));
                }
                snapshot = json!({"forwarded": e});
            }
            if target.is_none() && snapshot.is_null() {
                return Err(bad("nothing_reported"));
            }
            c.execute(
                "INSERT INTO c_reports (team, community, reporter, target, channel, message, snapshot, reason, at,
                   status, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'open', '')",
                params![
                    team,
                    community,
                    user,
                    target,
                    ch,
                    message,
                    snapshot.to_string(),
                    reason,
                    now as i64
                ],
            )
            .map_err(sql)?;
            let rid = c.last_insert_rowid();
            if let (Some(id), false) = (&community, team) {
                event(c, id, "report", "", rid, "", now)?;
                touched.members(c, id)?;
            }
            Ok(json!({"ok": true, "id": rid}))
        }
        Op::Reports { community: id, all } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::REPORTS)?;
            let mut st = c
                .prepare(&format!(
                    "SELECT {REPORT_COLUMNS} FROM c_reports WHERE team = 0 AND community = ?1
                     AND (?2 OR status = 'open') ORDER BY id DESC LIMIT 200"
                ))
                .map_err(sql)?;
            let rows = st.query_map(params![id, all], report_json).map_err(sql)?;
            let list: Vec<Value> = rows.collect::<rusqlite::Result<_>>().map_err(sql)?;
            Ok(json!({"ok": true, "reports": list}))
        }
        Op::HandleReport {
            community: id,
            id: rid,
            status,
            note,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::REPORTS)?;
            let note = clean_text(&note, MAX_REASON_CHARS)?;
            if !matches!(status.as_str(), "open" | "done" | "dismissed") {
                return Err(bad("bad_status"));
            }
            let n = c
                .execute(
                    "UPDATE c_reports SET status = ?3, note = ?4, handled_by = ?5, handled_at = ?6
                     WHERE team = 0 AND community = ?1 AND id = ?2",
                    params![id, rid, status, note, user, now as i64],
                )
                .map_err(sql)?;
            if n == 0 {
                return Err(not_found());
            }
            log(c, &id, user, "report", &rid.to_string(), &status, now)?;
            event(c, &id, "report", "", rid, "", now)?;
            touched.members(c, &id)?;
            Ok(json!({"ok": true}))
        }
        Op::Log {
            community: id,
            before,
        } => {
            let a = Actor::load(c, &id, user)?;
            a.need(perm::LOG)?;
            let mut st = c
                .prepare(
                    "SELECT id, actor, action, target, detail, at FROM c_log
                     WHERE community = ?1 AND id < ?2 ORDER BY id DESC LIMIT 100",
                )
                .map_err(sql)?;
            let rows = st
                .query_map(params![id, before.unwrap_or(i64::MAX)], |r| {
                    Ok(
                        json!({"id": r.get::<_, i64>(0)?, "actor": r.get::<_, String>(1)?,
                        "action": r.get::<_, String>(2)?, "target": r.get::<_, String>(3)?,
                        "detail": r.get::<_, String>(4)?, "at": r.get::<_, i64>(5)?}),
                    )
                })
                .map_err(sql)?;
            let list: Vec<Value> = rows.collect::<rusqlite::Result<_>>().map_err(sql)?;
            Ok(json!({"ok": true, "log": list}))
        }
        Op::TeamReports { all, .. } => {
            let mut st = c
                .prepare(&format!(
                    "SELECT {REPORT_COLUMNS} FROM c_reports WHERE team = 1
                     AND (?1 OR status = 'open') ORDER BY id DESC LIMIT 500"
                ))
                .map_err(sql)?;
            let rows = st.query_map(params![all], report_json).map_err(sql)?;
            let list: Vec<Value> = rows.collect::<rusqlite::Result<_>>().map_err(sql)?;
            Ok(json!({"ok": true, "reports": list}))
        }
        Op::TeamHandle {
            id: rid,
            status,
            note,
            ..
        } => {
            let note = clean_text(&note, MAX_REASON_CHARS)?;
            if !matches!(status.as_str(), "open" | "done" | "dismissed") {
                return Err(bad("bad_status"));
            }
            let n = c
                .execute(
                    "UPDATE c_reports SET status = ?2, note = ?3, handled_by = ?4, handled_at = ?5
                     WHERE team = 1 AND id = ?1",
                    params![rid, status, note, user, now as i64],
                )
                .map_err(sql)?;
            if n == 0 {
                return Err(not_found());
            }
            Ok(json!({"ok": true}))
        }
    }
}

/// A reported text of a closed channel, checked against its author's
/// device: (author, text, sent).
fn closed_text(
    c: &Connection,
    community: &str,
    channel: &str,
    e: &Value,
) -> Result<(String, String, u64), ApiError> {
    let field = |k: &str| {
        e.get(k)
            .and_then(Value::as_str)
            .ok_or_else(|| bad("bad_evidence"))
    };
    let (author, device, text) = (field("user")?, field("device")?, field("text")?);
    let sent = e
        .get("sent")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("bad_evidence"))?;
    let sig = hex::decode(field("sig")?).map_err(|_| bad("bad_evidence"))?;
    if chars(text) > MAX_TEXT_CHARS {
        return Err(bad("bad_evidence"));
    }
    let key = device_key(c, author, device)?.ok_or_else(|| bad("bad_evidence"))?;
    let m = OpenMessage {
        community,
        channel,
        user: author,
        device,
        sent,
        text,
    };
    verify_closed_message(&key, &m, &sig)
        .map_err(|_| err(StatusCode::FORBIDDEN, "bad_signature"))?;
    Ok((author.to_string(), text.to_string(), sent))
}

fn ids_text(ids: &Option<Vec<i64>>) -> Option<String> {
    ids.as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
}

fn check_role_ids(a: &Actor, ids: &Option<Vec<i64>>) -> Result<(), ApiError> {
    if let Some(list) = ids
        && (list.len() > 64 || list.iter().any(|id| !a.roles.iter().any(|r| r.id == *id)))
    {
        return Err(bad("unknown_role"));
    }
    Ok(())
}

fn message_json(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "user": r.get::<_, String>(1)?,
        "device": r.get::<_, String>(2)?,
        "text": r.get::<_, String>(3)?,
        "sent": r.get::<_, i64>(4)?,
        "sig": r.get::<_, String>(5)?,
        "at": r.get::<_, i64>(6)?,
    }))
}

// ------------------------------------------------------------------- sync

#[derive(Deserialize)]
pub(crate) struct Since {
    since: i64,
    #[serde(default)]
    wait: u64,
}

/// What happened in this account's communities since `since`: new and
/// deleted messages (only from channels it may read), changes, and being
/// removed. `since = -1` only answers where the log stands now.
pub(crate) async fn sync(
    State(s): State<Arc<Server>>,
    me: Session,
    Query(q): Query<Since>,
) -> Result<Json<Value>, ApiError> {
    let notify = s.community_waiter(&me.user);
    let notified = notify.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    let mut out = s.db().events(&me.user, q.since)?;
    if out.0.is_empty() && q.wait > 0 && q.since >= 0 {
        let _ = tokio::time::timeout(Duration::from_secs(q.wait.min(MAX_WAIT)), notified).await;
        out = s.db().events(&me.user, q.since)?;
    }
    Ok(Json(json!({"ok": true, "events": out.0, "seq": out.1})))
}

impl crate::Db {
    /// Events for `user` after `since`, and the last sequence number.
    fn events(&self, user: &str, since: i64) -> Result<(Vec<Value>, i64), ApiError> {
        let c = &self.conn;
        if since < 0 {
            let last: i64 = c
                .query_row("SELECT coalesce(max(seq), 0) FROM c_events", [], |r| {
                    r.get(0)
                })
                .map_err(sql)?;
            return Ok((Vec::new(), last));
        }
        let mut st = c
            .prepare(
                "SELECT seq, community, kind, channel, ref, target FROM c_events
                 WHERE seq > ?1 AND (target = ?2 OR community IN
                   (SELECT community FROM c_members WHERE user = ?2))
                 ORDER BY seq LIMIT 200",
            )
            .map_err(sql)?;
        let rows: Vec<(i64, String, String, String, i64, String)> = st
            .query_map(params![since, user], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(sql)?
            .collect::<rusqlite::Result<_>>()
            .map_err(sql)?;
        let mut cache: HashMap<String, Option<(Community, Vec<Role>, Standing)>> = HashMap::new();
        let mut out = Vec::new();
        let mut seq = since;
        for (sq, id, kind, ch, r, target) in rows {
            seq = sq;
            let mut e = json!({"seq": sq, "community": id, "kind": kind, "channel": ch,
                "ref": r, "target": target});
            // Events about oneself (removed, joined) always arrive; the rest
            // only as far as the member may see it.
            let private = matches!(kind.as_str(), "message" | "deleted" | "report");
            if private && target != user {
                let me = match cache.entry(id.clone()) {
                    std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
                    std::collections::hash_map::Entry::Vacant(v) => {
                        let loaded = match community(c, &id) {
                            Ok(k) => {
                                let rs = roles(c, &id)?;
                                standing(c, &id, &k.owner, user)?.map(|s| (k, rs, s))
                            }
                            Err(_) => None,
                        };
                        v.insert(loaded)
                    }
                };
                let Some((_, rs, st)) = me else { continue };
                if kind == "report" && perms_of(st, rs) & perm::REPORTS == 0 {
                    continue;
                }
                if kind == "message" || kind == "deleted" {
                    let Ok(chan) = channel(c, &id, &ch) else {
                        continue;
                    };
                    if !can_read(st, rs, &chan.access) {
                        continue;
                    }
                }
            }
            if kind == "message" {
                let msg = c
                    .query_row(
                        "SELECT id, user, device, text, sent, sig, at FROM c_messages WHERE id = ?1",
                        [r],
                        message_json,
                    )
                    .optional()
                    .map_err(sql)?;
                match msg {
                    Some(m) => e["message"] = m,
                    None => continue,
                }
            }
            out.push(e);
        }
        Ok((out, seq))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: i64, position: i64, perms: u32) -> Role {
        Role {
            id,
            name: format!("r{id}"),
            color: 0,
            position,
            perms,
        }
    }

    fn roles() -> Vec<Role> {
        vec![
            r(0, 0, perm::INVITE),
            r(1, 20, perm::ALL),
            r(2, 10, perm::KICK | perm::DELETE_MESSAGES | perm::REPORTS),
            r(3, 5, 0),
        ]
    }

    fn m(roles: &[i64]) -> Standing {
        Standing {
            owner: false,
            roles: roles.to_vec(),
        }
    }

    const OWNER: Standing = Standing {
        owner: true,
        roles: Vec::new(),
    };

    #[test]
    fn rights_add_up_and_the_owner_has_all() {
        let rs = roles();
        assert_eq!(perms_of(&OWNER, &rs), perm::ALL);
        assert_eq!(perms_of(&m(&[]), &rs), perm::INVITE);
        assert_eq!(
            perms_of(&m(&[2, 3]), &rs),
            perm::INVITE | perm::KICK | perm::DELETE_MESSAGES | perm::REPORTS
        );
        assert_eq!(power_of(&m(&[2, 3]), &rs), 10);
        assert_eq!(power_of(&m(&[]), &rs), 0);
        assert_eq!(power_of(&OWNER, &rs), i64::MAX);
    }

    #[test]
    fn nobody_acts_on_someone_as_high_or_on_the_owner() {
        let rs = roles();
        let admin = m(&[1]);
        let modr = m(&[2]);
        let member = m(&[3]);
        assert!(above(&admin, &modr, &rs));
        assert!(above(&modr, &member, &rs));
        assert!(!above(&modr, &admin, &rs));
        assert!(!above(&modr, &m(&[2]), &rs), "same height");
        assert!(!above(&admin, &OWNER, &rs));
        assert!(above(&OWNER, &admin, &rs));
    }

    #[test]
    fn roles_are_shaped_only_below_and_with_own_rights() {
        let rs = roles();
        let admin = m(&[1]);
        let modr = m(&[2]);
        assert!(may_shape_role(&admin, &rs, 15, perm::KICK));
        assert!(!may_shape_role(&admin, &rs, 20, perm::KICK), "not as high");
        assert!(
            !may_shape_role(&admin, &rs, 0, perm::KICK),
            "not as low as @alle"
        );
        assert!(
            !may_shape_role(&modr, &rs, 5, perm::KICK),
            "no right to manage roles"
        );
        let mut rs2 = rs.clone();
        rs2[2].perms |= perm::MANAGE_ROLES;
        assert!(may_shape_role(&modr, &rs2, 5, perm::KICK));
        assert!(
            !may_shape_role(&modr, &rs2, 5, perm::BAN),
            "a right it lacks"
        );
        assert!(!may_shape_role(&OWNER, &rs, 5, 1 << 20), "unknown bits");
    }

    #[test]
    fn channels_limit_reading_and_writing() {
        let rs = roles();
        let open = Access {
            read: None,
            write: None,
        };
        let news = Access {
            read: None,
            write: Some(vec![]),
        };
        let staff = Access {
            read: Some(vec![2]),
            write: None,
        };
        let member = m(&[3]);
        let modr = m(&[2]);
        assert!(can_read(&member, &rs, &open) && can_write(&member, &rs, &open));
        assert!(can_read(&member, &rs, &news) && !can_write(&member, &rs, &news));
        assert!(!can_read(&member, &rs, &staff) && !can_write(&member, &rs, &staff));
        assert!(can_read(&modr, &rs, &staff) && can_write(&modr, &rs, &staff));
        // Managing channels sees and writes everywhere.
        assert!(can_write(&m(&[1]), &rs, &news) && can_read(&m(&[1]), &rs, &staff));
        // 0 in a list means everyone.
        let all = Access {
            read: Some(vec![0]),
            write: Some(vec![0]),
        };
        assert!(can_write(&member, &rs, &all));
    }
}
