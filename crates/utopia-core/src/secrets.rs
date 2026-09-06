//! Encryption at rest for credentials ("sealing").
//!
//! LLM API keys, Ask-the-Data connection strings, tokens in source configs, and API
//! sources' push keys used to be stored in plaintext — anyone who could read the database
//! (or a backup of it) had every external credential. These values are now sealed with
//! AES-256-GCM before they reach the database, with the key kept out of the database:
//! the `UTOPIA_SECRET_KEY` environment variable, or `secret.key` generated under the data
//! directory on first start. The threat model is "a database leak is not a credential
//! leak": a pg_dump, or a read-only database account, only gets ciphertext. Someone who
//! can read both the data directory and the database (i.e. who has logged into the
//! server) is out of scope here — that's a server access-control matter.
//!
//! **Format**: `enc:v1:` + base64(nonce(12) ‖ ciphertext+tag). A value with no prefix is
//! read as plaintext (a pre-upgrade legacy row); at startup, callers of [`crate::secrets`]
//! (the store's backfill) seal those. Sealing is idempotent: sealing an already-sealed
//! value returns it unchanged, so neither the read path nor the write path needs to check first.
//!
//! **One key per process**: [`init`] is called once at service startup; before it's
//! initialized, [`seal`] returns its input unchanged and [`open`] only accepts plaintext —
//! this is the path taken by unit tests and tools that don't run the service.
//!
//! Key rotation is out of scope for this version: rotating the key means reading with the
//! old key and writing back with the new one, as an explicit migration.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use std::sync::OnceLock;

pub const PREFIX: &str = "enc:v1:";
const NONCE_LEN: usize = 12;

static SEALER: OnceLock<Aes256Gcm> = OnceLock::new();

/// 装钥匙。只认第一次；再调返回 false，钥匙不变
pub fn init(key: [u8; 32]) -> bool {
    SEALER
        .set(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key)))
        .is_ok()
}

pub fn is_ready() -> bool {
    SEALER.get().is_some()
}

/// 生一把新钥匙（CSPRNG 32 字节）
pub fn generate_key() -> [u8; 32] {
    Aes256Gcm::generate_key(OsRng).into()
}

/// 钥匙的文本形态：64 位十六进制或 44 位 base64，都收
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

/// 封印。幂等：已封印的原样返回；没装钥匙时原样返回（明文落库，和从前一样）
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

/// 开封。没有前缀的按明文原样返回（升级前的旧行）；有前缀而钥匙不对或被改过，报错——
/// 静默回一段乱码会让下游拿它去调外部接口，错得更远
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

/// 把 JSON 对象里给定键的字符串值封印（就地）。非字符串值不动
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

/// [`seal_json_keys`] 的反向
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
        // 测试二进制里只装一次；后面的测试拿同一把
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
