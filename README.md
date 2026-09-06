<div align="center">

<img src="web/public/arcadia.svg" alt="Arcadia" width="72">

# Arcadia

**A living record of knowledge, evidence and change.**

[![License](https://img.shields.io/badge/LICENSE-APACHE%202.0-3FB950?style=flat-square&labelColor=161B22)](LICENSE)
[![Rust](https://img.shields.io/badge/BUILT%20WITH-RUST-F74C00?style=flat-square&labelColor=161B22&logo=rust&logoColor=FFFFFF)](https://www.rust-lang.org)
[![Postgres](https://img.shields.io/badge/DATA-POSTGRES%20%2B%20PGVECTOR-336791?style=flat-square&labelColor=161B22&logo=postgresql&logoColor=FFFFFF)](https://github.com/pgvector/pgvector)
[![Built on Utopia](https://img.shields.io/badge/BUILT%20ON-UTOPIA-2D333B?style=flat-square&labelColor=161B22)](https://github.com/deeplethe/utopia)

[Overview](#overview) · [Features](#features) · [Quick start](#quick-start) · [Configuration](#configuration) · [Development](#development) · [Attribution](#attribution)

</div>

---

## Overview

Arcadia is a self-hosted **temporal knowledge graph and retrieval-augmented assistant**. Documents come in, an ontology-driven pipeline turns them into a bitemporal graph of entities and facts, and a chat assistant answers questions over that graph with inline citations — grounded in your own documents and databases, not a vendor's.

Arcadia adds a full **review workflow around changing knowledge** on top of that foundation: every proposed edit to a document is staged, reviewed, and approved before it lands, every assistant answer keeps an inspectable evidence trail, and you can search or browse the graph as it stood at any point in the past — not just as it is today.

It ships as **one Rust binary and one Postgres database.** Full-text search is embedded in the binary (Tantivy), vectors live in `pgvector`, and the job queue is a table — nothing else to run.

> Arcadia is built on top of [DeepLethe's Utopia](https://github.com/deeplethe/utopia), using its graph, ingestion, ontology, SQL-mapping, chat and account foundation, and adding the review/evidence/SSO layer described below along with a new visual identity. See [Attribution](#attribution).

<img src="assets/screenshots/login.jpg" alt="Arcadia sign-in screen">

<table>
<tr>
<td width="50%">

**Dashboard** — what's waiting for review, and what you've asked recently
<img src="assets/screenshots/overview.jpg" alt="Arcadia overview dashboard">

</td>
<td width="50%">

**Chat with citations** — every answer links back to the passage it came from
<img src="assets/screenshots/chat.jpg" alt="Arcadia chat with inline citations">

</td>
</tr>
<tr>
<td width="50%">

**Knowledge graph** — entities, facts and their evidence, browsable as of any date
<img src="assets/screenshots/graph.jpg" alt="Arcadia knowledge graph view">

</td>
<td width="50%">

**Change review** — side-by-side diff and dependency impact before anything lands
<img src="assets/screenshots/change-review.jpg" alt="Arcadia change review diff view">

</td>
</tr>
<tr>
<td width="50%">

**Ontology workbench** — the schema behind extraction, browsable as a graph
<img src="assets/screenshots/ontology.jpg" alt="Arcadia ontology workbench">

</td>
<td width="50%">

**Review queue** — unconfirmed facts, low-confidence extractions, and merges the AI adjudicator made (all reversible)
<img src="assets/screenshots/review.jpg" alt="Arcadia review queue dashboard">

</td>
</tr>
<tr>
<td width="50%">

**Evidence explorer** — search the knowledge base as it stood at a past date
<img src="assets/screenshots/evidence-explorer.jpg" alt="Arcadia evidence explorer historical search">

</td>
<td width="50%">

**Answer ledger** — every past answer, its model, its citations, and a replay tool to re-check it
<img src="assets/screenshots/answer-ledger.jpg" alt="Arcadia answer ledger with document replay">

</td>
</tr>
</table>

## Features

### Added in Arcadia

| | |
|---|---|
| **Historical document search** | Search retains chunk text at a specified point in time (`as_of`), with document/chunk eligibility checked before ranking. Graph browsing has its own independent "as known at" control, including entity detail views. |
| **Answer ledger** | Every assistant answer saves its response, captured document sources, tool exchange and model metadata in one transaction. Inspect any past answer, or export it as JSON. |
| **Change proposals** | Editors stage plain-text replacements for a document. Reviewers see the original and proposed text side by side, the facts directly supported by the change, dependent derived facts, and any answers that relied on the original. Knowledge-base administrators approve or reject. |
| **Fenced approval** | A stale proposal can't silently overwrite a document that changed underneath it. Approval checks the document revision, records a new version, enqueues reprocessing and appends an audit event — all in one transaction, and repeated approval can't double-enqueue the job. |
| **Document replay & preview** | Re-run a saved question against the current document, a historical snapshot, or a pending proposal, and inspect the new answer plus a structural citation check. Previewing never mutates the live document. |
| **Organization SSO** | OIDC authorization-code flow with PKCE, nonce, one-use state and RS256 verification. Administrators link an existing local account to a provider's exact subject identifier — no automatic email-based linking. |
| **Offline backup & recovery** | A checksummed archive of the database plus the data directory, with guarded restore (refuses to overwrite an existing backup or a populated restore target) and a read-only search-latency/recall probe for measuring your deployment. |
| **Deletion handling** | Purging a document redacts the evidence copied into proposals/traces and removes replays that reference it. In-flight evidence writes are fenced against a concurrent purge. |

### Inherited from Utopia

| | |
|---|---|
| **A complete application** | A system console, graph browser and ontology workbench in one web UI — install it and it works, not a library to build on top of. |
| **Knowledge ingest** | PDF, DOCX, PPTX, XLSX, XLS, ODS, CSV, TSV, Markdown, HTML and plain text, with legacy encodings detected automatically. Web pages, RSS, GitHub, Jira, Notion, WebDAV and S3-compatible buckets sync on a schedule; everything else comes in through the API. |
| **Search and chat** | Full-text on Tantivy, vectors on pgvector, fused with RRF. Answers stream with inline citations that open the passage they came from. Any OpenAI-compatible endpoint works (DeepSeek, Qwen, GLM, Ollama, vLLM) — the whole system can run air-gapped. |
| **Agent harness and agentic RAG** | The built-in agent searches documents, walks the graph (an entity's facts as of any date, or what changed in a period) and queries a mounted database. The same read-only tools are exposed over MCP. |
| **Ontology and cold start** | A new knowledge base starts from ontology packs you pick at creation — schema.org, W3C Org, PROV-O, FOAF and IOF Core ship in the binary. Terms outside the packs are counted as they appear; confirm the common ones and they join the ontology. |
| **Bitemporal graph** | Extraction turns documents into entities and facts against an editable ontology. Every fact carries when it held and where it came from. Correcting a fact closes the old version and links the new one to it instead of overwriting — the graph keeps two timelines: when something was true in the world, and when the system came to believe it. |
| **Entity resolution and review** | Duplicates resolve in three stages — exact name/alias, embedding similarity, then a model's judgment on doubtful pairs. Every merge can be undone. Low-confidence extractions, suspected duplicates and cardinality conflicts go to a review queue. |
| **Reasoning and derivation** | Ontology axioms compile into forward-chaining rules — transitivity, symmetry, inverses and relation hierarchy. Off by default. A derived fact is marked as such, carries validity and confidence, and shows what it was derived from; when it contradicts an asserted fact, the asserted one wins. |
| **Conflict detection** | A new fact clashing with an older one: close the old, keep both, or reject the new. Data breaking an axiom (self-loop, asymmetry, transitive cycle, cardinality): retract, relax the axiom, or accept both. |
| **Ontology-driven querying (Ask-the-Data)** | Mount a database on a knowledge base (Postgres, MySQL-protocol engines, Trino for Iceberg/Delta Lake/Hive, Databricks, Snowflake) and chat queries it alongside your documents. The agent proposes how tables map onto the ontology; you confirm. Read-only, parser-gated, row-limited. |
| **Multi-user and permissions** | Each knowledge base has its own members and roles — owner, admin, editor, viewer. Open bases are readable deployment-wide; restricted ones need an invitation. |
| **Decision ledger** | Confirming/rejecting a fact, merging or reverting an entity, rebuilding the graph — each leaves an append-only record of who, when, and what the object looked like at the time. |

## Quick start

Requires Docker Engine with Compose and enough memory/disk to compile the Rust application.

```bash
git clone https://github.com/nafeeur/arcadia.git
cd arcadia
cp .env.example .env
docker compose --profile app up --build -d
```

Open **http://localhost:1516**, create the initial administrator account, configure a chat and embedding model under workspace settings, and upload a document.

Data lives in the `dbdata` Postgres volume and `./data` for source files, the search index and the generated encryption key — keep both when backing up.

> Running under rootless **Podman** instead of Docker? `podman-compose --profile app up --build -d` works, but you'll likely need three fixes this repository's Compose file doesn't handle for you: point `UTOPIA_DB_IMAGE` at a fully-qualified image (`docker.io/pgvector/pgvector:pg16`) since Podman won't resolve a short name non-interactively; leave `UTOPIA_DATABASE_URL` unset in `.env` for this flow (it's for the native-`cargo run` dev flow below, and would otherwise override the compose network default); and if SELinux is enforcing, `chcon -Rt container_file_t ./data` so the app container can write to the bind mount.

## Configuration

All configuration is environment variables — see [`.env.example`](.env.example) for the full list with explanations. The essentials:

| Variable | Purpose |
|---|---|
| `UTOPIA_DATABASE_URL` | Postgres connection string (native/dev flow only — Compose supplies its own) |
| `UTOPIA_BIND_ADDR` | Address the server listens on (default `0.0.0.0:1516`) |
| `UTOPIA_SECRET_KEY` | Encrypts credentials (LLM API keys, connection strings, source tokens) at rest — auto-generated into `data/secret.key` on first start if unset |
| `UTOPIA_JWT_SECRET` | Session token signing key — auto-generated into the database on first start if unset |
| `UTOPIA_OPEN_REGISTRATION` | Whether anyone can sign up, or only the first account (default `true`) |
| `ARCADIA_OIDC_ISSUER` / `_CLIENT_ID` / `_CLIENT_SECRET` / `_REDIRECT_URI` | Organization SSO — see [ARCADIA.md](ARCADIA.md#oidc-setup) |

The environment variable prefix (`UTOPIA_*`), database role/name, and session cookie intentionally match upstream Utopia so existing deployment tooling and documentation keep working — see [ARCADIA.md](ARCADIA.md) for the full rationale and the complete operational reference (upgrades, backup/recovery, measuring a deployment).

## Development

```bash
docker compose up -d db          # Postgres with pgvector
cd web && pnpm install --frozen-lockfile && pnpm build
cd .. && cargo run -p utopia-server   # runs migrations, serves on :1516
```

Or for frontend iteration with hot reload:

```bash
cd web && pnpm install && pnpm dev    # :5173, proxies /api to the backend
```

### Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
UTOPIA_DATABASE_URL=postgres://utopia:utopia@localhost:1517/utopia \
  UTOPIA_TEST_REQUIRE_DB=1 cargo test --workspace -- --test-threads=1
python3 -m unittest discover -s scripts/arcadia -p 'test_*.py'
cd web && pnpm build
```

Start the application once against the test database first, so migrations are applied before the suite runs. See [CONTRIBUTING.md](CONTRIBUTING.md) for the full workflow, and [VERIFICATION.md](VERIFICATION.md) for what has and hasn't been verified in this build.

## Security

Read [SECURITY.md](SECURITY.md) before exposing an instance to the public internet — it documents the current threat-model boundaries (default credentials, data-source grants, what's encrypted at rest) and how to report a vulnerability.

## Status

Arcadia is at **v0.1**. The database schema evolves between versions and migrations only roll forward, with no rollback. Pin a specific version with `ARCADIA_IMAGE` in production, and take a backup — database plus the `data` directory — before upgrading.

## Attribution

Arcadia is built on top of **[Utopia](https://github.com/deeplethe/utopia)** by [DeepLethe](https://github.com/deeplethe) — a bitemporal knowledge graph and retrieval-augmented assistant, which serves as the foundation for Arcadia's ingestion, ontology, reasoning, SQL-mapping, chat and account layers. Arcadia adds the review workflow, historical search, evidence traces, organization SSO, recovery tooling and visual identity described above. It is an independent derivative project, not an official DeepLethe release, and is not affiliated with or endorsed by DeepLethe.

Original copyright and Apache-2.0 licensing are preserved; Arcadia's changes are distributed under the same [Apache-2.0 license](LICENSE). The complete upstream Git history and the exact patch that produced Arcadia are preserved in `provenance/`. Upstream's own README is kept verbatim in [UPSTREAM.md](UPSTREAM.md).
