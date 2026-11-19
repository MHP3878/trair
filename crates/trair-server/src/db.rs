// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Storage of the server in SQLite.
//!
//! What is stored: account keys and signed device lists (public), hashes
//! of session tokens, MLS key packages (public), and encrypted messages
//! until they are fetched. Messages carry no sender. Deleted rows are
//! overwritten (`secure_delete`).

use rusqlite::{Connection, OptionalExtension, params};

/// One account as stored.
pub struct AccountRow {
    /// Public account key, hex.
    pub key: String,
    /// The current signed device list, JSON.
    pub list: String,
    /// Its running number.
    pub seq: u64,
}

/// What a clean-up removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cleaned {
    /// Messages older than their lifetime.
    pub messages: usize,
    /// Expired sessions.
    pub sessions: usize,
    /// Accounts nobody used for too long, with all their data.
    pub accounts: usize,
    /// Chats nobody changed for as long.
    pub groups: usize,
}

/// Answer to a change of a chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// Taken; the chat is now at `epoch`.
    Taken {
        /// The new epoch.
        epoch: u64,
    },
    /// Another change was first. The chat is at `epoch`; `taken` is the
    /// hash of the change that got there.
    Refused {
        /// The server's epoch for the chat.
        epoch: u64,
        /// SHA-256 (hex) of the change taken last.
        taken: String,
    },
    /// The chat named the accounts that may change it, and this one is
    /// not among them.
    NotAllowed,
}

/// The database.
pub struct Db {
    conn: Connection,
}

type DbResult<T> = rusqlite::Result<T>;

impl Db {
    /// Open (and create) the database file.
    pub fn open(path: &std::path::Path) -> DbResult<Db> {
        Self::setup(Connection::open(path)?)
    }

    /// A database that lives only in memory. For tests.
    pub fn in_memory() -> DbResult<Db> {
        Self::setup(Connection::open_in_memory()?)
    }

    fn setup(conn: Connection) -> DbResult<Db> {
        conn.execute_batch(
            "PRAGMA secure_delete = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             CREATE TABLE IF NOT EXISTS accounts (
                 user TEXT PRIMARY KEY,
                 account_key TEXT NOT NULL,
                 device_list TEXT NOT NULL,
                 list_seq INTEGER NOT NULL,
                 created INTEGER NOT NULL,
                 last_seen INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 token_hash TEXT PRIMARY KEY,
                 user TEXT NOT NULL,
                 device TEXT NOT NULL,
                 expires INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS keypackages (
                 id INTEGER PRIMARY KEY,
                 user TEXT NOT NULL,
                 device TEXT NOT NULL,
                 data BLOB NOT NULL,
                 last_resort INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id INTEGER PRIMARY KEY,
                 user TEXT NOT NULL,
                 device TEXT NOT NULL,
                 body BLOB NOT NULL,
                 at INTEGER NOT NULL
             );
             -- Chats, only to order changes: random id, next epoch, hash of
             -- the last change taken, and (once a chat names them) the
             -- accounts that may change it. Not who is in them.
             CREATE TABLE IF NOT EXISTS groups (
                 id TEXT PRIMARY KEY,
                 next_epoch INTEGER NOT NULL,
                 last_hash TEXT NOT NULL,
                 updated INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS messages_to ON messages (user, device, id);
             CREATE INDEX IF NOT EXISTS keypackages_of ON keypackages (user, device);
             CREATE INDEX IF NOT EXISTS sessions_of ON sessions (user, device);",
        )?;
        // Databases from M3 lack the last-resort column.
        let has_column: bool = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('keypackages') WHERE name = 'last_resort'",
            [],
            |r| r.get::<_, i64>(0).map(|n| n > 0),
        )?;
        if !has_column {
            conn.execute_batch(
                "ALTER TABLE keypackages ADD COLUMN last_resort INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        // Databases from before groups lack the committers column.
        let has_committers: bool = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('groups') WHERE name = 'committers'",
            [],
            |r| r.get::<_, i64>(0).map(|n| n > 0),
        )?;
        if !has_committers {
            conn.execute_batch("ALTER TABLE groups ADD COLUMN committers TEXT;")?;
        }
        Ok(Db { conn })
    }

    /// Store a new account. False if the user name is taken.
    pub fn insert_account(
        &self,
        user: &str,
        key: &str,
        list: &str,
        seq: u64,
        now: u64,
    ) -> DbResult<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO accounts (user, account_key, device_list, list_seq, created, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![user, key, list, seq as i64, now as i64],
        )?;
        Ok(n == 1)
    }

    /// The stored account, if any.
    pub fn account(&self, user: &str) -> DbResult<Option<AccountRow>> {
        self.conn
            .query_row(
                "SELECT account_key, device_list, list_seq FROM accounts WHERE user = ?1",
                params![user],
                |r| {
                    Ok(AccountRow {
                        key: r.get(0)?,
                        list: r.get(1)?,
                        seq: r.get::<_, i64>(2)? as u64,
                    })
                },
            )
            .optional()
    }

