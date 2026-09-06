//! BlobStore: the storage/retrieval seam for raw file bytes.
//!
//! Content-addressed — the key is the sha256 of the file content, so the interface has
//! no notion of "path": locally it's a flat directory (pre-bucketing), for object storage
//! it's the object key, and any KV store can implement it. Idempotence, dedup, and
//! immutability (fingerprint changes when content changes, old versions are never
//! overwritten — the material basis for "replay has substance") all come free from
//! "content is the address".
//!
//! The only implementation at this stage is local disk (data/files/{sha256}). Adding
//! object storage / cloud drive later (P5 connectors, shared storage across multi-instance
//! deployments) only requires a new implementation — callers on the ingest/upload/parse/replay
//! path change zero lines. The UTOPIA_BLOB_BACKEND config entry is reserved but currently
//! only accepts "local".

use std::path::PathBuf;

#[async_trait::async_trait]
pub trait BlobStore: Send + Sync {
    /// Idempotent write: skips if the same fingerprint already exists.
    async fn put(&self, sha256: &str, bytes: &[u8]) -> anyhow::Result<()>;
    async fn get(&self, sha256: &str) -> anyhow::Result<Vec<u8>>;
    #[allow(dead_code)] // interface completeness: future consumers of the replay/GC path
    async fn exists(&self, sha256: &str) -> anyhow::Result<bool>;
    /// Actual delete (#268 second half): only call once the DB confirms nothing else
    /// references this fingerprint. Idempotent: not existing also counts as success.
    async fn delete(&self, sha256: &str) -> anyhow::Result<()>;
}

/// Local disk implementation: flat storage as `{dir}/{sha256}` (byte-identical to legacy behavior).
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
