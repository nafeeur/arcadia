# Arcadia

**A living record of knowledge, evidence and change.**

Arcadia is a substantial fork of [DeepLethe's Utopia](https://github.com/deeplethe/utopia): a self-hosted temporal knowledge graph and retrieval-augmented assistant. It adds a review workflow around changing knowledge, historical document retrieval, inspectable answer records, and a new visual identity.

The interface uses an evergreen navigation rail, warm paper surfaces, serif headings, an overview dashboard, side-by-side review panels and dedicated evidence workspaces. It retains the original graph, ingestion, ontology, SQL mapping, chat, review and account capabilities.

## Added in Arcadia

- **Historical document search:** retained Postgres chunk text supplies keyword recall at a specified record time. Time eligibility is checked before ranking. Graph browsing has a separate “as known at” control, including entity details.
- **Answer ledger:** assistant messages and private evidence traces save atomically. Inspect the answer, captured document text, tool exchange and model metadata; export JSON.
- **Change proposals:** editors stage plain-text document replacements. Review the original and proposed text, directly supported facts, dependent conclusions and your affected answers. Knowledge-base administrators approve or reject.
- **Fenced approval:** stale proposals cannot overwrite newer documents. Approval records a version, queues processing and appends an audit event in one transaction. Repeated approval cannot enqueue a second job.
- **Document replay and previews:** rerun a saved question against current or historical document evidence, or a pending proposal. Inspect the new answer, evidence and structural citation checks. Previewing does not mutate the live document.
- **Organization SSO:** OIDC authorization code flow with PKCE, nonce, one-use state and RS256 verification. Administrators link an existing account to the provider's exact subject identifier.
- **Recovery and measurement tools:** checksummed database-plus-file backup archives, guarded restore, and a read-only retrieval latency/recall probe.
- **Deletion handling:** purging a document redacts its copied trace/proposal evidence and removes affected replays. In-flight evidence saves are fenced against purge.

See [ARCADIA.md](ARCADIA.md) for the precise scope, setup, operational commands and verification limits. Original upstream documentation is preserved in [UPSTREAM.md](UPSTREAM.md); deployment instructions in this README take precedence for Arcadia.

## Run from source with Docker

Requires Docker Engine with Compose and enough memory/disk to compile the Rust application.

```sh
cp .env.example .env
docker compose --profile app up --build -d
```

Open **http://localhost:1516**, create the initial administrator account, configure chat/embedding models in workspace settings, and upload a document. Arcadia builds locally; it does not substitute the upstream Utopia application image.

Data lives in the `dbdata` Postgres volume and `./data` for source files, search index and the generated encryption key. Keep both when backing up. Existing Utopia deployments should make and verify an offline backup before applying Arcadia's additive migrations 0034 and 0035. Once applied, do not edit migration files.

## Local development

```sh
docker compose up -d db
cd web
pnpm install --frozen-lockfile
pnpm build
cd ..
cargo run -p utopia-server
```

The Rust crate/binary names, `UTOPIA_*` settings, database names and session cookie remain compatible with upstream. Product-facing branding is Arcadia. OIDC uses `ARCADIA_OIDC_*`; see `.env.example`.

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
UTOPIA_DATABASE_URL=postgres://utopia:utopia@localhost:1517/utopia \
  UTOPIA_TEST_REQUIRE_DB=1 cargo test --workspace -- --test-threads=1
python3 -m unittest discover -s scripts/arcadia -p 'test_*.py'
cd web && pnpm build
```

Start the application once against the test database to apply migrations before the full suite. Use an isolated test database. CI provisions Postgres 16 with pgvector and applies all migrations before its database tests.

## Attribution

Forked from Utopia by DeepLethe. Original copyright and Apache-2.0 licensing are preserved. Arcadia changes are distributed under the same [Apache-2.0 license](LICENSE). The upstream history remains intact in the working Git checkout. Arcadia is a separate derivative project, not an official DeepLethe release.
