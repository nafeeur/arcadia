//! An in-progress answer that can be reattached to.
//!
//! **An SSE stream is bound to one HTTP request, but an answer outlives the request.**
//! Generation already moved into an independent task (`api::chat`), so refreshing the page
//! no longer loses the answer; but once that stream breaks, it's broken — after a refresh
//! you can only wait for it to land in the DB, with that in-between stretch invisible. The
//! frontend hoisting the in-progress one out of the component solves switching back and
//! forth within the same tab; **refresh, switching tabs, switching devices are all outside
//! its scope.**
//!
//! This fills in that last stretch: register it during generation, and anyone can reattach.
//!
//! **Reattaching hands over a snapshot first, not a replay of events.** The event stream can
//! grow unboundedly, and buffering it means keeping every delta of a conversation in memory;
//! whereas a snapshot's size is just the size of the answer itself, with a natural ceiling.
//! It's simpler for the client too: overwrite current state with the snapshot, then keep
//! receiving deltas as usual, with no need to track "which delta have I replayed up to."
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// One SSE event: event name + already-serialized data.
///
/// Not `axum::response::sse::Event` — it has no way to read the content back, and here we
/// need to both broadcast it and use it to update the snapshot.
#[derive(Clone, Debug)]
pub struct Frame {
    pub event: &'static str,
    pub data: String,
}

impl Frame {
    pub fn new(event: &'static str, data: String) -> Self {
        Self { event, data }
    }
}

/// What this answer looks like up to this moment. Whoever reattaches gets this first.
#[derive(Clone, Default, Debug)]
pub struct Snapshot {
    pub content: String,
    pub steps: Vec<serde_json::Value>,
    pub sources: Vec<serde_json::Value>,
}

impl Snapshot {
    /// **The snapshot is derived from the events themselves, with no separate write path.**
    /// Two write paths eventually drift apart — that's exactly the shape this repo keeps
    /// tripping on (one place learns about a new field, the other doesn't)
    fn apply(&mut self, f: &Frame) {
        match f.event {
            "delta" => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&f.data) {
                    if let Some(t) = v["text"].as_str() {
                        self.content.push_str(t);
                    }
                }
            }
            "step" => {
                if let Ok(v) = serde_json::from_str(&f.data) {
                    self.steps.push(v);
                }
            }
            // sources is a full resend, not an append
            "sources" => {
                if let Ok(serde_json::Value::Array(a)) = serde_json::from_str(&f.data) {
                    self.sources = a;
                }
            }
            _ => {}
        }
    }

    pub fn to_frame(&self) -> Frame {
        Frame::new(
            "snapshot",
            json!({
                "content": self.content,
                "steps": self.steps,
                "sources": self.sources,
            })
            .to_string(),
        )
    }
}

struct Entry {
    tx: broadcast::Sender<Frame>,
    snap: Arc<RwLock<Snapshot>>,
}

/// In-progress generations, looked up by conversation.
#[derive(Default)]
pub struct Registry(RwLock<HashMap<Uuid, Entry>>);

/// The handle held for the duration of one generation. Sends events, deregisters on finish.
pub struct Handle {
    conversation_id: Uuid,
    tx: broadcast::Sender<Frame>,
    snap: Arc<RwLock<Snapshot>>,
    registry: Arc<Registry>,
}

impl Handle {
    /// Sends one event: records it into the snapshot, then broadcasts it.
    ///
    /// **Still holding the snapshot's write lock while broadcasting** is required. Merely
    /// guaranteeing "write before send" doesn't rule out duplication: someone reattaching
    /// between the two steps would see this segment both in the snapshot and again from the
    /// broadcast. Sending while holding the lock, with `attach` subscribing while holding the
    /// read lock, makes the two mutually exclusive — so the moment of reattaching falls
    /// entirely before this emit, or entirely after it
    pub async fn emit(&self, frame: Frame) {
        let mut snap = self.snap.write().await;
        snap.apply(&frame);
        // No subscribers is the normal case (the person left), not an error
        let _ = self.tx.send(frame);
    }

    /// Generation is done. **Anyone reattaching after deregistration gets "nothing running"**,
    /// at which point the answer is already persisted — just read it from the DB
    pub async fn finish(self) {
        self.registry.0.write().await.remove(&self.conversation_id);
    }
}

impl Registry {
    /// Registers one generation. Registering the same conversation again replaces the old
    /// one — shouldn't happen under normal conditions, and if it does, the new one wins
    pub async fn begin(self: &Arc<Self>, conversation_id: Uuid) -> Handle {
        let (tx, _) = broadcast::channel(256);
        let snap = Arc::new(RwLock::new(Snapshot::default()));
        self.0.write().await.insert(
            conversation_id,
            Entry {
                tx: tx.clone(),
                snap: snap.clone(),
            },
        );
        Handle {
            conversation_id,
            tx,
            snap,
            registry: self.clone(),
        }
    }

    /// Reattaches to a running generation: gets the snapshot as of this moment, plus deltas
    /// from then on.
    ///
    /// Returns `None` = no generation running for this conversation. **That's not an
    /// error**, it's the most common case
    pub async fn attach(
        &self,
        conversation_id: Uuid,
    ) -> Option<(Snapshot, broadcast::Receiver<Frame>)> {
        let map = self.0.read().await;
        let entry = map.get(&conversation_id)?;
        // **Subscribe while holding the snapshot's read lock.** `emit` broadcasts while
        // holding the write lock, so this segment is mutually exclusive with any emit: the
        // snapshot obtained lines up exactly with the subscription's starting point — that
        // small in-between stretch is neither missed nor duplicated
        let guard = entry.snap.read().await;
        let rx = entry.tx.subscribe();
        Some((guard.clone(), rx))
    }
}
