//! Saved SFTP passwords in the Windows Credential Manager (generic credentials).
//!
//! The password blob is stored as UTF-16LE (the convention used by Windows tools,
//! so it is also visible/removable via "Credential Manager" in the Control Panel).

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::ptr;

    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn save(target: &str, user: &str, password: &str) -> Result<(), String> {
        let mut target_w = wide(target);
        let mut user_w = wide(user);
        let mut blob: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let cred = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target_w.as_mut_ptr(),
            Comment: ptr::null_mut(),
            LastWritten: FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: ptr::null_mut(),
            TargetAlias: ptr::null_mut(),
            UserName: user_w.as_mut_ptr(),
        };
        // SAFETY: every pointer in `cred` refers to a live, NUL-terminated (strings) or
        // correctly-sized (blob) buffer owned by this stack frame for the whole call.
        let ok = unsafe { CredWriteW(&cred, 0) };
        // Best-effort wipe of the plaintext copy.
        blob.iter_mut().for_each(|b| *b = 0);
        if ok == 0 {
            Err(format!(
                "Failed to save password to Credential Manager: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }

    pub fn load(target: &str) -> Option<String> {
        let target_w = wide(target);
        let mut out: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: `target_w` is NUL-terminated; `out` receives a buffer we free with CredFree.
        let ok = unsafe { CredReadW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0, &mut out) };
        if ok == 0 || out.is_null() {
            return None;
        }
        // SAFETY: on success `out` points to a valid CREDENTIALW whose blob has
        // `CredentialBlobSize` readable bytes until CredFree is called.
        let result = unsafe {
            let cred = &*out;
            let bytes = if cred.CredentialBlob.is_null() {
                &[][..]
            } else {
                std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize)
            };
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            String::from_utf16(&units).ok()
        };
        // SAFETY: `out` was allocated by CredReadW.
        unsafe { CredFree(out as *const core::ffi::c_void) };
        result
    }

    pub fn delete(target: &str) {
        let target_w = wide(target);
        // SAFETY: `target_w` is NUL-terminated. Failure (e.g. not found) is ignored.
        unsafe {
            CredDeleteW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn save(_target: &str, _user: &str, _password: &str) -> Result<(), String> {
        Err("Saving passwords is only supported on Windows".to_string())
    }
    pub fn load(_target: &str) -> Option<String> {
        None
    }
    pub fn delete(_target: &str) {}
}

/// Saves (or overwrites) the password for `target`.
pub fn save_password(target: &str, user: &str, password: &str) -> Result<(), String> {
    imp::save(target, user, password)
}

/// Returns the saved password for `target`, if any.
pub fn load_password(target: &str) -> Option<String> {
    imp::load(target)
}

/// Removes the saved password for `target` (no-op if absent).
pub fn delete_password(target: &str) {
    imp::delete(target)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn save_load_delete_round_trip_including_non_ascii() {
        let target = format!("tuif:sftp:__unit_test_{}", std::process::id());
        save_password(&target, "tester", "pässwörd-日本語").unwrap();
        assert_eq!(load_password(&target).as_deref(), Some("pässwörd-日本語"));
        save_password(&target, "tester", "second").unwrap();
        assert_eq!(load_password(&target).as_deref(), Some("second"));
        delete_password(&target);
        assert_eq!(load_password(&target), None);
    }
}
