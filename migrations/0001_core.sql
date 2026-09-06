-- Core: multi-tenant tables, job queue, access control and deployment settings.
-- pgvector extension is created up front (chunks.embedding in P1 depends on it); the pgvector/pgvector image ships it already.
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE organizations (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE users (
    id            UUID PRIMARY KEY,
    org_id        UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    -- **Uniqueness only constrains active accounts** — see the partial index below.
    -- The address frees up once deactivated; otherwise "deactivated" would mean
    -- "this email is permanently unusable", and the same person couldn't even
    -- make a new account.
    email         TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    display_name  TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- System administrator in a single-tenant deployment. The first person to register gets this automatically (see accounts.rs).
    is_admin      BOOLEAN NOT NULL DEFAULT FALSE,
    -- **Soft delete, not DELETE.** Audit events, merge logs, the change ledger and
    -- adjudication decisions all have an `actor_id` pointing at this person, and
    -- those are audit material — we must still be able to answer "who did this"
    -- after the person is gone. Deactivation only cuts off access:
    -- `find_user_by_email` and `find_user_by_id` each carry a
    -- `deactivated_at IS NULL` clause — the former blocks login, the latter
    -- blocks already-issued tokens (session validation goes through it, so
    -- deactivation takes effect immediately).
    deactivated_at TIMESTAMPTZ,
    -- Who deactivated this account. A bare foreign key — the person who deactivated someone can themselves later be deactivated, and that record still needs to exist.
    deactivated_by UUID REFERENCES users(id)
);

-- email is unique, **but only among active accounts**. Deactivated accounts can
-- share a duplicate address, so any query that looks a person up by email must
-- carry `deactivated_at IS NULL` — it already needs that clause anyway (otherwise
-- a deactivated person could still log in), so this index just makes that clause
-- also a correctness guarantee.
CREATE UNIQUE INDEX users_email_active_idx ON users (email) WHERE deactivated_at IS NULL;

CREATE TABLE workspaces (
    id          UUID PRIMARY KEY,
    org_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE memberships (
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    role         TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'editor', 'viewer')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);
CREATE INDEX memberships_workspace_idx ON memberships (workspace_id);

CREATE TABLE knowledge_bases (
    id           UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    kind         TEXT NOT NULL DEFAULT 'knowledge' CHECK (kind IN ('knowledge', 'memory')),
    description  TEXT,
    -- The deployment's public default space (the first KB created in a workspace): always open, never deletable (enforced by the API).
    is_default   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- An open KB with no membership row falls back to the deployment role; a
    -- restricted KB is only visible to members listed in kb_members (outsiders get NotFound).
    visibility   TEXT NOT NULL DEFAULT 'open'
                 CHECK (visibility IN ('open', 'restricted')),
    -- Whether to extend the ontology on the user's behalf. **An explicit toggle,
    -- not something inferred from behavior** — it used to be inferred from
    -- "has the ontology ever been touched", and a wrong inference there was
    -- absurd: clicking Add once on a proposal would permanently turn off the
    -- suggestion feature forever, because that recorded an ontology action with
    -- an actor attached. And once it went false it could never go true again —
    -- the ontology stayed frozen on whatever vocabulary the first batch of
    -- documents happened to contain, even though new documents keep arriving every day.
    auto_extend_ontology BOOLEAN NOT NULL DEFAULT TRUE,
    -- **Not the "system language"** (see docs/decisions/0004). Interface language
    -- lives on the client; the backend has no locale. This column governs the
    -- **corpus's language**: a class's description goes verbatim into the
    -- extraction prompt, and the reader is the model reading your documents —
    -- judgments are more reliable when the description and the judged text
    -- share a language. So a Chinese team reading English technical docs wants
    -- a Chinese interface while this column should be 'en' — one toggle can't do both jobs.
    --
    -- The allowed values live in a CHECK rather than the application layer:
    -- this column picks a compile-time constant table, and a value with no
    -- matching table would silently fall back to English instead of erroring —
    -- that's the hardest kind of bug to track down.
    ontology_lang TEXT NOT NULL DEFAULT 'en',
    -- The default KB is always open (the rule is enforced by the API; this is the DB-level second guarantee).
    CONSTRAINT kb_default_open CHECK (NOT is_default OR visibility = 'open'),
    CONSTRAINT knowledge_bases_ontology_lang_chk CHECK (ontology_lang IN ('en', 'zh'))
);
CREATE INDEX knowledge_bases_workspace_idx ON knowledge_bases (workspace_id);

-- Job queue: consumed with FOR UPDATE SKIP LOCKED, see docs/DESIGN.md section 2.
CREATE TABLE jobs (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind         TEXT NOT NULL,
    payload      JSONB NOT NULL DEFAULT '{}',
    status       TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'done', 'failed')),
    attempts     INT NOT NULL DEFAULT 0,
    max_attempts INT NOT NULL DEFAULT 3,
    run_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_at    TIMESTAMPTZ,
    last_error   TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX jobs_claim_idx ON jobs (run_at) WHERE status = 'queued';

