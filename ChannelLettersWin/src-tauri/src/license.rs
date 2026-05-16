//
// license.rs — Windows port of LicenseStore.swift verification logic.
//
// Token wire format (identical across Mac, Windows, JS gate):
//   "SLP1.<base64url(payload_json)>.<base64url(p256_raw_signature_64B)>"
//
// Public key is the same P-256 (X, Y) coordinates as on macOS — sourced from
// tools/secrets/public-key.jwk.json. The key MUST stay in sync with the JS
// gate's PUBLIC_KEY_JWK in channel-letters.html and the macOS LicenseStore.
//

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::EncodedPoint;
use serde::Deserialize;

// Same constants as LicenseStore.swift.
const PUBLIC_KEY_X_B64URL: &str = "M6E7adPhySLtQ7ZGhJDIiEUfu8xq6KVGLLSNx9OBYx0";
const PUBLIC_KEY_Y_B64URL: &str = "2azHn-ewL6_YMaAGLNG4A1mtIn8aGqP4iPG2Q1ufLCo";

#[derive(Debug, Deserialize)]
pub struct Payload {
    pub v: i32,
    pub email: String,
    pub exp: i64, // 0 = perpetual
    pub iat: i64,
    pub trial: bool,
    pub nonce: String,
    pub hw: Option<String>,
}

#[derive(Debug)]
pub enum Outcome {
    Ok(Payload),
    Expired(Payload),
    Invalid(&'static str), // reason matches JS gate i18n keys
}

pub fn verify(raw: &str, hw_uuid: &str) -> Outcome {
    let trimmed = raw.trim();
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() != 3 || parts[0] != "SLP1" {
        return Outcome::Invalid("format");
    }

    let payload_b64 = parts[1];
    let sig_b64 = parts[2];

    let payload_bytes = match URL_SAFE_NO_PAD.decode(payload_b64) {
        Ok(b) => b,
        Err(_) => return Outcome::Invalid("payload"),
    };
    let payload: Payload = match serde_json::from_slice(&payload_bytes) {
        Ok(p) => p,
        Err(_) => return Outcome::Invalid("payload"),
    };

    let sig_bytes = match URL_SAFE_NO_PAD.decode(sig_b64) {
        Ok(b) => b,
        Err(_) => return Outcome::Invalid("sig-decode"),
    };
    if sig_bytes.len() != 64 {
        return Outcome::Invalid("sig-decode");
    }

    let pub_key = match build_public_key() {
        Some(k) => k,
        None => return Outcome::Invalid("verify-error"),
    };

    let signature = match Signature::from_slice(&sig_bytes) {
        Ok(s) => s,
        Err(_) => return Outcome::Invalid("sig-decode"),
    };

    // We signed the base64url payload bytes (UTF-8), same as macOS does.
    if pub_key.verify(payload_b64.as_bytes(), &signature).is_err() {
        return Outcome::Invalid("signature");
    }

    // Hardware binding check.
    if let Some(hw) = payload.hw.as_ref() {
        if !hw.is_empty() && !hw.eq_ignore_ascii_case(hw_uuid) {
            return Outcome::Invalid("hw-mismatch");
        }
    }

    // Expiry check.
    let now = chrono_secs();
    if payload.exp != 0 && payload.exp < now {
        return Outcome::Expired(payload);
    }

    Outcome::Ok(payload)
}

fn build_public_key() -> Option<VerifyingKey> {
    let x = URL_SAFE_NO_PAD.decode(PUBLIC_KEY_X_B64URL).ok()?;
    let y = URL_SAFE_NO_PAD.decode(PUBLIC_KEY_Y_B64URL).ok()?;
    if x.len() != 32 || y.len() != 32 {
        return None;
    }
    let point = EncodedPoint::from_affine_coordinates(
        x.as_slice().into(),
        y.as_slice().into(),
        /* compress */ false,
    );
    VerifyingKey::from_encoded_point(&point).ok()
}

fn chrono_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
