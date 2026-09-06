//! Encryption at rest for credentials (sealing).
//!
//! LLM API keys, data-source connection strings, tokens in source configs, and push
//! secrets for API sources stored in the database used to be plaintext — anyone who
//! could read the database (or a backup of it) got every external credential. Now
//! these values are sealed with AES-256-GCM before hitting the database, with the key
//! kept out of the database: the `UTOPIA_SECRET_KEY` environment variable, or a
//! `secret.key` generated under the data directory on first startup. The threat model
//! is "a leaked database doesn't mean leaked credentials" — a pg_dump, or a read-only
//! database account, gets you ciphertext. Someone who can read both the data directory
//! and the database (i.e. who has logged into the server) is out of scope here — that's
//! server access control's job.
//!
//! **Format**: `enc:v1:` + base64(nonce(12) ‖ ciphertext+tag). Values without the prefix
//! are read as plaintext (legacy rows written before the upgrade); callers of
//! [`crate::secrets`] at startup (the store's backfill) seal them in. Sealing is
//! idempotent: sealing an already-sealed value returns it unchanged, so neither the read
//! nor write path needs to check first.
//!
//! **One key per process**: the service calls [`init`] once at startup; before it's
//! initialized, [`seal`] returns its input unchanged and [`open`] only accepts
//! plaintext — this is the path unit tests and service-less tools take.
//!
//! Key rotation isn't in this version: rotating a key means reading with the old key and
//! writing back with the new one, which is an explicit migration.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use std::sync::OnceLock;

pub const PREFIX: &str = "enc:v1:";
const NONCE_LEN: usize = 12;

static SEALER: OnceLock<Aes256Gcm> = OnceLock::new();

/// Loads the key. Only the first call counts; subsequent calls return false and the
/// key is left unchanged
pub fn init(key: [u8; 32]) -> bool {
    SEALER
        .set(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key)))
        .is_ok()
}

pub fn is_ready() -> bool {
    SEALER.get().is_some()
}

/// Generates a fresh key (32 CSPRNG bytes)
pub fn generate_key() -> [u8; 32] {
    Aes256Gcm::generate_key(OsRng).into()
}

/// Text form of the key: 64-char hex or 44-char base64, either is accepted
pub fn parse_key(text: &str) -> Option<[u8; 32]> {
    let t = text.trim();
    let bytes: Vec<u8> = if t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        (0..64)
            .step_by(2)
            .map(|i| u8::from_str_radix(&t[i..i + 2], 16).ok())
            .collect::<Option<Vec<u8>>>()?
    } else {
        B64.decode(t).ok()?
    };
    bytes.try_into().ok()
}

pub fn key_to_hex(key: &[u8; 32]) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn is_sealed(stored: &str) -> bool {
    stored.starts_with(PREFIX)
}

/// Seals. Idempotent: an already-sealed value returns unchanged; with no key loaded,
/// returns unchanged too (plaintext hits the database, same as before)
pub fn seal(plain: &str) -> String {
    if is_sealed(plain) {
        return plain.to_string();
    }
    let Some(cipher) = SEALER.get() else {
        return plain.to_string();
    };
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut out = cipher
        .encrypt(&nonce, plain.as_bytes())
        .expect("AES-GCM encryption does not fail on in-memory input");
    let mut bytes = nonce.to_vec();
    bytes.append(&mut out);
    format!("{PREFIX}{}", B64.encode(bytes))
}

/// Opens. Without the prefix, returns unchanged as plaintext (legacy rows from before
/// the upgrade); with the prefix but a wrong key or tampered value, errors out — silently
/// returning garbage would let downstream code use it to call an external API, which
/// fails in a worse way
pub fn open(stored: &str) -> anyhow::Result<String> {
    let Some(body) = stored.strip_prefix(PREFIX) else {
        return Ok(stored.to_string());
    };
    let Some(cipher) = SEALER.get() else {
        anyhow::bail!("a sealed secret was read before the secret key was loaded");
    };
    let bytes = B64
        .decode(body)
        .map_err(|_| anyhow::anyhow!("a sealed secret is not valid base64"))?;
    if bytes.len() < NONCE_LEN {
        anyhow::bail!("a sealed secret is too short");
    }
    let (nonce, ct) = bytes.split_at(NONCE_LEN);
    let plain = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| anyhow::anyhow!("a sealed secret does not open with this key (wrong UTOPIA_SECRET_KEY, or the value was tampered with)"))?;
    String::from_utf8(plain).map_err(|_| anyhow::anyhow!("a sealed secret is not UTF-8"))
}

pub fn seal_opt(plain: Option<&str>) -> Option<String> {
    plain.map(seal)
}

pub fn open_opt(stored: Option<&str>) -> anyhow::Result<Option<String>> {
    stored.map(open).transpose()
}

/// Seals the string values of given keys in a JSON object (in place). Non-string
/// values are left alone
pub fn seal_json_keys(value: &mut serde_json::Value, keys: &[&str]) {
    if let Some(obj) = value.as_object_mut() {
        for key in keys {
            if let Some(serde_json::Value::String(s)) = obj.get(*key) {
                let sealed = seal(s);
                obj.insert((*key).to_string(), serde_json::Value::String(sealed));
            }
        }
    }
}

/// The inverse of [`seal_json_keys`]
pub fn open_json_keys(value: &mut serde_json::Value, keys: &[&str]) -> anyhow::Result<()> {
    if let Some(obj) = value.as_object_mut() {
        for key in keys {
            if let Some(serde_json::Value::String(s)) = obj.get(*key) {
                let opened = open(s)?;
                obj.insert((*key).to_string(), serde_json::Value::String(opened));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() {
        // Loaded only once within the test binary; later tests reuse the same key
        let _ = init([7u8; 32]);
    }

    #[test]
    fn a_secret_round_trips_and_never_repeats() {
        ready();
        let a = seal("sk-live-123");
        let b = seal("sk-live-123");
        assert!(is_sealed(&a));
        assert_ne!(a, b, "a fresh nonce every time");
        assert_eq!(open(&a).unwrap(), "sk-live-123");
        assert_eq!(open(&b).unwrap(), "sk-live-123");
    }

    #[test]
    fn sealing_is_idempotent_and_plaintext_passes_through() {
        ready();
        let once = seal("token");
        assert_eq!(seal(&once), once, "already sealed stays as it is");
        assert_eq!(open("legacy-plain").unwrap(), "legacy-plain");
    }

    #[test]
    fn a_tampered_value_does_not_open() {
        ready();
        let sealed = seal("secret");
        let mut broken = sealed.clone();
        broken.pop();
        broken.push(if sealed.ends_with('A') { 'B' } else { 'A' });
        assert!(open(&broken).is_err());
        assert!(open("enc:v1:not-base64!").is_err());
    }

    #[test]
    fn json_keys_are_sealed_in_place() {
        ready();
        let mut cfg = serde_json::json!({ "token": "t0k", "url": "https://x", "n": 3 });
        seal_json_keys(&mut cfg, &["token", "url_missing", "n"]);
        assert!(is_sealed(cfg["token"].as_str().unwrap()));
        assert_eq!(cfg["url"], "https://x");
        assert_eq!(cfg["n"], 3);
        open_json_keys(&mut cfg, &["token"]).unwrap();
        assert_eq!(cfg["token"], "t0k");
    }

    #[test]
    fn keys_parse_from_hex_and_base64() {
        let key = generate_key();
        assert_eq!(parse_key(&key_to_hex(&key)), Some(key));
        assert_eq!(parse_key(&B64.encode(key)), Some(key));
        assert_eq!(parse_key("too-short"), None);
    }
}