-- Workspace-level LLM settings (chat and embedding configured separately, OpenAI-compatible protocol).
CREATE TABLE llm_settings (
    workspace_id   UUID PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    chat_base_url  TEXT,
    chat_api_key   TEXT,
    chat_model     TEXT,
    embed_base_url TEXT,
    embed_api_key  TEXT,
    embed_model    TEXT,
    embed_dim      INT,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- KB-level access control. Deployment roles hang off an invisible workspace
-- (memberships untouched); each KB carries its own role matrix, configured in that KB's own Settings.
CREATE TABLE kb_members (
    kb_id      UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role       TEXT NOT NULL CHECK (role IN ('viewer', 'editor', 'admin')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Who added this member (left NULL when there's no one to attribute it to; the UI then falls back to showing just the timestamp).
    added_by   uuid REFERENCES users(id) ON DELETE SET NULL,
    PRIMARY KEY (kb_id, user_id)
);

CREATE TABLE deployment_settings (
    singleton         BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    open_registration BOOLEAN NOT NULL DEFAULT TRUE,
    -- Job worker concurrency (changeable in system settings; the scheduling loop
    -- reads it hot, so changes take effect immediately). **This is an outer
    -- backstop, not the throttle** — the real throttling is done by the
    -- per-model semaphore (see model_concurrency); this only guards against
    -- jobs piling up without bound. It should be comfortably larger than the
    -- sum of the per-model limits, otherwise throttled jobs can occupy every
    -- slot and starve everything else.
    worker_concurrency INT NOT NULL DEFAULT 32
        CHECK (worker_concurrency BETWEEN 1 AND 32),
    -- Character budget for inlining the ontology into the extraction prompt;
    -- past this it switches to per-chunk retrieval candidates instead. This
    -- lives in deployment settings rather than an environment variable because
    -- it needs to be changeable without a restart — tuning it requires a pair
    -- of comparisons (full inline vs. per-chunk retrieval) at every ontology
    -- size, and if changing it meant restarting the service, nobody would ever
    -- run that comparison a second time. 24000 characters (roughly 6000 tokens) is a placeholder, pending that comparison.
    ontology_prompt_budget INTEGER NOT NULL DEFAULT 24000,
    -- Fallback for any model with no entry in model_concurrency.
    default_model_concurrency INT NOT NULL DEFAULT 10,
    -- JWT signing key. **Generated automatically on first start**, so "get it
    -- running per the README" and "secure" stop being two separate chores —
    -- reminders alone don't prevent the kind of incident where a default value
    -- like dev-secret-change-me ends up in production. UTOPIA_JWT_SECRET still
    -- takes priority over this column: to rotate the key, or to align multiple
    -- instances explicitly, just set the environment variable — that path stays open.
    jwt_secret TEXT,
    -- Default ontology_lang for new KBs; see knowledge_bases.ontology_lang for what it means.
    default_ontology_lang TEXT NOT NULL DEFAULT 'en',
    CONSTRAINT deployment_default_ontology_lang_chk
        CHECK (default_ontology_lang IN ('en', 'zh'))
);
INSERT INTO deployment_settings DEFAULT VALUES;

-- Concurrency is limited **per model, not per deployment**. The real constraint
-- is the model provider's rate limit, and that's per model (together with its
-- base_url): a local Ollama might only handle 2 concurrent calls, a hosted API
-- 50 — one global number can't govern both correctly.
--
-- The throttle sits at the LLM call site rather than at job scheduling: jobs
-- that don't call a model (folder sync) shouldn't be constrained by it, and
-- jobs that call different models (extraction uses chat, ingestion uses
-- embedding) shouldn't crowd each other out either.
CREATE TABLE model_concurrency (
    base_url        TEXT NOT NULL,
    model           TEXT NOT NULL,
    max_concurrent  INT  NOT NULL CHECK (max_concurrent BETWEEN 1 AND 256),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (base_url, model)
);
