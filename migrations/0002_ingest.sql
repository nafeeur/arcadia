-- Ingestion pipeline: sources, documents, chunks, versions and sync records.

-- A source is a folder: a source is the container in Library that holds the
-- documents it has ingested, and it can sync on a schedule.
-- kind: upload (virtual home for manually uploaded files, source_id is usually NULL) | watch_folder | url | rss | api.
-- Design: see docs/DESIGN.md §4 Ingestion channels.
CREATE TABLE sources (
    id         UUID PRIMARY KEY,
    kb_id      UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL DEFAULT 'upload',
    name       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Each source kind's own configuration (URL list, RSS address, selectors, ...). The shape varies by kind, so this is JSONB rather than a pile of sparse columns.
    config     JSONB NOT NULL DEFAULT '{}',
    -- NULL = manual sync only.
    sync_interval_minutes INTEGER,
    last_sync_at     TIMESTAMPTZ,
    last_sync_status TEXT NOT NULL DEFAULT 'never'
                     CHECK (last_sync_status IN ('never', 'queued', 'running', 'ok', 'failed')),
    last_sync_error  TEXT,
    last_sync_added  INTEGER NOT NULL DEFAULT 0,
    icon       TEXT,
    -- Cron expression (standard 5-field), mutually exclusive with sync_interval_minutes.
    -- The UI builds it with a visual picker; only Advanced mode exposes the raw expression.
    sync_cron  TEXT,
    -- **Stored in plaintext, not hashed.** In the self-hosted threat model,
    -- "reveal once" would just be self-inflicted friction: plaintext storage
    -- means it can be viewed again anytime (via an Editor-only endpoint). If the
    -- DB is compromised the document contents are already exposed, so hashing
    -- this secret buys nothing extra; Rotate is kept as the response to a leak.
    ingest_token TEXT
);
CREATE INDEX sources_kb_idx ON sources (kb_id);

CREATE TABLE documents (
    id              UUID PRIMARY KEY,
    kb_id           UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    source_id       UUID REFERENCES sources(id) ON DELETE SET NULL,
    filename        TEXT NOT NULL,
    mime            TEXT NOT NULL DEFAULT 'application/octet-stream',
    size_bytes      BIGINT NOT NULL DEFAULT 0,
    sha256          TEXT NOT NULL,
    -- pending → parsing → indexing → embedding → ready | failed
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'parsing', 'indexing', 'embedding', 'ready', 'failed')),
    error           TEXT,
    -- Document time: has a trust tier and is editable (see DESIGN.md 4.2).
    doc_time        TIMESTAMPTZ,
    doc_time_source TEXT NOT NULL DEFAULT 'file_mtime',
    text_len        INT NOT NULL DEFAULT 0,
    chunk_count     INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Graph extraction status. **Kept separate from the ingestion pipeline
    -- status**: this two-stage split means a document is searchable as soon as
    -- parsing finishes, while extraction runs at its own pace.
    graph_status    TEXT NOT NULL DEFAULT 'none'
                    CHECK (graph_status IN ('none', 'queued', 'extracting', 'done', 'failed')),
    -- Document tags (for filtering and bulk organization; not an entity/folder system).
    --
    -- **All four layers are empty today, and deliberately left that way.**
    -- Nothing writes it, nothing reads it, nothing surfaces it — `set_document_tags`
    -- has zero callers, and the frontend has never even mentioned the field
    -- name. It has survived three rounds of migration squashing (53 → 19 → 10)
    -- without anyone remembering it existed.
    --
    -- It's kept, not forgotten: **tags would be the one dimension on this table
    -- that a human actually attaches themselves.** Source describes where a
    -- document came from, name and status are system-assigned — none of the
    -- three express a cross-source grouping that only a person would know,
    -- like "this batch needs redacting" or "the Q3 bundle".
    --
    -- The counter-argument holds equally well: Arcadia's thesis is that **the
    -- graph is the organizing structure**, and what migrations 0009 / 0010 /
    -- 0011 removed was exactly "mechanisms that duplicate the ontology".
    -- Adding this would be building a second organizing system next to the
    -- graph. There's a sharper objection too: the most common real need behind
    -- "tags" is actually "I haven't reviewed this one yet", and that isn't a
    -- tag, it's **review status** — which deserves a first-class representation of its own.
    --
    -- Left unresolved, pending outside input. Whoever next wants to clean up
    -- dead code: this comment is the conclusion, don't just delete the column.
    tags            TEXT[] NOT NULL DEFAULT '{}',
    -- Logical identity within the source (watch_folder relative path / url / rss
    -- guid / api external_id). Ingestion uses this to decide new / changed /
    -- unchanged — a content change replaces the document in place rather than
    -- piling up a new document, and the old version is recorded in document_versions.
    external_key    TEXT,
    -- Files that disappear from a watched directory get this timestamp. **Kept, not deleted, by default.**
    missing_since   TIMESTAMPTZ,
    -- Reason extraction failed. A separate column rather than reusing `error`:
    -- that column belongs to the parsing pipeline (set_status clears it), so the two don't interfere with each other.
    graph_error     TEXT,
    -- Ownership token for the extraction job. Bumping it on re-extraction
    -- effectively "fires" whichever job is currently running: after finishing
    -- each chunk it re-reads this value, and if the epoch has changed it quietly
    -- exits and hands the document off to the new job. Relying on graph_status
    -- alone is unreliable — the new job would write the status back to
    -- "extracting", leaving the old job with no way to tell it's been superseded.
    extract_epoch   INT NOT NULL DEFAULT 0
);
CREATE INDEX documents_kb_idx ON documents (kb_id, created_at DESC);
CREATE INDEX documents_tags_idx ON documents USING gin (tags);
CREATE UNIQUE INDEX documents_kb_sha_idx ON documents (kb_id, sha256);

