# Arcadia build notes

## Purpose and provenance

Arcadia turns Utopia's evolving knowledge graph into a workspace for inspecting evidence and reviewing document changes. It is a derivative product with its own interface; Utopia's Rust packages and compatible deployment settings remain intact.

The source was cloned from `https://github.com/deeplethe/utopia` and developed on the local `arcadia` branch. `UPSTREAM_REVISION` records the base commit. The source package also includes the original Git history in `provenance/upstream.bundle` and the complete Arcadia change in `provenance/arcadia.patch`. `UPSTREAM.md` preserves the upstream README; `LICENSE` and existing attribution remain unchanged.

## What is implemented

| Area | Behavior |
| --- | --- |
| Navigation and visual identity | New evergreen rail, paper surfaces, serif headings, responsive overview and evidence workspaces, sign-in presentation and Arcadia favicon. Existing feature pages inherit the new tokens. |
| Historical search | Postgres lexical search over retained chunks for `as_of`; chunk and document eligibility precede ranking. Current keyword search keeps Tantivy. Vector recall also checks document eligibility before limiting results. |
| Graph record time | A separate UTC control sends `as_of` to the graph and entity detail endpoints. Existing world-time controls remain independent. |
| Answer traces | New assistant messages save the answer, captured document sources, tool exchange and available model metadata in the same transaction. Traces are scoped to their owner and knowledge base. |
| Change review | Plain-text replacement proposals retain the original content and document revision. Editors propose; knowledge-base administrators approve/reject. |
| Impact inspection | Directly supported active facts, dependent derived-fact count and the caller's directly sourced answers. Lists stop at 200 and are labeled potential dependencies. |
| Approval | Document revision check and locks prevent stale overwrites. One transaction changes document metadata, records a version, enqueues processing and records the audit event. |
| Replay | A fresh answer using document retrieval and the current chat model, with optional historical time or proposed replacement. Saved comparisons and JSON export retain their evidence. |
| SSO | One configured OIDC issuer, authorization-code flow, S256 PKCE, one-use ten-minute state, nonce, issuer/audience/expiry checks, RS256 signature verification and explicit identity linking. |
| Recovery | Offline Postgres dump plus data directory archive, SHA-256 manifest, archive validation, refusal to overwrite an existing backup or populated restore database/data directory. |
| Measurement | Read-only search probe reports latency, errors, throughput and optional expected-document recall. It does not fabricate scale or answer-accuracy claims. |
| Purge | Copied proposal/trace text is redacted; replays referencing a purged source are removed. In-flight evidence persistence locks source rows to prevent reintroducing purged text. |
| Documentation | In-app Arcadia guide, searchable by the assistant's documentation tool; English and Chinese interface strings for the new surfaces. |

## Boundaries that matter

This is a substantial implementation, not a claim that every imaginable feature or every upstream roadmap item is complete.

- **Replay is document-only and nondeterministic.** It does not reconstruct the full original prompt, model state, graph/SQL tool run, external databases, prior conversation context or old model version. Exact stored output can be inspected; a new answer may differ.
- **Impact is dependency inspection.** It is not a causal simulation. Answers using only graph/SQL results are not automatically linked to every underlying document. There is no transitive answer-to-answer impact engine.
- **Citation validation is structural.** It detects absent/out-of-range numbered citations, not hallucinations or factual correctness.
- **Proposals replace one document with plain text.** They are not binary-file diffs, ontology transactions, cross-document commits or a branch/merge system. Approving a connector-managed document does not write back to its remote source; a later sync can update it again.
- **Approval queues work.** It does not make ingestion and model extraction synchronous or guarantee their success. Watch library status and retry diagnostics. Physical storage may contain an unreferenced content-addressed blob after a rejected concurrent approval.
- **Trace retention follows conversation retention.** Deleting a conversation deletes its traces and replays. Traces are inspectable application records, not a tamper-proof archive. The existing audit ledger separately records approval decisions without document text.
- **Historical lexical analysis differs from current search.** Postgres's `simple` dictionary is not Tantivy/Jieba, especially for unsegmented CJK. Historical snapshots of every mutable entity attribute are not added.
- **SSO has an explicit scope.** One issuer; RS256 ID tokens; public clients or `client_secret_basic`. No SAML, SCIM provisioning, automatic email linking, provider logout or automatic session revocation on IdP account changes. Existing local sessions follow the application's session rules.
- Existing external connectors, SQL engines and extraction models have not been certified against live accounts in this session. No high-scale latency, cost, factual-quality or availability claim is made.

## Start and upgrade

Follow `README.md` for Docker and local development. The UI is served by the Rust application on port 1516. A live installation requires Postgres with pgvector; answer generation also requires a configured model endpoint. There is no hosted Arcadia deployment attached to this source package.

