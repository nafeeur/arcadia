# Security notes

*[English](SECURITY.md)*

Utopia is currently v0.1. Below are **known, unresolved** limitations — not a vulnerability report, but places the design hasn't reached yet.

## Before deploying to the public internet

**The default database password is `utopia`.** By default the port is bound only to loopback (`127.0.0.1:1517`), so it's unreachable from outside. If you change `UTOPIA_DB_BIND` to expose it, replace `UTOPIA_DB_PASSWORD` in `.env` first.

**A data source's security ceiling is its grant.** Registering a data source is a deployment-level action, and the connection string it carries reaches every workspace it's granted to. Only grant a source to workspaces that should see that database, and use a read-only database role in the connection string itself — the SQL gate described below is defense in depth, not a substitute for least privilege at the source.

## Already in place

- **Credentials encrypted at rest** — LLM API keys, query-over-data connection strings, and the tokens and push secrets in source configs are sealed with AES-256-GCM before reaching Postgres. The key never enters the database: `UTOPIA_SECRET_KEY`, or a `secret.key` generated under the data directory on first boot. Take the key along when backing up the data directory — without it these values can't be read back. Plaintext rows written by older versions are sealed retroactively on the next startup.
- **The JWT signing key is generated on first boot** — a 32-byte CSPRNG value stored in the database; there's no such thing as a default key shared across all deployments.
- **Session cookies get `Secure` automatically behind TLS** — decided from `X-Forwarded-Proto`, so local HTTP development still works as usual. When the proxy doesn't send that header, force it on with `UTOPIA_COOKIE_SECURE=true`.
- **The database port is bound to loopback only** — `127.0.0.1:1517`; the app reaches the database over the compose-internal network.
- **An optional restricted runtime role** — once `UTOPIA_APP_DB_PASSWORD` and `UTOPIA_MIGRATION_URL` are configured, the app connects with a role that can read/write business tables but only insert (never modify) the audit ledger; migrations run under a separate owner identity.
- **A data source only reaches workspaces it's been granted to** — a registered database can only be attached to a knowledge base where an explicit grant exists. Before that, any knowledge-base admin could attach any registered data source, which crosses tenant boundaries in a multi-workspace deployment.
- **Query-over-data goes through a read-only gate** — a parse allowlist, a read-only transaction, and an enforced row-count cap, three layers deep, so a statement that dodges the parser still can't write.
- **An account is deactivated, not deleted** — `users.deactivated_at` blocks login while decisions that person made remain attributable in the audit ledger.
- **Password hashing uses argon2.**

## Reporting a vulnerability

Please email **security@deeplethe.com** rather than opening a public issue. State the affected version or commit, the endpoint or component, and reproduction steps. Expect a reply within a few days; the release that ships the fix will credit you in its notes, unless you'd rather it didn't.
