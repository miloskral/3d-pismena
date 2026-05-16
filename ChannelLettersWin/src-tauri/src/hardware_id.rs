//
// hardware_id.rs — Windows equivalent of macOS HardwareID.swift.
//
// Reads `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` — a 36-char UUID
// generated once at Windows install. Stable across reboots and feature
// updates; changes only on a clean Windows reinstall (analogous to macOS
// IOPlatformUUID changing only after a hardware swap).
//
// Returned uppercased to match the macOS canonical form, so the same
// `payload.hw` value works for both platforms.
//

#[cfg(windows)]
pub fn machine_guid() -> String {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(key) = hklm.open_subkey("SOFTWARE\\Microsoft\\Cryptography") {
        if let Ok(guid) = key.get_value::<String, _>("MachineGuid") {
            return guid.trim().trim_matches('{').trim_matches('}').to_uppercase();
        }
    }
    // Fallback — extremely unlikely. Some hostile environments wipe the
    // registry value; the user will see an empty hw mismatch downstream.
    String::new()
}

#[cfg(not(windows))]
pub fn machine_guid() -> String {
    // Dev mode on macOS / Linux. Returns a deterministic placeholder so the
    // app can boot and run the UI; license HW check will fail in payload.hw.
    "00000000-0000-0000-0000-000000000000".to_string()
}
