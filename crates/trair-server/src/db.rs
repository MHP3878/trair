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
                 data BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id INTEGER PRIMARY KEY,
                 user TEXT NOT NULL,
                 device TEXT NOT NULL,
                 body BLOB NOT NULL,
                 at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS messages_to ON messages (user, device, id);
             CREATE INDEX IF NOT EXISTS keypackages_of ON keypackages (user, device);
             CREATE INDEX IF NOT EXISTS sessions_of ON sessions (user, device);",
        )?;
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
        let have: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM keypackages WHERE user = ?1 AND device = ?2",
            params![user, device],
            |r| r.get(0),
        )?;
        let room = max.saturating_sub(have as usize);
        for p in packages.iter().take(room) {
            self.conn.execute(
                "INSERT INTO keypackages (user, device, data) VALUES (?1, ?2, ?3)",
                params![user, device, p],
            )?;
        }
        Ok(room.min(packages.len()))
    }

    /// Take one key package of a device; it is deleted.
    pub fn claim_keypackage(&self, user: &str, device: &str) -> DbResult<Option<Vec<u8>>> {
        let row: Option<(i64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT id, data FROM keypackages WHERE user = ?1 AND device = ?2 ORDER BY id LIMIT 1",
                params![user, device],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, data)) = row {
            self.conn
                .execute("DELETE FROM keypackages WHERE id = ?1", params![id])?;
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
        Ok(cleaned)
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