    /// Replace the device list (the caller checked it is newer).
    pub fn set_list(&self, user: &str, list: &str, seq: u64) -> DbResult<()> {
        self.conn.execute(
            "UPDATE accounts SET device_list = ?2, list_seq = ?3 WHERE user = ?1",
            params![user, list, seq as i64],
        )?;
        Ok(())
    }

    /// Note that the account was used.
    pub fn touch(&self, user: &str, now: u64) -> DbResult<()> {
        self.conn.execute(
            "UPDATE accounts SET last_seen = ?2 WHERE user = ?1",
            params![user, now as i64],
        )?;
        Ok(())
    }

    /// Remember a session by the hash of its token.
    pub fn insert_session(
        &self,
        token_hash: &str,
        user: &str,
        device: &str,
        expires: u64,
    ) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO sessions (token_hash, user, device, expires) VALUES (?1, ?2, ?3, ?4)",
            params![token_hash, user, device, expires as i64],
        )?;
        Ok(())
    }

    /// User and device of a valid session.
    pub fn session(&self, token_hash: &str, now: u64) -> DbResult<Option<(String, String)>> {
        self.conn
            .query_row(
                "SELECT user, device FROM sessions WHERE token_hash = ?1 AND expires > ?2",
                params![token_hash, now as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }

    /// Remove sessions, key packages and waiting messages of a device that
    /// is no longer on its account's list.
    pub fn drop_device(&self, user: &str, device: &str) -> DbResult<()> {
        for table in ["sessions", "keypackages", "messages"] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE user = ?1 AND device = ?2"),
                params![user, device],
            )?;
        }
        Ok(())
    }

    /// Store key packages; at most `max` per device are kept.
    pub fn add_keypackages(
        &self,
        user: &str,
        device: &str,
        packages: &[Vec<u8>],
        max: usize,
    ) -> DbResult<usize> {
        let have = self.keypackage_count(user, device)?;
        let room = max.saturating_sub(have);
        for p in packages.iter().take(room) {
            self.conn.execute(
                "INSERT INTO keypackages (user, device, data) VALUES (?1, ?2, ?3)",
                params![user, device, p],
            )?;
        }
        Ok(room.min(packages.len()))
    }

    /// One-time key packages waiting for a device.
    pub fn keypackage_count(&self, user: &str, device: &str) -> DbResult<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM keypackages WHERE user = ?1 AND device = ?2 AND last_resort = 0",
            params![user, device],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Delete the one-time key packages of a device (an update made the
    /// old ones useless).
    pub fn drop_keypackages(&self, user: &str, device: &str) -> DbResult<usize> {
        self.conn.execute(
            "DELETE FROM keypackages WHERE user = ?1 AND device = ?2 AND last_resort = 0",
            params![user, device],
        )
    }

    /// Replace the last-resort key package of a device. It is handed out
    /// only when no one-time package is left, and never deleted by that.
    pub fn set_last_resort(&self, user: &str, device: &str, package: &[u8]) -> DbResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM keypackages WHERE user = ?1 AND device = ?2 AND last_resort = 1",
            params![user, device],
        )?;
        tx.execute(
            "INSERT INTO keypackages (user, device, data, last_resort) VALUES (?1, ?2, ?3, 1)",
            params![user, device, package],
        )?;
        tx.commit()
    }

    /// Take one key package of a device: a one-time package (deleted), or
    /// if none is left the last-resort package (kept).
    pub fn claim_keypackage(&self, user: &str, device: &str) -> DbResult<Option<Vec<u8>>> {
        let row: Option<(i64, Vec<u8>, i64)> = self
            .conn
            .query_row(
                "SELECT id, data, last_resort FROM keypackages WHERE user = ?1 AND device = ?2
                 ORDER BY last_resort, id LIMIT 1",
                params![user, device],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((id, data, last_resort)) = row {
            if last_resort == 0 {
                self.conn
                    .execute("DELETE FROM keypackages WHERE id = ?1", params![id])?;
            }
            return Ok(Some(data));
        }
        Ok(None)
    }

    /// Number of messages waiting for a device.
    pub fn queue_len(&self, user: &str, device: &str) -> DbResult<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE user = ?1 AND device = ?2",
            params![user, device],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Queue a message for a device.
    pub fn push_message(&self, user: &str, device: &str, body: &[u8], now: u64) -> DbResult<i64> {
        self.conn.execute(
            "INSERT INTO messages (user, device, body, at) VALUES (?1, ?2, ?3, ?4)",
            params![user, device, body, now as i64],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The oldest waiting messages of a device: id, body, time.
    pub fn messages(
        &self,
        user: &str,
        device: &str,
        limit: usize,
    ) -> DbResult<Vec<(i64, Vec<u8>, u64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, body, at FROM messages WHERE user = ?1 AND device = ?2 ORDER BY id LIMIT ?3")?;
        let rows = stmt.query_map(params![user, device, limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64))
        })?;
        rows.collect()
    }

    /// Take a change of a chat if it is the first for its epoch: store
    /// the new epoch and queue the commit and welcome, all or nothing.
    /// Otherwise nothing is queued and the answer names what was taken.
    ///
    /// Once a chat named the accounts that may change it (`committers`,
    /// sent along with a change), a change from any other account (`by`)
    /// is refused. A new list replaces the old one with a taken change.
    #[allow(clippy::too_many_arguments)]
    pub fn commit(
        &self,
        group: &str,
        epoch: u64,
        hash: &str,
        deliveries: &[(&str, &str, &[u8])],
        now: u64,
        by: &str,
        committers: Option<&[String]>,
    ) -> DbResult<CommitOutcome> {
        let tx = self.conn.unchecked_transaction()?;
        let known: Option<(i64, String, Option<String>)> = tx
            .query_row(
                "SELECT next_epoch, last_hash, committers FROM groups WHERE id = ?1",
                params![group],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((_, _, Some(list))) = &known {
            let allowed: Vec<String> = serde_json::from_str(list).unwrap_or_default();
            if !allowed.iter().any(|u| u == by) {
                return Ok(CommitOutcome::NotAllowed);
            }
        }
        if let Some((next, taken, _)) = known
            && next as u64 != epoch
        {
            return Ok(CommitOutcome::Refused {
                epoch: next as u64,
                taken,
            });
        }
        let list = committers.map(|c| serde_json::to_string(c).unwrap_or_default());
        tx.execute(
            "INSERT INTO groups (id, next_epoch, last_hash, updated, committers)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET next_epoch = excluded.next_epoch,
                 last_hash = excluded.last_hash, updated = excluded.updated,
                 committers = COALESCE(excluded.committers, groups.committers)",
            params![group, (epoch + 1) as i64, hash, now as i64, list],
        )?;
        for (user, device, body) in deliveries {
            tx.execute(
                "INSERT INTO messages (user, device, body, at) VALUES (?1, ?2, ?3, ?4)",
                params![user, device, body, now as i64],
            )?;
        }
        tx.commit()?;
        Ok(CommitOutcome::Taken { epoch: epoch + 1 })
    }

    /// Delete fetched messages; only the device's own.
    pub fn ack(&self, user: &str, device: &str, ids: &[i64]) -> DbResult<usize> {
        let mut n = 0;
        for id in ids {
            n += self.conn.execute(
                "DELETE FROM messages WHERE id = ?1 AND user = ?2 AND device = ?3",
                params![id, user, device],
            )?;
        }
        Ok(n)
    }

    /// Delete an account and everything that belongs to it.
    pub fn delete_account(&self, user: &str) -> DbResult<()> {
        for table in ["sessions", "keypackages", "messages", "accounts"] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE user = ?1"),
                params![user],
            )?;
        }
        Ok(())
    }

    /// Remove old messages, expired sessions and idle accounts.
    pub fn cleanup(&self, now: u64, message_lifetime: u64, idle_limit: u64) -> DbResult<Cleaned> {
        let mut cleaned = Cleaned {
            messages: self.conn.execute(
                "DELETE FROM messages WHERE at <= ?1",
                params![now.saturating_sub(message_lifetime) as i64],
            )?,
            sessions: self.conn.execute(
                "DELETE FROM sessions WHERE expires <= ?1",
                params![now as i64],
            )?,
            accounts: 0,
            groups: 0,
        };
        let idle: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT user FROM accounts WHERE last_seen <= ?1")?;
            let rows =
                stmt.query_map(params![now.saturating_sub(idle_limit) as i64], |r| r.get(0))?;
            rows.collect::<DbResult<_>>()?
        };
        for user in &idle {
            self.delete_account(user)?;
        }
        cleaned.accounts = idle.len();
        // Chats nobody changed for as long are forgotten too.
        cleaned.groups = self.conn.execute(
            "DELETE FROM groups WHERE updated <= ?1",
            params![now.saturating_sub(idle_limit) as i64],
        )?;
        Ok(cleaned)
    }

    /// Every table and column as `table.column`. For tests that prove
    /// what the server does not store.
    pub fn tables(&self) -> DbResult<Vec<String>> {
        let names: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<DbResult<_>>()?
        };
        let mut out = Vec::new();
        for t in names {
            let mut stmt = self
                .conn
                .prepare("SELECT name FROM pragma_table_info(?1)")?;
            let cols = stmt.query_map(params![t], |r| r.get::<_, String>(0))?;
            for c in cols {
                out.push(format!("{t}.{}", c?));
            }
        }
        Ok(out)
    }

    /// Every stored value of every table, one after the other. For tests
    /// that prove no plaintext reached the server.
    pub fn dump(&self) -> DbResult<Vec<u8>> {
        let names: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<DbResult<_>>()?
        };
        let mut out = Vec::new();
        for t in names {
            let mut stmt = self.conn.prepare(&format!("SELECT * FROM \"{t}\""))?;
            let n = stmt.column_count();
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                for i in 0..n {
                    match row.get_ref(i)? {
                        rusqlite::types::ValueRef::Text(b) | rusqlite::types::ValueRef::Blob(b) => {
                            out.extend_from_slice(b)
                        }
                        _ => {}
                    }
                    out.push(0);
                }
            }
        }
        Ok(out)
    }

    /// Column names of the message table. For the test that proves no
    /// sender is stored.
    pub fn message_columns(&self) -> DbResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM pragma_table_info('messages')")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    }
}
