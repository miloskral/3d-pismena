//
// keychain.rs — Windows equivalent of macOS Keychain.swift.
//
// On macOS we use kSec generic-password items. On Windows we use DPAPI
// (CryptProtectData / CryptUnprotectData) with the LOCAL_MACHINE flag DISABLED
// — meaning the encrypted blob can only be unsealed by the same Windows user
// account on the same Windows install. Storage is a single file at
// `%LOCALAPPDATA%\ChannelLetters\store.bin` containing a JSON object of
// {license, trialCache} encrypted as one blob.
//
// Why DPAPI and not Credential Manager? Credential Manager is fine for short
// secrets but caps at 2560 bytes per item; the trial-cache JSON snapshot is
// well within that, and we'd need TWO items (license + trial). DPAPI is
// simpler — one encrypted file the user must actively delete to wipe (raises
// the bar for trial-reset cheats just like the macOS Keychain does).
//

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    pub license: Option<String>,
    pub trial_cache: Option<TrialCache>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TrialCache {
    pub snapshot_json: String,
    pub saved_at_sec: i64,
}

pub fn store_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // Dev / non-Windows fallback: a temp dir per platform.
            std::env::temp_dir()
        });
    let dir = base.join("ChannelLetters");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("store.bin")
}

#[cfg(windows)]
pub fn load() -> Store {
    let path = store_path();
    let Ok(blob) = std::fs::read(&path) else {
        return Store::default();
    };
    match dpapi_unprotect(&blob) {
        Ok(plaintext) => serde_json::from_slice(&plaintext).unwrap_or_default(),
        Err(_) => Store::default(),
    }
}

#[cfg(windows)]
pub fn save(store: &Store) -> Result<(), String> {
    let plaintext = serde_json::to_vec(store).map_err(|e| e.to_string())?;
    let blob = dpapi_protect(&plaintext)?;
    std::fs::write(store_path(), blob).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn load() -> Store {
    let path = store_path();
    let Ok(blob) = std::fs::read(&path) else {
        return Store::default();
    };
    serde_json::from_slice(&blob).unwrap_or_default()
}

#[cfg(not(windows))]
pub fn save(store: &Store) -> Result<(), String> {
    let plaintext = serde_json::to_vec(store).map_err(|e| e.to_string())?;
    std::fs::write(store_path(), plaintext).map_err(|e| e.to_string())
}

// ---- DPAPI wrappers ------------------------------------------------------

#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Foundation::HLOCAL;
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    unsafe {
        CryptProtectData(
            &mut input,
            None,
            None,
            None,
            None,
            0,
            &mut output,
        )
        .map_err(|e| format!("CryptProtectData failed: {e:?}"))?;

        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let owned = slice.to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut _));
        Ok(owned)
    }
}

#[cfg(windows)]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Foundation::HLOCAL;
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    unsafe {
        CryptUnprotectData(
            &mut input,
            None,
            None,
            None,
            None,
            0,
            &mut output,
        )
        .map_err(|e| format!("CryptUnprotectData failed: {e:?}"))?;

        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let owned = slice.to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut _));
        Ok(owned)
    }
}
