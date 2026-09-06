-- Graph layer: ontology vocabulary + entities + a bitemporal fact ledger + evidence chains.
-- Design: see docs/DESIGN.md §3 — facts are append-only; extraction errors set invalidated_at, fact changes close valid_to.

-- Classes. **The two vectors are not redundant**: type resolution issues two
-- kinds of query (see `type_resolution.rs`) — one is the model's short phrase
-- ("district. place"), the other is a whole profile. These used to be compared
-- against the same `label + description` vector — the queries came in two
-- shapes but the document only had one, so short queries got dominated by
-- classes whose description is a tautology of the label (a one-liner class
-- like "Park\nA park." wins on length, not semantics: the median length of a
-- retrieved class was 44, versus 89 for the population as a whole).
--
-- The pattern "the shorter side is systematically closer" has bitten this
-- repository four times now (incomparable across entities, incomparable
-- across the two query paths, incomparable between two queries on the same
-- path, empty-description dangling classes getting an unfair advantage) — the
-- first four fixes were all on the query side; this one is on the document
-- side: **two kinds of query deserve two documents**, short-to-short and long-to-long.
--
-- Dimension varies, same as chunks.embedding, following whichever model the
-- workspace has chosen, so no HNSW index — sequential scan. Ontologies run to
-- the thousands of rows; sequential scan is plenty fast enough.
CREATE TABLE entity_types (
    id         UUID PRIMARY KEY,
    kb_id      UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    key        TEXT NOT NULL,
    label      TEXT NOT NULL,
    color      TEXT NOT NULL DEFAULT '#64748b',
    builtin    BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Graph nodes render by type's shape (organization/product = square used to be hardcoded in the frontend).
    shape      TEXT NOT NULL DEFAULT 'circle' CHECK (shape IN ('circle', 'square')),
    -- Not cosmetic copy for humans — this is the semantic guidance fed into the
    -- extraction prompt ("Event: something with a definite point in time, such
    -- as a launch, an acquisition, a meeting"), and it directly affects extraction quality.
    description TEXT NOT NULL DEFAULT '',
    -- The IRI is the global identity; `key` is the short label the model reads
    -- (see the "division of labor between IRI and key" note in migration 0001
    -- P2). Re-import matches against an existing row by IRI — matching by key
    -- would treat the same class as a new one whenever upstream changes
    -- rdfs:label (since that changes key), orphaning all of its entities.
    iri        TEXT,
    -- Long-form document: label + description.
    embedding  vector,
    -- **Stores what text was actually embedded, not just a timestamp.** A
    -- timestamp can only answer "has this been embedded", not "is the
    -- embedding still current for this text" — once the description changes or
    -- the model changes, the vector is stale and the timestamp can't tell you
    -- that. Storing the source text and the model name means a backfill job
    -- can compare and know exactly who needs re-embedding, without having to
    -- hook every write site that might change a description (miss one and it silently rots).
    embedded_text  text,
    embedded_model text,
    -- Short-form document: label only.
    label_embedding      vector,
    label_embedded_text  text,
    label_embedded_model text,
    UNIQUE (kb_id, key)
);
CREATE UNIQUE INDEX entity_types_iri_idx ON entity_types (kb_id, iri) WHERE iri IS NOT NULL;

CREATE TABLE relation_types (
    id         UUID PRIMARY KEY,
    kb_id      UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    key        TEXT NOT NULL,
    label      TEXT NOT NULL,
    -- Temporal semantics: state (an interval) / event (a point in time) / eternal (no time dimension).
    temporal   TEXT NOT NULL DEFAULT 'state' CHECK (temporal IN ('state', 'event', 'eternal')),
    -- Cardinality uniqueness (single-valued at any given moment), the basis for
    -- temporal-conflict detection (auto-closing valid_to): functional = unique
    -- on the subject side; inverse_functional = unique on the object side (one leader per project).
    functional BOOLEAN NOT NULL DEFAULT FALSE,
    inverse_functional BOOLEAN NOT NULL DEFAULT FALSE,
    builtin    BOOLEAN NOT NULL DEFAULT FALSE,
    -- Attribute system: an attribute is a relation whose range is a literal
    -- (an RDF datatype property) — one table serving two purposes. Attribute
    -- values go through the facts.object_value channel, reusing the whole
    -- temporal/evidence/review machinery.
    kind       TEXT NOT NULL DEFAULT 'relation' CHECK (kind IN ('relation', 'attribute')),
    datatype   TEXT CHECK (datatype IN ('text', 'number', 'date', 'bool')),
    unit       TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    description TEXT NOT NULL DEFAULT '',
    iri        TEXT,
    embedding  vector,
    embedded_text  text,
    embedded_model text,
    -- The remaining property axioms belong to the same family as `functional` /
    -- `inverse_functional` — those two just landed in the schema first.
    -- **Defaults to false, not NULL**: OWL is open-world, but consistency
    -- checking can only judge what's been written down — "not declared" and
    -- "declared false" have the same consequence here (neither is grounds for
    -- reporting a contradiction), so there's no need for a three-state value to
    -- distinguish a difference that doesn't change behavior.
    --
    -- **The `is_` prefix on these column names isn't style fussiness**:
    -- `symmetric` and `asymmetric` are both Postgres reserved words (as in
    -- `BETWEEN SYMMETRIC`) — using them bare would fail with a syntax error
    -- right on the CREATE TABLE line. Quoting would work around it, but that
    -- would require every future piece of SQL touching these two columns to
    -- remember to quote them too — miss one and it only blows up at runtime.
    is_transitive  BOOLEAN NOT NULL DEFAULT FALSE,
    is_symmetric   BOOLEAN NOT NULL DEFAULT FALSE,
    is_asymmetric  BOOLEAN NOT NULL DEFAULT FALSE,
    is_irreflexive BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (kb_id, key)
);
CREATE UNIQUE INDEX relation_types_iri_idx ON relation_types (kb_id, iri) WHERE iri IS NOT NULL;

-- domain / range are **association tables, not columns**: in OWL it's normal
-- for one property to have multiple rdfs:domain values (the subject of
-- works_at might be a person or an organization) — a single column can't
-- express that, and import would have to pick one and drop the rest (FOAF has
-- properties exactly like this). We also don't keep a "primary domain" column
-- either — that would be the same fact recorded in two places, and two places
-- eventually drift apart (we've already been burned once by preview and
-- persistence each computing key-conflict separately and the preview lying
-- about the result). One authoritative place means a reader never has to guess which one to trust.
--
-- range only applies to object properties: a datatype property's range is a literal type, which lives on relation_types.datatype.
CREATE TABLE relation_type_domains (
    relation_type_id UUID NOT NULL REFERENCES relation_types(id) ON DELETE CASCADE,
    entity_type_id   UUID NOT NULL REFERENCES entity_types(id)   ON DELETE CASCADE,
    PRIMARY KEY (relation_type_id, entity_type_id)
);

CREATE TABLE relation_type_ranges (
    relation_type_id UUID NOT NULL REFERENCES relation_types(id) ON DELETE CASCADE,
    entity_type_id   UUID NOT NULL REFERENCES entity_types(id)   ON DELETE CASCADE,
    PRIMARY KEY (relation_type_id, entity_type_id)
);

-- Reverse lookups: the ontology page needs to list a class's properties, and
-- extraction needs to filter which properties are usable for a class. The
-- primary key covers the forward direction; the reverse direction needs its own index.
CREATE INDEX relation_type_domains_entity_idx ON relation_type_domains (entity_type_id);
CREATE INDEX relation_type_ranges_entity_idx  ON relation_type_ranges  (entity_type_id);

-- subClassOf. **A class can have multiple parents** — this is normal in real
-- vocabularies: FOAF's Person is simultaneously a foaf:Agent and a
-- geo:SpatialThing — two directions, not ancestors on the same chain. If we
-- only recognized one parent, a property whose domain sits on the other
-- branch would fail to validate (latitude's domain is SpatialThing, but if
-- person is only attached under agent, that fact gets extracted and then blocked).
--
-- **is_primary is not redundant**: the left-hand panel renders a tree, and a
-- class can only be drawn in one place — "which branch it's drawn under" is a
-- question the subClassOf set alone can't answer. It's extra information, not the same fact recorded twice.
CREATE TABLE entity_type_parents (
    child_id   UUID NOT NULL REFERENCES entity_types(id) ON DELETE CASCADE,
    parent_id  UUID NOT NULL REFERENCES entity_types(id) ON DELETE CASCADE,
    -- The left panel's tree view follows this branch. Not semantically load-bearing, display only.
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (child_id, parent_id),
    -- Self-loops are blocked right here; longer cycles are checked by the application before writing (SQL alone can't catch A→B→A).
    CONSTRAINT entity_type_parents_no_self CHECK (child_id <> parent_id)
);

-- At most one primary parent per class.
CREATE UNIQUE INDEX entity_type_parents_primary_idx
    ON entity_type_parents (child_id) WHERE is_primary;

-- Reverse lookup: the left panel needs to find children by parent, and domain checking needs to walk up the parent chain.
CREATE INDEX entity_type_parents_parent_idx ON entity_type_parents (parent_id);

-- Class disjointness. **Stored as a table, not an array column**: the question
-- being asked is "are A and B disjoint" — a single point lookup; an array
-- column would mean either a full table scan or building a GIN index, when the
-- underlying semantics here really is just a single edge.
CREATE TABLE entity_type_disjoint (
    kb_id UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    a_id  UUID NOT NULL REFERENCES entity_types(id) ON DELETE CASCADE,
    b_id  UUID NOT NULL REFERENCES entity_types(id) ON DELETE CASCADE,
    -- Each direction gets its own row (the import side has already expanded
    -- the axiom's symmetry). The primary key therefore de-duplicates
    -- naturally, and a query never has to care which end the caller asked from.
    PRIMARY KEY (kb_id, a_id, b_id),
    -- A class being disjoint with itself is a meaningless statement — better to block it at the door than let a consistency check guess at it.
    CHECK (a_id <> b_id)
);

-- "What is disjoint with this class" is the only lookup shape used (consistency checking asks it using an entity's class).
CREATE INDEX entity_type_disjoint_a_idx ON entity_type_disjoint (kb_id, a_id);

CREATE TABLE entities (
    id             UUID PRIMARY KEY,
    kb_id          UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    -- **Nullable**, see docs/decisions/0009: "not yet classified" is not itself a class.
    -- It used to be a sentinel row called `concept`, and a sentinel with a name
    -- is a name that can collide — the key derived from SKOS's skos:Concept is
    -- literally `concept`, and the import rule "a placeholder with no IRI
    -- claims it" would let that import take over the sentinel, silently
    -- turning every unclassified entity into a bona fide skos:Concept overnight.
    -- That's not a collision that gets skipped — it's semantics being silently
    -- rewritten. NULL has no name, so nothing collides with it, and it can't be
    -- forgotten about either — failing to filter out a sentinel produces no
    -- warning at all, while mishandling a NULL fails loudly on the spot.
    type_id        UUID REFERENCES entity_types(id) ON DELETE RESTRICT,
    canonical_name TEXT NOT NULL,
    aliases        TEXT[] NOT NULL DEFAULT '{}',
    attrs          JSONB NOT NULL DEFAULT '{}',
    -- Points to the surviving entity once merged (merges are reversible; a P2 follow-up).
    merged_into    UUID REFERENCES entities(id),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Entity profile: an incremental centroid of evidence chunk vectors (used
    -- for contextual-similarity judgments; reuses the chunk embeddings already computed during ingestion).
    profile_embedding vector,
    profile_n      INTEGER NOT NULL DEFAULT 0,
    -- Disambiguating display suffix when entities share a name (e.g. "Jane Smith · Platform Engineering").
    disambiguator  TEXT,
    -- The type the model proposed: **the word it used when the ontology had no
    -- room for it.** Without recording this, there'd be no way to find the
    -- entities that want a proposed "model" class — they'd be mixed in with
    -- everything else unclassified, recoverable only by re-extracting the whole KB.
    proposed_type  TEXT,
    -- What the model itself said this entity most specifically is.
    --
    -- **Cannot share a column with proposed_type**: that column means "what the
    -- model wanted isn't in the ontology", and the ontology-growth loop uses
    -- its rarity as a threshold — if every entity populated it, the loop would
    -- propose a new class for every single entity.
    --
    -- Why we need it at all: there's always a "close enough" option on the
    -- list. The ontology has `product`, the model decides that's good enough
    -- and picks it, and the specific thought in its head — "a vector database" — is lost.
    -- And that specific name is exactly what type resolution needs most: a
    -- short name matched against a short label is a much closer match than
    -- matching a paragraph of prose against schema.org's "A software application."
    specific_type  text,
    -- **How this type was decided.** Protection has to happen up front —
    -- `entity_retypes` remembers "who changed it", but that's a record after
    -- the fact; the entity itself needs to carry whether anyone has ever made
    -- a call on it, otherwise every read path can only see `type_id`. This was
    -- missing on the type-resolution path: an entity a human set to
    -- `organization` would, as long as `organization` had subclasses, get
    -- pulled back into resolution and rejudged on the very next pass regardless.
    --
    -- Since migration 0009 there's also another case: "no type" can now be **a
    -- human decision** — someone looked at this entity and decided the
    -- ontology has no suitable class for it. `type_id IS NULL` alone can't
    -- distinguish "not yet judged" from "a human judged it and decided there's
    -- none", so without this column the next extraction pass would just assign it a type anyway.
    --
    --   extracted  decided by extraction (a `resolve_type_drift` promotion)
    --   inferred   decided by the engine (type resolution, claimed after the ontology grew a new class)
    --   human      decided by a person (direct edit in the entity panel, or an approval in the review queue)
    --
    -- `inferred` and `extracted` are kept separate rather than merged into a
    -- single "not human" value because they carry different trust levels, and
    -- if we ever need a rule like "the engine may revise its own inferences
    -- but not extraction's", the distinction is already there to build it on.
    type_source    TEXT NOT NULL DEFAULT 'extracted'
                   CHECK (type_source IN ('extracted', 'human', 'inferred'))
);
-- **Not unique**: a name is not an identity (the two "Jane Smith"s from
-- migration 0001 P0), so entities sharing a name are allowed to coexist — this
-- index only serves candidate recall.
--
-- Since migration 0009, type_id can also be NULL, and in Postgres NULL <> NULL
-- — so two same-named entities that both lack a type are, if anything, even
-- less likely to be blocked here. That's the intended behavior: when
-- unclassified, we know even less about whether they're the same thing, which is even less reason to merge them.
CREATE INDEX entities_kb_type_name_idx
    ON entities (kb_id, type_id, lower(canonical_name)) WHERE merged_into IS NULL;
CREATE INDEX entities_kb_idx ON entities (kb_id);
-- Cross-type same-name recall (for type-drift handling): the index above is prefixed by type_id and can't serve a cross-type query.
CREATE INDEX entities_kb_name_idx
    ON entities (kb_id, lower(canonical_name)) WHERE merged_into IS NULL;

-- Fact ledger: SPO + a bitemporal timeline (append-only, never DELETEd).
CREATE TABLE facts (
    id              UUID PRIMARY KEY,
    kb_id           UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    subject_id      UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    -- **Nullable**: "we can't say what the relation is" shouldn't itself be a
    -- relation (the same move as what migration 0009 did for `concept`). It
    -- used to be a builtin relation row called `related_to`, sitting alongside
    -- real relations on the ontology page — but what it actually encoded was
    -- "the extractor found an edge, but the ontology has no matching relation
    -- for it", which is control flow, not vocabulary.
    --
    -- Removing it loses zero information: the original wording already lives
    -- in the evidence's `proposed_predicate`, just hidden behind that fake
    -- vocabulary entry. Removing it actually surfaces more information — what
    -- used to universally display as "related to" now shows what the source
    -- text actually said: acquired / runs_on / sued (see fact_surface_predicate).
    predicate_id    UUID REFERENCES relation_types(id) ON DELETE CASCADE,
    object_id       UUID REFERENCES entities(id) ON DELETE CASCADE,
    object_value    JSONB,
    valid_from      TIMESTAMPTZ,
    valid_to        TIMESTAMPTZ,
    -- **Each endpoint records its own precision, and no date means no precision.**
    --
    -- This used to be a single `valid_precision NOT NULL DEFAULT 'day'`, so
    -- facts with no date at all still got 'day' written into the ledger — the
    -- ledger filled in a definite-looking value in a place where it actually
    -- knew nothing, and any UI rendering "precise to the day" off this field
    -- would go ahead and say so. The default value itself was the bug: it made
    -- "never measured" look identical to "measured to the day".
    --
    -- A single column describing both endpoints doesn't work either: a fact
    -- that only has valid_to (something like "until 2023"), would have that
    -- column named `from` while describing a `to`.
    valid_from_precision TEXT,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    invalidated_at  TIMESTAMPTZ,
    confidence      REAL NOT NULL DEFAULT 1.0,
    derived_by_rule UUID,
    supersedes      UUID REFERENCES facts(id),
    -- The end endpoint. There's an extra 'unknown' value because the ledger
    -- originally had no way to say "it ended, but we don't know when":
    -- `valid_to IS NULL` was carrying two meanings at once, so a sentence like
    -- "former CEO of Weta Digital" — **where the source text explicitly says it
    -- ended, just not when** — could only be written as null, which the system
    -- then read as "still ongoing", and the graph would confidently assert
    -- something the source text said had already ended.
    --
    --   still ongoing            valid_to IS NULL   valid_to_precision IS NULL
    --   ended, date unknown      valid_to IS NULL   valid_to_precision = 'unknown'
    --   ended in 2023            valid_to = …       valid_to_precision = 'year'
    --
    -- **We deliberately don't use the textbook "indeterminate instant" trick of
    -- filling valid_to with the document date as an upper bound**: that would
    -- put a timestamp in the column that looks definite, forcing every reader
    -- to check the precision before trusting it — and this product's whole
    -- value proposition is not lying to people.
    valid_to_precision text,
    CHECK (object_id IS NOT NULL OR object_value IS NOT NULL),
    -- The start endpoint has no 'unknown' value — "it started, but we don't
    -- know when" and "we don't know whether it started" are indistinguishable
    -- in this ledger, and forcing in an extra state would only invite readers to guess.
    CONSTRAINT facts_from_precision_matches_date
      CHECK ((valid_from IS NULL) = (valid_from_precision IS NULL)),
    -- That `IS NOT NULL` is not redundant. Without it, a row where `valid_to`
    -- has a date but precision is NULL would **pass**: `NULL IN ('year',…)`
    -- evaluates to NULL, `TRUE AND NULL` is NULL, `NULL OR FALSE` is still
    -- NULL, and a CHECK constraint treats NULL as passing. Three-valued logic is silent here.
    CONSTRAINT facts_to_precision_matches_date
      CHECK (
        (valid_to IS NOT NULL AND valid_to_precision IS NOT NULL
           AND valid_to_precision IN ('year', 'month', 'day'))
        OR (valid_to IS NULL AND (valid_to_precision IS NULL OR valid_to_precision = 'unknown'))
      )
);
-- Hot-path partial indexes: invalidated rows are excluded from the index (ledger boundaries and cleanup, DESIGN.md §3.1).
CREATE INDEX facts_live_subject_idx ON facts (kb_id, subject_id) WHERE invalidated_at IS NULL;
CREATE INDEX facts_live_object_idx  ON facts (kb_id, object_id)  WHERE invalidated_at IS NULL;
CREATE INDEX facts_live_time_idx    ON facts (kb_id, valid_from, valid_to) WHERE invalidated_at IS NULL;
-- Point lookups for the temporal-conflict-detection invariant: one for the subject side, one for the object side (open period + not invalidated).
CREATE INDEX facts_open_pair_idx ON facts (kb_id, subject_id, predicate_id)
    WHERE valid_to IS NULL AND invalidated_at IS NULL;
CREATE INDEX facts_open_obj_pair_idx ON facts (kb_id, object_id, predicate_id)
    WHERE valid_to IS NULL AND invalidated_at IS NULL;
-- The epistemic axis. The indexes above all live on the world axis and only
-- recognize live rows, serving "what was true at a given moment"; these two
-- serve a different question — **when was it written, and when was it overturned**
-- ("what changed in our understanding last quarter", see the chat `changes` tool).
-- The invalidated index is a partial index with an inverted condition: live
-- rows all have invalidated_at = NULL, and including them would make the index
-- as big as the table, while overturned facts are naturally the minority.
CREATE INDEX facts_recorded_idx ON facts (kb_id, recorded_at DESC);
CREATE INDEX facts_invalidated_idx ON facts (kb_id, invalidated_at DESC)
    WHERE invalidated_at IS NOT NULL;

-- Evidence chain: fact ↔ source chunk (provenance is a first-class citizen).
CREATE TABLE fact_evidence (
    fact_id  UUID NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    chunk_id UUID NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    quote    TEXT,
    -- Evidence provenance version: which document and which version of it this came from (used for version reconciliation and "evidence is stale" display).
    document_id UUID REFERENCES documents(id) ON DELETE CASCADE,
    doc_version INT,
    -- The model's exact wording. **Recorded on the evidence, not the fact**:
    -- facts are deduplicated by (kb, subject, predicate, object), so if chunk A
    -- says "runs on" and chunk B says "optimized for", they merge into the same
    -- row — putting the wording there would mean first-writer-wins with the
    -- rest silently discarded. Evidence is one row per chunk, which is the
    -- right granularity, and it already carries `quote` (that chunk's
    -- supporting text) — the exact wording is the same kind of thing: the raw shape of each individual observation.
    proposed_predicate TEXT,
    PRIMARY KEY (fact_id, chunk_id)
);


-- Temporal conflicts (S3): auto-closed when uncertain, escalated to a human to decide close / keep / reject_new.
CREATE TABLE fact_conflicts (
    id          UUID PRIMARY KEY,
    kb_id       UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    old_fact_id UUID NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    new_fact_id UUID NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    -- no_time | simultaneous | low_confidence
    reason      TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'resolved')),
    -- closed | kept_both | rejected_new
    resolution  TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ,
    UNIQUE (old_fact_id, new_fact_id)
);
CREATE INDEX fact_conflicts_open_idx ON fact_conflicts (kb_id) WHERE status = 'open';

-- **What to display** for a fact with no predicate.
--
-- Implemented as a function rather than repeating a subquery at every read
-- site: there are six or more paths that read facts (graph edges, the entity
-- panel, change history, low-confidence review, document output, resolution
-- profiles), and six copies of the same SQL would eventually drift — and
-- drifting here means the same edge gets called by a different name on different pages.
--
-- **Determinism**: ties in occurrence count break by lexical order, so the same fact always displays the same word.
CREATE FUNCTION fact_surface_predicate(fact uuid) RETURNS text
LANGUAGE sql STABLE AS $$
    SELECT e.proposed_predicate
      FROM fact_evidence e
     WHERE e.fact_id = fact AND e.proposed_predicate IS NOT NULL
     GROUP BY e.proposed_predicate
     ORDER BY count(*) DESC, e.proposed_predicate
     LIMIT 1
$$;

-- When an entity-type proposal is adopted, which entities had their type changed.
--
-- Symmetric with fact_adoptions, for the same reason: creating the type
-- without touching entities would grow the ontology without improving the
-- graph. And the change must be reversible, or nobody will trust the system to auto-create classes.
--
-- Entities aren't append-only (they're mutable rows — the P0 PATCH edits them
-- directly), so reverting relies on recording the prior type rather than a `supersedes` chain.
CREATE TABLE entity_retypes (
    batch_id     UUID NOT NULL,
    kb_id        UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    entity_id    UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    -- **Nullable**: the single most common retype since migration 0009 is
    -- "from no type to a type", and this table is the only record for
    -- reverting it. If this were NOT NULL, the very first type assignment
    -- couldn't be recorded, and that whole batch of changes would be unreversible.
    from_type_id UUID REFERENCES entity_types(id) ON DELETE CASCADE,
    to_type_id   UUID NOT NULL REFERENCES entity_types(id) ON DELETE CASCADE,
    -- Consistent with fact_adoptions: mark as reverted rather than delete — both the adoption and the reversal are things that happened.
    reverted_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Who made the change. Nullable = decided automatically by the engine, the
    -- same convention as `entity_merges.merged_by`.
    --
    -- **This only answers "who initiated this change"**, not "was this type a
    -- human judgment" — those two things got conflated once before: type
    -- resolution passed along whoever clicked "run", so every entity the
    -- engine decided ended up with `type_source = human` and was never resolved again (see #117).
    actor_id     UUID REFERENCES users(id),
    PRIMARY KEY (batch_id, entity_id)
);

CREATE INDEX entity_retypes_kb_idx ON entity_retypes (kb_id, created_at DESC);

-- When a surface predicate is adopted, which fact got rewritten into which.
--
-- Without this table, a rewrite leaves only the single `facts.supersedes`
-- pointer, which can't cover the "merged into an already-existing fact" case:
-- the old row is invalidated with no successor pointing at it at all. Two
-- consequences follow — reverting can't find where it went, and entity history
-- reads "invalidated with no successor" as rejected, so the UI would say "this
-- record was retracted" when it was actually merged intact into another assertion.
--
-- This also closes the other half of a governance requirement: audit rows used to only record the total "49 facts rewritten", with no way to answer "which 49, specifically".
CREATE TABLE fact_adoptions (
    -- The batch for one adoption action; reverting operates at this granularity.
    batch_id     UUID NOT NULL,
    kb_id        UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    predicate_id UUID NOT NULL REFERENCES relation_types(id) ON DELETE CASCADE,
    old_fact_id  UUID NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    new_fact_id  UUID NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    -- superseded = a new row replaces the old one; merged = merged into an existing row.
    mode         TEXT NOT NULL,
    -- Reverting doesn't delete the row: erasing "what happened" contradicts the
    -- ledger's own rule, and reverting is itself a human decision that entity history needs to attribute.
    reverted_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (batch_id, old_fact_id)
);

-- Used by entity history to ask, per fact, "was this one merged away" — this index serves that lookup.
CREATE INDEX fact_adoptions_old_idx ON fact_adoptions (old_fact_id);
-- List revertible batches per KB.
CREATE INDEX fact_adoptions_kb_idx ON fact_adoptions (kb_id, created_at DESC);

-- Blocked facts leave no trace. The extractor has seven `continue` sites:
-- unknown subject type, attribute domain mismatch, value doesn't fit the
-- datatype, confidence too low, and so on — a fact gets extracted, gets
-- blocked, and nothing is said about it; the user just sees something missing
-- from the graph. That directly conflicts with three of this project's
-- principles: the ledger is append-only, every fact has evidence, and uncertainty surfaces to a human.

-- Aggregated per document: this can compute both "how many didn't land" for a
-- single document and roll up to the KB level. It also naturally fixes a
-- lifecycle problem — ontology_misses used to only get cleared on a full KB
-- rebuild (graph.rs), so a per-source re-extraction would never clear it and
-- counts would go stale; clearing per document instead means every
-- re-extraction accounts for itself automatically.
CREATE TABLE extraction_drops (
    kb_id       UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    -- Machine-aggregatable reason code (attr_domain_mismatch / low_confidence / ...).
    reason      TEXT NOT NULL,
    -- The specific object under that reason (an attribute key, a predicate name, "salary@organization").
    detail      TEXT NOT NULL,
    count       INT NOT NULL DEFAULT 1,
    -- A sample so a human can see at a glance what was dropped ("Acme Corp → salary").
    example     TEXT,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (kb_id, document_id, reason, detail)
);

CREATE INDEX extraction_drops_doc_idx ON extraction_drops (document_id);