Migrations 0034 and 0035 add Arcadia's tables, index and purge trigger. The source compatibility names (`utopia-server`, `UTOPIA_*`, database/role names and session cookie) are intentional. Use `ARCADIA_IMAGE` to override the locally built application tag. Take an offline backup before upgrading an existing installation. Migration rollback is not supplied; recovery uses the complete pre-upgrade database and files with matching application code.

## OIDC setup

Set these on the application process or in `.env` for Compose:

```dotenv
ARCADIA_OIDC_ISSUER=https://identity.example.com/your-issuer
ARCADIA_OIDC_CLIENT_ID=arcadia
ARCADIA_OIDC_CLIENT_SECRET=
ARCADIA_OIDC_REDIRECT_URI=https://arcadia.example.com/api/v1/auth/oidc/callback
```

Register the exact callback with the provider. Discovery, authorization, token and JWKS endpoints require HTTPS. Only a localhost/127.0.0.1 callback may use HTTP for development. The application fetches discovery and JWKS from the configured issuer; configure an issuer you control or trust.

Create the local account first. Under Account → Organization SSO, an organization administrator selects the account and supplies its exact provider `sub`. After linking, the person can use the organization sign-in button. Email equality alone never links accounts. Keep an administrator password login available while validating provider configuration.

## Offline backup and recovery

Requires Python 3.12+ and PostgreSQL client tools matching the server's major version. All application instances and other ingestion writers must be stopped for the entire operation. The archive contains credentials and the encryption key when that key is stored in `data/secret.key`; protect it as a sensitive backup. If you supply `UTOPIA_SECRET_KEY` externally, preserve that key separately as well.

The script reads the connection from `PGDATABASE`, then `UTOPIA_MIGRATION_URL`, then `UTOPIA_DATABASE_URL`. It does not load `.env` automatically. Set the variable in your shell using your normal secret-management process.

```sh
# Stop application writers; leave Postgres running.
docker compose --profile app stop app
python3 scripts/arcadia/backup.py backup ../arcadia-backup.tar.gz \
  --data-dir ./data --writers-stopped
python3 scripts/arcadia/backup.py verify ../arcadia-backup.tar.gz
```

For a recovery drill, point the connection variable at a **new empty** pgvector-capable database and use a new empty file destination:

```sh
python3 scripts/arcadia/backup.py restore ../arcadia-backup.tar.gz \
  --data-dir ./restored-data --writers-stopped
```

The script validates archive paths/checksums, stages files on the destination filesystem and restores the database in a single transaction. The final directory rename and database transaction cannot be one cross-system transaction. If final file placement fails after the database commits, keep writers stopped and recover into another fresh database/directory; never resume with mismatched files. Restore the matching application version, configure it to use the restored data directory/key, then test sign-in, a document download and search before switching traffic.

Archive integrity tests passed in this session. A native Postgres dump/restore drill has not been run here.

## Measure your deployment

Prepare a private JSON query set, for example:

```json
[
  {"q":"renewal notice", "as_of":null, "expected_document_ids":["YOUR_DOCUMENT_UUID"]},
  {"q":"contract price", "as_of":"2026-03-01T00:00:00Z", "expected_document_ids":[]}
]
```

Set `ARCADIA_TOKEN` to a session JWT. MCP personal access tokens do not authenticate this REST endpoint. Use HTTPS outside localhost and a token with access only to the intended knowledge base.

```sh
python3 scripts/arcadia/benchmark.py --url http://localhost:1516 \
  --kb YOUR_KB_UUID --queries queries.json --repeats 5 --concurrency 2 \
  --output retrieval-report.json
```

Record dataset size, hardware, database version, model configuration and concurrent ingestion alongside the report. Recall is reported only for queries with expected document IDs. This is retrieval evaluation, not answer-quality evaluation.

## Verification record

See `VERIFICATION.md` for the completed checks and remaining deployment checks. The primary automated gate is the native Postgres CI workflow; this session used an in-process Postgres-compatible PGlite runtime for the focused database and HTTP acceptance tests because the environment could not start native Postgres under its process restrictions.

## Recover a Git checkout from the source package

The archive contains a complete source tree and its built frontend. To create a separate checkout with the upstream Git history and Arcadia changes ready for review, run from the extracted `arcadia` directory:

```sh
git clone provenance/upstream.bundle ../arcadia-git
git -C ../arcadia-git apply --index ../arcadia/provenance/arcadia.patch
```

The patch leaves the Arcadia changes staged for your own commit. No remote repository was created and nothing was pushed to the original Utopia repository.
