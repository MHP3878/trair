// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! What depends on the operating system.
//!
//! On Windows the vault file gets a second lock: DPAPI encrypts it for the
//! current Windows user. A copy of the file is then useless on another PC
//! or for another user, even with the password. (That is also why a new
//! Windows installation loses the account, as the first start says.)
//! Other systems only run the tests; there the file is stored as the core
//! sealed it.

use std::path::Path;

/// Marks a vault wrapped with DPAPI.
#[cfg_attr(not(windows), allow(dead_code))]
const DPAPI_MAGIC: &[u8; 8] = b"TRAIRW1\0";

/// Create the data folder, readable only by this user where the system
/// allows it.
pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(windows)]
pub use windows::{pick_image, protect_windows, unwrap, wrap};

/// Let the person choose a picture file. Only on Windows; elsewhere the
/// app passes a path itself (tests).
#[cfg(not(windows))]
pub fn pick_image() -> Option<Option<String>> {
    None
}

#[cfg(not(windows))]
pub fn wrap(inner: &[u8]) -> Result<Vec<u8>, ()> {
    Ok(inner.to_vec())
}

#[cfg(not(windows))]
pub fn unwrap(file: &[u8]) -> Result<Vec<u8>, ()> {
    if file.starts_with(DPAPI_MAGIC) {
        // A Windows vault cannot be opened anywhere else.
        return Err(());
    }
    Ok(file.to_vec())
}

/// Screenshot block: only on Windows.
#[cfg(not(windows))]
pub fn protect_windows(_on: bool) -> Option<u32> {
    None
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use super::DPAPI_MAGIC;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LocalFree};
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
        WDA_MONITOR, WDA_NONE,
    };
    use windows_sys::core::BOOL;

    /// Extra input for DPAPI, so other programs of the same user cannot
    /// decrypt the blob by just calling DPAPI with no entropy.
    const ENTROPY: &[u8] = b"trair/vault-dpapi/v1";

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    /// Copy the output of DPAPI and give its memory back to Windows.
    ///
    /// # Safety
    /// `out` must be a blob filled by a successful DPAPI call.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: DPAPI returned `cbData` valid bytes at `pbData`, allocated
        // with LocalAlloc; we copy them and free the block exactly once.
        unsafe {
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            std::ptr::write_bytes(out.pbData, 0, out.cbData as usize);
            LocalFree(out.pbData.cast());
            bytes
        }
    }

    pub fn wrap(inner: &[u8]) -> Result<Vec<u8>, ()> {
        let input = blob(inner);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: all pointers point to live buffers of the given sizes;
        // DPAPI only reads `input` and `entropy` and fills `out`.
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err(());
        }
        // SAFETY: `out` was filled by the successful call above.
        let protected = unsafe { take(out) };
        let mut file = DPAPI_MAGIC.to_vec();
        file.extend_from_slice(&protected);
        Ok(file)
    }

    pub fn unwrap(file: &[u8]) -> Result<Vec<u8>, ()> {
        let Some(protected) = file.strip_prefix(DPAPI_MAGIC.as_slice()) else {
            // On Windows every vault must carry the DPAPI layer.
            return Err(());
        };
        let input = blob(protected);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `wrap`.
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err(());
        }
        // SAFETY: `out` was filled by the successful call above.
        Ok(unsafe { take(out) })
    }

    struct Search {
        pid: u32,
        affinity: u32,
        set: u32,
    }

    unsafe extern "system" fn visit(hwnd: HWND, param: LPARAM) -> BOOL {
        // SAFETY: `param` is the address of the `Search` that
        // `protect_windows` keeps alive for the whole EnumWindows call.
        let search = unsafe { &mut *(param as *mut Search) };
        let mut pid = 0u32;
        // SAFETY: `hwnd` comes from EnumWindows; `pid` is a valid out pointer.
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == search.pid {
            // SAFETY: plain call on a window of this process.
            let mut ok = unsafe { SetWindowDisplayAffinity(hwnd, search.affinity) };
            if ok == 0 && search.affinity == WDA_EXCLUDEFROMCAPTURE {
                // Before Windows 10 2004: the window shows black instead.
                // SAFETY: as above.
                ok = unsafe { SetWindowDisplayAffinity(hwnd, WDA_MONITOR) };
            }
            if ok != 0 {
                search.set += 1;
            }
        }
        1
    }

    /// The Windows "open file" dialog for pictures. `Some(None)`: the
    /// person cancelled.
    pub fn pick_image() -> Option<Option<String>> {
        use windows_sys::Win32::UI::Controls::Dialogs::{
            GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
            OPENFILENAMEW,
        };
        let wide = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        // "Name\0pattern\0...\0\0"
        let filter = wide("Bilder (PNG, JPG, GIF, BMP)\0*.png;*.jpg;*.jpeg;*.gif;*.bmp\0\0");
        let title = wide("Profilbild wählen\0");
        let mut file = vec![0u16; 1024];
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            lpstrFilter: filter.as_ptr(),
            lpstrFile: file.as_mut_ptr(),
            nMaxFile: file.len() as u32,
            lpstrTitle: title.as_ptr(),
            Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            ..Default::default()
        };
        // The dialog uses shell (COM) objects; this thread may not have
        // COM yet.
        use windows_sys::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        };
        // SAFETY: no pointers; paired with CoUninitialize below on success.
        let com = unsafe {
            CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        };
        // SAFETY: every pointer in `ofn` points to a live, NUL-terminated
        // buffer of the stated size for the whole call.
        let chosen = unsafe { GetOpenFileNameW(&mut ofn) } != 0;
        if com >= 0 {
            // SAFETY: CoInitializeEx succeeded on this thread.
            unsafe { CoUninitialize() };
        }
        if !chosen {
            return Some(None);
        }
        let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
        Some(String::from_utf16(&file[..len]).ok())
    }

    /// Hide all top-level windows of this process from screenshots and
    /// screen recording (`on`), or show them again. Returns how many
    /// windows were changed.
    pub fn protect_windows(on: bool) -> Option<u32> {
        let mut search = Search {
            // SAFETY: no preconditions.
            pid: unsafe { GetCurrentProcessId() },
            affinity: if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE },
            set: 0,
        };
        // SAFETY: `visit` matches WNDENUMPROC and `search` outlives the call.
        unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
        Some(search.set)
    }
}
