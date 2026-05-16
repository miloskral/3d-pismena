//
// bundle.rs — Decrypts the AES-256-GCM channel-letters.enc into a UTF-8
// HTML string at startup. Identical wire format to the macOS app:
//
//     bytes 0..11   = 12-byte GCM nonce
//     bytes 12..27  = 16-byte GCM authentication tag
//     bytes 28..    = ciphertext
//
// Key derivation: SHA-256(PASSPHRASE) → 32 raw bytes. The passphrase MUST
// match `encrypt-bundle.mjs` in the project root and `encryptionPassphrase`
// in WebViewHost.swift. Rotating the key means changing all three.
//

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine as _;
use sha2::{Digest, Sha256};

const PASSPHRASE: &str =
    "ChannelLetters-bundle-v1-k4S2nP9wQzL7eY3hB6vR8jX1uM5tA0fC";

/// Decrypt the bundled `.enc` blob and return the plaintext HTML.
/// `logo_png` is inlined as a data: URL so the HTML can render the logo
/// without needing relative file access (mirrors the macOS Ikonka-S.png
/// inlining in WebViewHost.swift::decryptedHTMLString).
pub fn decrypt_html(enc: &[u8], logo_png: Option<&[u8]>) -> Result<String, String> {
    if enc.len() < 28 {
        return Err(format!("bundle too short: {} bytes", enc.len()));
    }
    let nonce_bytes = &enc[0..12];
    let tag_bytes = &enc[12..28];
    let ct_bytes = &enc[28..];

    // AES-GCM in the `aes-gcm` crate expects `ciphertext || tag` concatenated.
    let mut ct_with_tag = Vec::with_capacity(ct_bytes.len() + tag_bytes.len());
    ct_with_tag.extend_from_slice(ct_bytes);
    ct_with_tag.extend_from_slice(tag_bytes);

    let mut hasher = Sha256::new();
    hasher.update(PASSPHRASE.as_bytes());
    let key_bytes = hasher.finalize();
    let key: &Key<Aes256Gcm> = Key::<Aes256Gcm>::from_slice(key_bytes.as_slice());
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ct_with_tag.as_ref())
        .map_err(|e| format!("AES-GCM open failed: {e}"))?;

    let mut html = String::from_utf8(plaintext)
        .map_err(|e| format!("decrypted bytes are not valid UTF-8: {e}"))?;

    if let Some(png) = logo_png {
        let b64 = base64::engine::general_purpose::STANDARD.encode(png);
        let data_url = format!("data:image/png;base64,{b64}");
        html = html.replace("Ikonka%20S.png", &data_url);
        html = html.replace("Ikonka S.png", &data_url);
        // The Win build distributes logo as "logo.png" — handle both names.
        html = html.replace("logo.png", &data_url);
    }

    Ok(html)
}
