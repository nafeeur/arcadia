use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use utopia_core::config::AppConfig;
use utopia_search::SearchIndex;
use uuid::Uuid;

/// In-process event (pushed to the frontend over SSE for partial refresh).
#[derive(Clone, Debug, serde::Serialize)]
pub struct AppEvent {
    /// None = belongs to no knowledge base. The alerts badge is cross-KB, and a
    /// system-level alert has no knowledge base at all
    pub kb_id: Option<Uuid>,
    /// document = document ingestion/extraction status changed; review = review
    /// queue changed; alert = something changed in the alerts center
    pub kind: &'static str,
    pub document_id: Option<Uuid>,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub jwt_secret: String,
    pub search: Arc<SearchIndex>,
    /// In-memory index of the Charter (built-in docs): used by chat's search_docs tool
    pub docs: Arc<utopia_search::DocsIndex>,
    /// Access seam for raw file bytes (content-addressed, key = sha256); currently backed by local disk
    pub blob: Arc<dyn crate::blob::BlobStore>,
    pub open_registration: bool,
    /// Forces the Secure cookie flag (a config option); when not forced, decided
    /// per request from X-Forwarded-Proto
    pub cookie_secure: bool,
    /// Worker concurrency: read fresh every scheduler loop tick -- a system
    /// setting change takes effect immediately
    pub worker_concurrency: Arc<std::sync::atomic::AtomicUsize>,
    /// Per-model concurrency gate: background tasks acquire a permit before
    /// calling the LLM. The limit is stored in the database and takes effect immediately when changed
    pub model_gates: Arc<crate::llm_util::ModelGates>,
    pub events: broadcast::Sender<AppEvent>,
    /// Answers currently being generated, looked up by conversation. **Can be
    /// reattached after a page refresh** (see `live`)
    pub live: Arc<crate::live::Registry>,
}

impl AppState {
    /// `jwt_secret` is resolved by the entry point: the environment variable if
    /// given, otherwise the one stored in the database (generated on first boot).
    /// It isn't read from cfg here because by this point it must already be a
    /// settled value, not an Option.
    pub fn new(
        pool: PgPool,
        cfg: &AppConfig,
        search: Arc<SearchIndex>,
        jwt_secret: String,
    ) -> Self {
        let (events, _) = broadcast::channel(256);
        let data_dir = PathBuf::from(&cfg.data_dir);
        let blob = Arc::new(crate::blob::LocalBlobStore::new(data_dir.join("files")));
        Self {
            pool,
            jwt_secret,
            search,
            docs: Arc::new(crate::docs_corpus::build_index()),
            blob,
            open_registration: cfg.open_registration,
            cookie_secure: cfg.cookie_secure,
            worker_concurrency: Arc::new(std::sync::atomic::AtomicUsize::new(32)),
            model_gates: Arc::new(crate::llm_util::ModelGates::default()),
            events,
            live: Arc::new(crate::live::Registry::default()),
        }
    }

    /// send returns Err when there are no subscribers -- normal, silently ignored.
    pub fn emit_document(&self, kb_id: Uuid, document_id: Uuid) {
        let _ = self.events.send(AppEvent {
            kb_id: Some(kb_id),
            kind: "document",
            document_id: Some(document_id),
        });
    }

    pub fn emit_review(&self, kb_id: Uuid) {
        let _ = self.events.send(AppEvent {
            kb_id: Some(kb_id),
            kind: "review",
            document_id: None,
        });
    }

    /// A memory extracted a fact awaiting confirmation (0015). The confirmation
    /// card in the conversation refreshes off this -- extraction is async, so
    /// the card can only appear once the task finishes, not the moment the
    /// assistant replies
    pub fn emit_pending(&self, kb_id: Uuid) {
        let _ = self.events.send(AppEvent {
            kb_id: Some(kb_id),
            kind: "pending",
            document_id: None,
        });
    }

    /// The graph changed. Fires once after reasoning adds an edge to the
    /// graph -- it doesn't go through the document pipeline, and the
    /// `document` event is specific to the document pipeline
    pub fn emit_graph(&self, kb_id: Uuid) {
        let _ = self.events.send(AppEvent {
            kb_id: Some(kb_id),
            kind: "graph",
            document_id: None,
        });
    }

    pub fn emit_source(&self, kb_id: Uuid) {
        let _ = self.events.send(AppEvent {
            kb_id: Some(kb_id),
            kind: "source",
            document_id: None,
        });
    }

    /// Something changed in alerts. **Carries no data, and checks no
    /// permissions** -- everyone who receives it just refetches the list, and
    /// "who can see what" is decided in exactly one place, the list query.
    ///
    /// The cost is that someone without permission also gets woken up to
    /// refetch, and gets back nothing. What that buys is zero permission logic
    /// anywhere on the push path -- there's no way for push and list to disagree.
    pub fn emit_alert(&self) {
        let _ = self.events.send(AppEvent {
            kb_id: None,
            kind: "alert",
            document_id: None,
        });
    }
}