CREATE TABLE chunks (
    id           UUID PRIMARY KEY,
    kb_id        UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    document_id  UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    seq          INT NOT NULL,
    text         TEXT NOT NULL,
    heading      TEXT,
    char_start   INT NOT NULL DEFAULT 0,
    char_end     INT NOT NULL DEFAULT 0,
    -- Dimension varies with the chosen embedding model; P1 does a sequential scan for retrieval, and an HNSW index gets built on the configured dimension once volume grows.
    embedding    vector,
    -- Version soft-delete: when a document updates, the old chunks are marked
    -- (superseded_at) rather than physically deleted — fact_evidence references
    -- stay unbroken and the old text can still be replayed; the embedding is
    -- cleared when marked (superseded chunks don't participate in retrieval).
    doc_version   INT NOT NULL DEFAULT 1,
    superseded_at TIMESTAMPTZ,
    -- Graph-extraction completion marker: when a document updates, unchanged
    -- chunks that are "claimed" carry this forward and skip re-extraction
    -- (incremental extraction), and it also lets an interrupted extraction resume where it left off.
    extracted_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX chunks_document_idx ON chunks (document_id, seq);
CREATE INDEX chunks_kb_idx ON chunks (kb_id);
CREATE INDEX chunks_live_idx ON chunks (document_id) WHERE superseded_at IS NULL;


CREATE UNIQUE INDEX documents_source_key_idx
    ON documents (source_id, external_key) WHERE external_key IS NOT NULL;

-- Raw material for version replay (file blobs are content-addressed, never deleted).
CREATE TABLE document_versions (
    id          UUID PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    version     INTEGER NOT NULL,
    sha256      TEXT NOT NULL,
    size_bytes  BIGINT NOT NULL DEFAULT 0,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (document_id, version)
);

-- One row per sync (time/status/output/error) — an auditable history per channel.
-- Only the most recent 50 rows are kept per source (trimmed in finish_run).
CREATE TABLE source_sync_runs (
    id           UUID PRIMARY KEY,
    source_id    UUID NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    started_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at  TIMESTAMPTZ,
    status       TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'ok', 'failed')),
    created_docs INTEGER NOT NULL DEFAULT 0,
    updated_docs INTEGER NOT NULL DEFAULT 0,
    error        TEXT
);
CREATE INDEX source_sync_runs_source_idx ON source_sync_runs (source_id, started_at DESC);
