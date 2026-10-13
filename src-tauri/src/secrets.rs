//! At-rest protection for the one secret Nova stores: the optional AI
//! provider API key.
//!
//! The key lives inside the persisted settings JSON. Before that JSON is
//! written to SQLite the key is encrypted with Windows DPAPI
//! (`CryptProtectData`), scoped to the current Windows user, and stored as
//! `dpapi:<hex>`. Reading it back decrypts transparently. On any failure the
//! value is left untouched — Nova degrades to plaintext rather than losing the
//! user's key, and the Settings → Privacy panel states which mode is in use.
//!
//! This protects the key from other users and from backups/sync tools that
//! copy the database file; it is not a substitute for the OS credential store
//! for high-value secrets (documented in the README).

pub const PREFIX: &str = "dpapi:";

/// Whether secrets can be protected on this platform.
pub fn available() -> bool {
    cfg!(windows)
}

/// Encrypt a value for the current Windows user. Returns the original string
/// when protection is unavailable or fails.
pub fn protect(plaintext: &str) -> String {
    if plaintext.is_empty() || plaintext.starts_with(PREFIX) {
        return plaintext.to_string();
    }
    match protect_inner(plaintext) {
        Some(bytes) => format!("{PREFIX}{}", to_hex(&bytes)),
        None => plaintext.to_string(),
    }
}

/// Decrypt a stored value. Values without the prefix (older databases, or a
/// machine where protection failed) are returned unchanged.
pub fn unprotect(stored: &str) -> String {
    let Some(hex) = stored.strip_prefix(PREFIX) else {
        return stored.to_string();
    };
    match from_hex(hex).and_then(|bytes| unprotect_inner(&bytes)) {
        Some(plain) => plain,
        None => {
            // The blob was written by another Windows user or is corrupt:
            // report it as missing rather than crashing the app.
            eprintln!("[nova] stored API key could not be decrypted (different Windows user?)");
            String::new()
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[cfg(windows)]
fn protect_inner(plaintext: &str) -> Option<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

    let mut data = plaintext.as_bytes().to_vec();
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(&input, None, None, None, None, 0, &mut output).ok()?;
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        let _ = windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData as *mut core::ffi::c_void,
        )));
    }
    Some(bytes)
}

#[cfg(windows)]
fn unprotect_inner(blob: &[u8]) -> Option<String> {
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let mut data = blob.to_vec();
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output).ok()?;
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        let _ = windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData as *mut core::ffi::c_void,
        )));
    }
    String::from_utf8(bytes).ok()
}

#[cfg(not(windows))]
fn protect_inner(_plaintext: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(not(windows))]
fn unprotect_inner(_blob: &[u8]) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let bytes = vec![0u8, 1, 15, 16, 255];
        assert_eq!(from_hex(&to_hex(&bytes)), Some(bytes));
        assert_eq!(from_hex("abc"), None, "odd length is rejected");
        assert_eq!(from_hex("zz"), None, "non-hex is rejected");
    }

    #[test]
    fn plaintext_values_pass_through() {
        assert_eq!(unprotect("sk-existing"), "sk-existing");
        assert_eq!(unprotect(""), "");
        // Corrupt blobs must not panic and must not leak garbage.
        assert_eq!(unprotect("dpapi:nothex"), "");
    }

    /// A stand-in plaintext used only to prove at-rest protection works.
    const SAMPLE_PLAINTEXT: &str = "test-value-1234567890";

    #[test]
    fn protected_value_roundtrips_for_the_current_user() {
        if !available() {
            // Protection is Windows-only; on other platforms values pass through.
            assert_eq!(protect(SAMPLE_PLAINTEXT), SAMPLE_PLAINTEXT);
            return;
        }
        let stored = protect(SAMPLE_PLAINTEXT);
        assert!(stored.starts_with(PREFIX), "expected a DPAPI blob");
        assert_ne!(stored, SAMPLE_PLAINTEXT);
        assert!(!stored.contains(SAMPLE_PLAINTEXT));
        assert_eq!(unprotect(&stored), SAMPLE_PLAINTEXT);
        // Protecting twice must not double-wrap.
        assert_eq!(protect(&stored), stored);
    }
}
