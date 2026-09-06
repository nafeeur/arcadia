# Verification

This record describes checks performed during the Arcadia build session. It is not a production certification.

| Check | Result |
| --- | --- |
| Rust formatting | `cargo fmt --all --check` passed. |
| Rust static checks | `cargo clippy --workspace --all-targets -- -D warnings` passed. |
| Rust library/binary suite | `cargo test --workspace --lib --bins --quiet` completed successfully. Cargo reported 362 passing tests and one ignored live-network test. Five database-dependent tests in that invocation returned early because no database was configured; this number must not be interpreted as 362 executed behavioral checks. |
| New storage acceptance | Passed separately with a database: historical lexical/vector recall, boundary handling, KB and trace-owner isolation, original proposal text, stale/repeated approval rejection, exactly one version/job/audit event, rejection, purge redaction and refusal to save purged evidence. |
| New HTTP acceptance | Passed separately with a database and local model fixture: unauthenticated access, owner isolation, viewer/editor/admin permissions, historical replay, proposal preview, saved comparisons and approval. The fixture is deterministic and does not measure real model quality. |
| OIDC verification | Local tests passed for URL policy, RS256 signatures, issuer/audience, expiry, issued-at time, nonce, subject, authorized-party claim, unsupported algorithm and unknown key ID. Test-only RSA material is included in the fixtures directory; it is never an application credential. |
| Operator tools | Six Python tests passed: archive/key/blob preservation, checksums, unsafe path/link rejection, occupied database refusal, retrieval report calculation and redirect refusal. Database dump/restore is mocked in these tests. |
| Frontend | TypeScript typecheck, style guard (45 files) and Vite production build passed. The build retains a warning for an approximately 804 KB uncompressed initial JavaScript chunk. Several heavy feature pages are loaded separately. |
| Patch hygiene | `git diff --check` passed. |

## Database environment limitation

Native Postgres could not be started under this environment's process restrictions. The focused acceptance tests ran against PGlite 0.5.8 with pgvector extension 0.0.9 and pglite-socket 0.2.11, through a test adapter suppressing unsolicited extended-protocol `ReadyForQuery` messages. Each focused test used an isolated database runtime and one SQLx connection.

An attempted full workspace run through that shared PGlite socket failed on prepared-statement collisions across connection lifetimes. This is an unsupported test-runtime limitation, and the full native Postgres integration suite is **not claimed to have passed**. The focused tests were rerun independently and passed. Native Postgres 16/pgvector CI is configured to run the store and server suites, but that remote CI job was not executed during this session.

## Still requires a deployment environment

- Native Postgres full-suite and concurrent-writer validation.
- Docker image build/boot and a complete native database-plus-files restore drill.
- Browser visual, keyboard and assistive-technology testing.
- Actual OIDC provider registration and sign-in.
- Live model quality/cost evaluation and live external-connector compatibility.
- Measurements on a representative production-sized corpus.

No hosted deployment was created. Run instructions and operator commands are in `README.md` and `ARCADIA.md`. The included logs are evidence for the specific checks above, not a general claim that every feature or deployment condition is verified.
