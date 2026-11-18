// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Trair core: accounts, devices, MLS groups and the strict policy.
//!
//! Everything secret lives here and never in the app layer. The server is
//! treated as untrusted: it delivers bytes, but who is in a group and who
//! sent a message is decided only by MLS signatures and by device
//! certificates signed with the account key.

pub mod account;
pub mod error;
pub mod keypackages;
pub mod mls;
pub mod policy;

pub use account::{Account, DeviceCertificate, Fingerprint};
pub use error::{CoreError, Result};
pub use mls::{Device, Event, KeyPackageOffer, Member, Outgoing};
pub use policy::{Policy, Trust};

/// Version of this crate, as set in the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short product claim shown in the app and in server banners.
pub const SLOGAN: &str = "Private communication.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn slogan_is_unchanged() {
        assert_eq!(SLOGAN, "Private communication.");
    }
}
