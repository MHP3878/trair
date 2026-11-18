// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The four C functions the app calls. Nothing else is exported.
//!
//! Memory rule: every buffer that crosses the door is allocated here with
//! [`trair_alloc`] (or returned by [`trair_call`]) and given back with
//! [`trair_free`], which overwrites it with zeros first. The app writes
//! its request into a buffer from `trair_alloc`, so the password it
//! contains is wiped as soon as the app frees that buffer.

#![allow(unsafe_code)]

use std::panic::{AssertUnwindSafe, catch_unwind};

use zeroize::Zeroize;

use crate::api;

/// Version of this C interface. The app refuses a library with another
/// version.
pub const ABI_VERSION: u32 = 1;

/// Version of the C interface, see [`ABI_VERSION`].
#[unsafe(no_mangle)]
pub extern "C" fn trair_abi_version() -> u32 {
    ABI_VERSION
}

/// A zeroed buffer of `len` bytes, or null for `len == 0` or more than
/// [`api::MAX_REQUEST`] bytes. Give it back with [`trair_free`].
#[unsafe(no_mangle)]
pub extern "C" fn trair_alloc(len: usize) -> *mut u8 {
    if len == 0 || len > api::MAX_REQUEST {
        return std::ptr::null_mut();
    }
    Box::into_raw(vec![0u8; len].into_boxed_slice()).cast()
}

/// Overwrite a buffer with zeros and release it.
///
/// # Safety
/// `ptr` must come from [`trair_alloc`] or [`trair_call`] with exactly
/// this `len` (for `trair_call`: 4 plus the length it reports), and must
/// not be used afterwards. Null is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trair_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: by the contract above, `ptr` is a boxed slice of `len` bytes
    // that we handed out and that nobody uses any more.
    let mut buffer = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) };
    buffer.zeroize();
}

/// Handle one JSON request of `len` bytes at `request`. Returns a buffer
/// that starts with the answer length as 4 bytes little endian, followed
/// by the JSON answer. Free it with `trair_free(ptr, 4 + length)`.
/// Returns null only if `request` is null.
///
/// # Safety
/// `request` must point to `len` readable bytes for the whole call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trair_call(request: *const u8, len: usize) -> *mut u8 {
    if request.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: by the contract above.
    let input = unsafe { std::slice::from_raw_parts(request, len) };
    let answer = catch_unwind(AssertUnwindSafe(|| api::handle(input)))
        .unwrap_or_else(|_| br#"{"ok":false,"error":"internal"}"#.to_vec());
    let mut out = Vec::with_capacity(4 + answer.len());
    out.extend_from_slice(&(answer.len() as u32).to_le_bytes());
    out.extend_from_slice(&answer);
    Box::into_raw(out.into_boxed_slice()).cast()
}
