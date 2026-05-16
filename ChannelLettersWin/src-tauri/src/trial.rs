//
// trial.rs — Windows port of TrialService.swift.
//
// Calls the SAME Vercel endpoint as the macOS app (https://lumisign3d.com/api/trial)
// with SHA-256(MachineGuid) as the anchor. Fail-closed logic is identical:
//
//   1. Server reachable → use live snapshot, cache it.
//   2. Server unreachable AND cache ≤ 48 h old → adjust the cached daysLeft
//      by elapsed time and serve that.
//   3. Otherwise → daysLeft = 0, expired = true (JS gate locks STL export).
//
// This stops the "block lumisign3d.com + wipe local store" cheat.
//

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::keychain::{self, TrialCache};

pub const ENDPOINT: &str = "https://lumisign3d.com/api/trial";
pub const TIMEOUT_SECS: u64 = 3;
pub const MAX_CACHE_STALENESS_SEC: i64 = 48 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(rename = "trialStartSec")]
    pub trial_start_sec: i64,
    #[serde(rename = "daysLeft")]
    pub days_left: i64,
    #[serde(rename = "trialDays")]
    pub trial_days: i64,
    pub expired: bool,
    #[serde(rename = "nowSec")]
    pub now_sec: i64,
}

pub fn sha256_hex(s: &str) -> String {
    let digest = Sha256::digest(s.as_bytes());
    hex::encode(digest)
}

/// Single entry point used by `main.rs` when building the SLP_INJECTED bridge.
pub fn resolve(hw_uuid: &str, version: &str) -> Snapshot {
    let now = now_sec();

    // 1. Try server.
    if let Some(fresh) = fetch_blocking(hw_uuid, version) {
        persist_cache(&fresh, now);
        return fresh;
    }

    // 2. Cached snapshot ≤ 48 h?
    if let Some(cached) = read_cache() {
        let age = now - cached.saved_at_sec;
        if age <= MAX_CACHE_STALENESS_SEC {
            if let Ok(snap) = serde_json::from_str::<Snapshot>(&cached.snapshot_json) {
                let elapsed_days = (age as f64) / 86_400.0;
                let adjusted = ((snap.days_left as f64) - elapsed_days).ceil().max(0.0) as i64;
                eprintln!(
                    "[trial] server unreachable; cache aged {}s; daysLeft {} → {}",
                    age, snap.days_left, adjusted
                );
                return Snapshot {
                    trial_start_sec: snap.trial_start_sec,
                    days_left: adjusted,
                    trial_days: snap.trial_days,
                    expired: adjusted == 0,
                    now_sec: now,
                };
            }
        } else {
            eprintln!(
                "[trial] server unreachable AND cache too old ({}s > {}s) — failing closed",
                age, MAX_CACHE_STALENESS_SEC
            );
        }
    } else {
        eprintln!("[trial] server unreachable AND no cache — failing closed");
    }

    // 3. Fail closed.
    Snapshot {
        trial_start_sec: 0,
        days_left: 0,
        trial_days: 14,
        expired: true,
        now_sec: now,
    }
}

fn fetch_blocking(hw_uuid: &str, version: &str) -> Option<Snapshot> {
    let hw_hash = sha256_hex(hw_uuid);
    let body = serde_json::json!({
        "hwHash": hw_hash,
        "version": version,
    });

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build();

    let resp = agent.post(ENDPOINT).send_json(body).ok()?;
    if resp.status() != 200 {
        return None;
    }

    #[derive(Deserialize)]
    struct Wire {
        ok: Option<bool>,
        #[serde(rename = "trialStartSec")] trial_start_sec: Option<i64>,
        #[serde(rename = "daysLeft")]      days_left: Option<i64>,
        #[serde(rename = "trialDays")]     trial_days: Option<i64>,
        expired: Option<bool>,
        #[serde(rename = "nowSec")]        now_sec: Option<i64>,
    }
    let w: Wire = resp.into_json().ok()?;
    if w.ok != Some(true) { return None; }

    Some(Snapshot {
        trial_start_sec: w.trial_start_sec?,
        days_left: w.days_left?,
        trial_days: w.trial_days?,
        expired: w.expired?,
        now_sec: w.now_sec?,
    })
}

fn persist_cache(snap: &Snapshot, saved_at: i64) {
    let snapshot_json = match serde_json::to_string(snap) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut store = keychain::load();
    store.trial_cache = Some(TrialCache { snapshot_json, saved_at_sec: saved_at });
    let _ = keychain::save(&store);
}

fn read_cache() -> Option<TrialCache> {
    keychain::load().trial_cache
}

fn now_sec() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Serialize a Snapshot as a JS object literal (matches LicenseStore.jsLiteral
/// in Swift). Used to build the SLP_INJECTED bridge.
pub fn to_js_literal(snap: &Snapshot) -> String {
    format!(
        "{{trialStartSec:{},daysLeft:{},trialDays:{},expired:{},nowSec:{}}}",
        snap.trial_start_sec,
        snap.days_left,
        snap.trial_days,
        if snap.expired { "true" } else { "false" },
        snap.now_sec,
    )
}
