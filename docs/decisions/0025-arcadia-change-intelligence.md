# Arcadia: review knowledge changes and retain the evidence

Arcadia is an Apache-2.0 derivative of Utopia. Upstream history and licensing remain intact. Internal crate names and UTOPIA configuration stay compatible; the product and UI are Arcadia.

## Delivery scope

1. Historical lexical search over retained chunk versions, with temporal filtering before ranking.
2. Durable per-answer evidence snapshots and model metadata, linked to the original conversation and its owner.
3. Document change proposals: staged replacement text, dependency/answer impact, reviewer approval, stale-version checks, and atomic replacement/job enqueue.
4. Evidence replay: regenerate an answer from current or historical document evidence and compare to the exact captured answer. This is explicitly a document RAG rerun, not deterministic reconstruction of a graph/SQL agent or a causal simulation.
5. A dramatically different workspace: persistent side navigation, an editorial operations overview, change inbox, trace explorer, evidence workbench, and accessible responsive styling.
6. Backup/restore utilities, a repeatable load probe, API tests, and clear deployment instructions.

## Invariants

All reads check knowledge-base membership. Private conversation traces remain private to the conversation owner, even when a document is shared. Impact counts and lists only include the caller's traces. Proposal approval requires a KB admin and cannot approve a stale document revision. Staging never changes live knowledge. Approval records a document version and enqueues normal processing in the same database transaction. Extraction remains asynchronous and is not represented as completed on approval.

Snapshots capture the evidence text visible at answer completion, tool exchanges already recorded by Utopia, and model identity without credentials. They are not claimed to reconstruct the complete original prompt or mutable database state. Physical document purge redacts copied source text; conversation deletion removes associated traces.

Historical full text uses Postgres simple-text lexical ranking rather than pretending Tantivy indexes historical versions. Current searches keep Tantivy (including its Chinese segmentation); historical lexical tokenization differs and is documented.

## Scope that needs external validation

Production SSO needs an actual identity provider. Live connector compatibility needs accounts and real services. Model quality and cost claims need configured models and repeated measurements. No feature is labeled production-verified merely because it compiles. These are tracked explicitly in ARCADIA.md.
