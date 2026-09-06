-- Ontology: extraction-miss counts, imports, proposals, and human-approved refinement pairs.

-- Types/relations hit during extraction that fall outside the allow-list: not noise, a signal for ontology growth
CREATE TABLE ontology_misses (
    kb_id      UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    -- attribute_type and relation_type are kept separate: when an out-of-vocabulary predicate
    -- carries a literal value (`founding_date: "2015"`), what's missing is an attribute, not a
    -- relation — getting this wrong would make the ontology-proposal step build a relation instead
    kind       TEXT NOT NULL
               CHECK (kind IN ('entity_type', 'relation_type', 'attribute_type')),
    key        TEXT NOT NULL,
    example    TEXT,
    count      INT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The person said no. **Mark, don't delete**: dismiss used to be a DELETE, so the next
    -- extraction run hitting the same term would reinsert it unchanged — the user's "no" didn't
    -- survive one extraction cycle. With auto-growth of the ontology on, that amounted to the
    -- system overriding an explicit human decision
    dismissed_at TIMESTAMPTZ,
    PRIMARY KEY (kb_id, kind, key)
);

-- The first layer of ontology import: fidelity to the source.
--
-- The projection only covers what we can consume today (classes, labels, rdfs:comment,
-- subClassOf, object/data properties, functional, domain/range). **What we can't parse isn't
-- an error, it's "not projected yet"** — the source is stored content-addressed in a blob, and
-- when the reasoning engine ships or we add a new consumer, it gets reprojected with no action
-- needed from the user. That downgrades "we can't express this" from a capability gap to
-- "projection doesn't cover this yet".
CREATE TABLE ontology_imports (
    id            UUID PRIMARY KEY,
    kb_id         UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    -- Content fingerprint of the blob; re-importing the same file doesn't duplicate storage
    sha256        TEXT NOT NULL,
    filename      TEXT NOT NULL,
    -- turtle | rdfxml
    format        TEXT NOT NULL,
    byte_size     BIGINT NOT NULL,
    -- Projection version: once the projection logic changes, this says which imports need a rerun
    projection_version INT NOT NULL DEFAULT 1,
    -- What this projection did (counts and detail of created/updated/not-yet-projected), shared by the preview and post-hoc audit
    summary       JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Who imported it. Left NULL after account deletion, same rule as the audit ledger
    imported_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    imported_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ontology_imports_kb_idx ON ontology_imports (kb_id, imported_at DESC);


-- Ontology proposals, persisted.
--
-- These used to live only in browser memory (`Ontology.tsx`'s `useState<OntologyProposals>`):
-- one refresh, one navigation away, one crash, and the whole batch of proposals was gone —
-- seeing them again meant rerunning the model.
--
-- **What's lost isn't the raw material.** Unmatched terms always persist in `ontology_misses`.
-- What's lost is the **clustering result** — which terms got grouped under the same proposal,
-- and the "adopting this will reclassify N terms" estimate. That's exactly the one thing worth
-- being able to verify: 0003 records the model suggesting `optimized_for` be merged into
-- `runs_on` ("optimized for RTX" isn't the same as "runs on RTX") — **it was only caught because
-- the merged terms were visible in a tooltip**, and it became the direct evidence for "this
-- shouldn't auto-merge everything." Something worth verifying shouldn't live only as long as
-- one page's lifecycle.
CREATE TABLE ontology_proposals (
    id         UUID PRIMARY KEY,
    kb_id      UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    -- Proposal bucket, named the same as the four sections the API returns:
    -- entity_types | relation_types | attribute_types | map_to
    section    TEXT NOT NULL,
    key        TEXT NOT NULL,
    -- The proposal verbatim: label, description, reason, forms, datatype, temporal…
    --
    -- **Stored as JSONB rather than split into columns**: the four sections genuinely have
    -- different shapes (relations have temporal and forms, attributes have datatype, map_to has
    -- a target) — splitting them means either four tables or one sparse wide one. The frontend
    -- consumes this same JSON, so storing it verbatim means not changing the contract. Which
    -- terms got merged is still queryable (`payload->'forms'`)
    payload    JSONB NOT NULL,
    -- open = still awaiting review; adopted / rejected = someone has already decided
    status     TEXT NOT NULL DEFAULT 'open'
               CHECK (status IN ('open', 'adopted', 'rejected')),
    decided_by UUID REFERENCES users(id),
    decided_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Only one row per (kb, section, key). Rerunning Suggest refreshes it, it doesn't pile up another
    UNIQUE (kb_id, section, key)
);

-- "How many are still waiting?" is the most common question asked of this table (another gap
-- from 0003: with the auto-growth switch off, there's no "N new terms since last time"
-- notification — the signal sits in the panel, but nobody actively looks)
CREATE INDEX ontology_proposals_open_idx
    ON ontology_proposals (kb_id, created_at DESC)
    WHERE status = 'open';

-- Human-approved "coarse type → refined type" pairs. Approve once, and that same pair never goes back to manual review.
--
-- Why this table exists: the manual-review bucket is triggered by "is the selected type inside
-- the coarse type's subtree", and in practice that check mostly measures whether **the seed
-- type is even connected to the imported vocabulary's class tree**, not actual risk. schema.org
-- gives Place its own `place` key with none of the built-in `location` subclasses under it, so
-- every single location → city ends up flagged as cross-axis — 14 out of 24 entities flagged,
-- every one of them correct.
--
-- Being cross-axis is a property of the (coarse type, target type) pair, not of the entity.
-- Once a person has confirmed "things under location can be a city," the second city
-- shouldn't be asked about again.
CREATE TABLE type_refinement_pairs (
    kb_id       uuid NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    from_type_id uuid NOT NULL REFERENCES entity_types(id) ON DELETE CASCADE,
    to_type_id  uuid NOT NULL REFERENCES entity_types(id) ON DELETE CASCADE,
    approved_by uuid REFERENCES users(id),
    approved_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (kb_id, from_type_id, to_type_id)
);
