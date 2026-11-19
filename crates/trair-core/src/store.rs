// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The chat database of one device.
//!
//! SQLCipher encrypts the whole file (pages, indexes, journal) with a
//! random 32-byte key that lives in the vault. Without the vault password
//! the file is noise; with a wrong key it does not open at all. Deleted
//! rows are overwritten (`secure_delete`), temporary data stays in memory.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, Transaction};
use zeroize::Zeroizing;

use crate::error::{CoreError, Result};

/// Version of the tables below.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    k TEXT PRIMARY KEY,
    v BLOB NOT NULL
) WITHOUT ROWID;
-- The OpenMLS key-value store: group states and private keys.
CREATE TABLE IF NOT EXISTS mls (
    k BLOB PRIMARY KEY,
    v BLOB NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS contacts (
    user TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    name TEXT NOT NULL,
    confirmed INTEGER NOT NULL,
    added INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS chats (
    id TEXT PRIMARY KEY,
    peer TEXT NOT NULL,
    state TEXT NOT NULL,
    created INTEGER NOT NULL,
    last INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    chat TEXT NOT NULL,
    sender TEXT NOT NULL,
    user TEXT NOT NULL,
    body TEXT NOT NULL,
    at INTEGER NOT NULL,
    outgoing INTEGER NOT NULL,
    missing INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_by_chat ON messages (chat, id);
-- Changes (commits) waiting for the server's answer, one per chat. Kept
-- so a change is never lost or applied twice across a restart.
CREATE TABLE IF NOT EXISTS outbox (
    chat TEXT PRIMARY KEY,
    epoch INTEGER NOT NULL,
    commit_bytes BLOB NOT NULL,
    recipients TEXT NOT NULL,
    welcome BLOB,
    welcome_to TEXT NOT NULL,
    removed TEXT NOT NULL
) WITHOUT ROWID;
-- Hashes of processed incoming messages, so a message delivered twice
-- (for example after a crash before the server was told) is noticed.
CREATE TABLE IF NOT EXISTS seen (
    hash BLOB PRIMARY KEY,
    at INTEGER NOT NULL
) WITHOUT ROWID;
";

/// An open chat database.
pub(crate) struct Store {
    conn: Connection,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

fn storage<E: std::fmt::Display>(e: E) -> CoreError {
    CoreError::Storage(e.to_string())
}

impl Store {
    /// Open (or create) the database at `path` with `key`.
    pub(crate) fn open(path: &Path, key: &[u8; 32]) -> Result<Store> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(storage)?;
        // A raw key: no password derivation inside SQLCipher, the key is
        // already random.
        let pragma = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex::encode(key)));
        conn.execute_batch(&pragma).map_err(storage)?;
        // SQLCipher would print decryption failures to stderr.
        conn.execute_batch("PRAGMA cipher_log_level = NONE;")
            .map_err(storage)?;
        // The first read shows whether the key fits.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|_| CoreError::Storage("wrong key or damaged chat database".into()))?;
        conn.execute_batch(
            "PRAGMA secure_delete = ON;
             PRAGMA temp_store = MEMORY;
             PRAGMA journal_mode = DELETE;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(storage)?;
        conn.execute_batch(SCHEMA).map_err(storage)?;
        let store = Store { conn };
        match store.meta_int("schema")? {
            None => store.set_meta_int("schema", SCHEMA_VERSION)?,
            Some(SCHEMA_VERSION) => {}
            Some(v) => {
                return Err(CoreError::Storage(format!(
                    "chat database version {v} is newer than this Trair"
                )));
            }
        }
        Ok(store)
    }

    fn meta_int(&self, k: &str) -> Result<Option<i64>> {
        match self
            .conn
            .query_row("SELECT v FROM meta WHERE k = ?1", [k], |r| {
                r.get::<_, i64>(0)
            }) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(storage(e)),
        }
    }

    fn set_meta_int(&self, k: &str, v: i64) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO meta (k, v) VALUES (?1, ?2)
                 ON CONFLICT (k) DO UPDATE SET v = excluded.v",
                rusqlite::params![k, v],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// A value from `meta`.
    pub(crate) fn meta(&self, k: &str) -> Result<Option<Vec<u8>>> {
        match self
            .conn
            .query_row("SELECT v FROM meta WHERE k = ?1", [k], |r| {
                r.get::<_, Vec<u8>>(0)
            }) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(storage(e)),
        }
    }

    /// Run `f` in one transaction: all of it is stored, or nothing.
    pub(crate) fn write<T>(&mut self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let tx = self.conn.transaction().map_err(storage)?;
        let out = f(&tx)?;
        tx.commit().map_err(storage)?;
        Ok(out)
    }

    /// For reading.
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// Set a `meta` value inside a transaction.
pub(crate) fn put_meta(tx: &Transaction, k: &str, v: &[u8]) -> Result<()> {
    tx.execute(
        "INSERT INTO meta (k, v) VALUES (?1, ?2)
         ON CONFLICT (k) DO UPDATE SET v = excluded.v",
        rusqlite::params![k, v],
    )
    .map_err(storage)?;
    Ok(())
}

pub(crate) fn sql<E: std::fmt::Display>(e: E) -> CoreError {
    storage(e)
}
