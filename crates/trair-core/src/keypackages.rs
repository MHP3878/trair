// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Key package supply (problem 1 from the planning document).
//!
//! Every add consumes one one-time key package. The server reports how
//! many are left; below [`REFILL_BELOW`] the device uploads new ones up to
//! [`TARGET`]. In addition each device keeps one last-resort package on
//! the server, handed out only when the supply is empty. After joining a
//! group through the last-resort package the device updates its own keys
//! in that group right away, so the reused package is not a weak point
//! for long.

/// Refill when fewer than this many one-time packages are left.
pub const REFILL_BELOW: usize = 20;

/// Fill up to this many one-time packages.
pub const TARGET: usize = 50;

/// How many one-time packages to upload, given how many the server still has.
pub fn refill_count(available: usize) -> usize {
    if available < REFILL_BELOW {
        TARGET - available
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refill_rules() {
        assert_eq!(refill_count(0), TARGET);
        assert_eq!(refill_count(REFILL_BELOW - 1), TARGET - REFILL_BELOW + 1);
        assert_eq!(refill_count(REFILL_BELOW), 0);
        assert_eq!(refill_count(TARGET), 0);
    }
}
