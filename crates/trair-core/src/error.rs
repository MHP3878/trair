// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Errors returned by the core.

use thiserror::Error;

/// Everything that can go wrong in the core. The app turns these into
/// user-facing text; the strings here are for logs and tests.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// The strict policy refused the action. Nothing was sent.
    #[error("blocked by strict mode: {0}")]
    Blocked(String),
    /// A member or key package carries an invalid device certificate.
    /// This is always refused, strict mode or not.
    #[error("forged device certificate: {0}")]
    Forged(String),
    /// The group is not known on this device.
    #[error("unknown group {0}")]
    UnknownGroup(String),
    /// A group with this id already exists on this device.
    #[error("group {0} already exists")]
    GroupExists(String),
    /// A key package from the server is malformed, invalid or does not
    /// match what the server claimed.
    #[error("invalid key package: {0}")]
    InvalidKeyPackage(String),
    /// An incoming message was rejected (tampered, replayed, wrong epoch,
    /// not for us, ...).
    #[error("message rejected: {0}")]
    Rejected(String),
    /// User or device names must be short and simple.
    #[error("invalid name: {0}")]
    InvalidName(String),
    /// Internal error from the MLS library or the crypto provider.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Short form for results in this crate.
pub type Result<T> = std::result::Result<T, CoreError>;

pub(crate) fn internal<E: std::fmt::Debug>(e: E) -> CoreError {
    CoreError::Internal(format!("{e:?}"))
}
