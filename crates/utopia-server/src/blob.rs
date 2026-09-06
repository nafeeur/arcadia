//! BlobStore: the read/write seam for raw file bytes.
//!
//! Content-addressed — the key is the sha256 of the file's content, so the interface has no
//! notion of a "path": locally it's a flat pre-bucketing directory, in object storage it's the
//! object key, and any KV store can implement it. Idempotency, dedup and immutability (the
//! fingerprint changes when the content does, and an old version is never overwritten — the
//! material basis for "replay has something to replay") all come free from "content is the
//! address".
//!
//! The only implementation today is local disk (data/files/{sha256}). Wiring up object
//! storage/network drives later (P5 connectors, shared storage across multiple instances) only
//! needs a new implementation — callers of ingest/upload/parse/replay don't change a line. The
//! `UTOPIA_BLOB_BACKEND` config knob is reserved for this; it currently only accepts "local".

use std::path::PathBuf;

#[async_trait::async_trait]
pub trait BlobStore: Send + Sync {
    /// Idempotent write: skipped if the same fingerprint already exists.
    async fn put(&self, sha256: &str, bytes: &[u8]) -> anyhow::Result<()>;
    async fn get(&self, sha256: &str) -> anyhow::Result<Vec<u8>>;
    #[allow(dead_code)] // Interface completeness: for future consumers on the replay/GC path
    async fn exists(&self, sha256: &str) -> anyhow::Result<bool>;
    /// Actual deletion (#268, second half): only call once the store confirms nothing references this fingerprint anymore. Idempotent: not existing also counts as success.
    async fn delete(&self, sha256: &str) -> anyhow::Result<()>;
}

/// Local-disk implementation: stored flat as `{dir}/{sha256}` (byte-for-byte matching historical behavior).
pub struct LocalBlobStore {
    dir: PathBuf,
}

impl LocalBlobStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
}

#[async_trait::async_trait]
impl BlobStore for LocalBlobStore {
    async fn put(&self, sha256: &str, bytes: &[u8]) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let path = self.dir.join(sha256);
        if !path.exists() {
            tokio::fs::write(&path, bytes).await?;
        }
        Ok(())
    }

    async fn get(&self, sha256: &str) -> anyhow::Result<Vec<u8>> {
        Ok(tokio::fs::read(self.dir.join(sha256)).await?)
    }

    async fn exists(&self, sha256: &str) -> anyhow::Result<bool> {
        Ok(self.dir.join(sha256).exists())
    }

    async fn delete(&self, sha256: &str) -> anyhow::Result<()> {
        match tokio::fs::remove_file(self.dir.join(sha256)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
