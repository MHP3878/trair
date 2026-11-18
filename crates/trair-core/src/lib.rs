// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Trair core.
//!
//! Milestone M0 only provides the project skeleton. The MLS wrapper, the
//! strict security policy and the regression tests from the prototype
//! follow in milestone M1.

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
