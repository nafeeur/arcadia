//! Builds LLM clients from workspace settings, plus per-model concurrency gates.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use utopia_core::models::LlmSettings;
use utopia_llm::LlmClient;

use crate::state::AppState;

pub fn chat_client(s: &LlmSettings) -> Option<LlmClient> {
    if !s.chat_ready() {
        return None;
    }
    Some(LlmClient::new(
        s.chat_base_url.as_deref()?,
        s.chat_api_key.as_deref(),
        s.chat_model.as_deref()?,
    ))
}

pub fn embed_client(s: &LlmSettings) -> Option<LlmClient> {
    if !s.embed_ready() {
        return None;
    }
    Some(LlmClient::new(
        s.embed_base_url.as_deref()?,
        s.embed_api_key.as_deref(),
        s.embed_model.as_deref()?,
    ))
}

/// Per-model semaphore registry. When the limit changes, swap in a fresh semaphore — in-flight
/// permits on the old one simply run to completion, and the swap moment may briefly exceed the
/// new limit, which is acceptable. In exchange, a change takes effect immediately with no cache
/// invalidation to do, and no fighting with the fact that a tokio `Semaphore` can't shrink.
#[derive(Default)]
pub struct ModelGates {
    inner: std::sync::Mutex<HashMap<String, (usize, Arc<Semaphore>)>>,
}

impl ModelGates {
    fn gate(&self, key: &str, limit: usize) -> Arc<Semaphore> {
        let mut m = self.inner.lock().unwrap();
        match m.get(key) {
            Some((n, sem)) if *n == limit => sem.clone(),
            _ => {
                let sem = Arc::new(Semaphore::new(limit));
                m.insert(key.to_string(), (limit, sem.clone()));
                sem
            }
        }
    }
}

/// Acquires a permit before a background task calls a model, held until the call finishes.
///
/// **Background tasks only** (extraction, adjudication, ingest embedding, ontology
/// suggestions). User chat and search never go through here — making someone's typing wait
/// behind ten background extractions would be a bad product, and it's never a single person
/// typing that actually blows through a provider's rate limit anyway.
///
/// When the limit can't be read (table not created yet, store temporarily unreachable), this
/// **lets the call through**: the concurrency limit is a safeguard, and a pipeline shouldn't
/// stall entirely just because its config couldn't be read.
pub async fn acquire(
    state: &AppState,
    base_url: &str,
    model: &str,
) -> Option<OwnedSemaphorePermit> {
    let limit = utopia_store::model_limits::limit_for(&state.pool, base_url, model)
        .await
        .ok()?;
    let key = format!("{base_url}|{model}");
    state
        .model_gates
        .gate(&key, limit)
        .acquire_owned()
        .await
        .ok()
}

/// Convenience form of `acquire`: takes the chat model's identity directly from workspace settings.
pub async fn acquire_chat(state: &AppState, s: &LlmSettings) -> Option<OwnedSemaphorePermit> {
    let (base, model) = (s.chat_base_url.as_deref()?, s.chat_model.as_deref()?);
    acquire(state, base, model).await
}

/// Convenience form of `acquire`: for the embedding model.
pub async fn acquire_embed(state: &AppState, s: &LlmSettings) -> Option<OwnedSemaphorePermit> {
    let (base, model) = (s.embed_base_url.as_deref()?, s.embed_model.as_deref()?);
    acquire(state, base, model).await
}
