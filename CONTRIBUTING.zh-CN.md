# Contributing to Utopia

[English](CONTRIBUTING.md)

Welcome. This document only covers the rules specific to this repository — general open-source etiquette isn't repeated here.

## Branching and merge flow

| Branch | What it is |
|---|---|
| `main` | The stable branch, tracking released versions. Only merged into from `dev`, and only by a maintainer |
| `dev` | The integration branch; all contributions land here first |

A contributor's path:

```bash
git switch dev && git pull
git switch -c fix/some-thing        # branch from dev, not from main
# make your changes, commit with -s (see DCO below)
git push -u origin fix/some-thing
```

Then open a PR with **base set to `dev`, not `main`**. A maintainer merges it once CI and review pass.

The `dev → main` merge is initiated by a maintainer on their own schedule; contributors don't need to worry about it. Two rules keep the branches from drifting apart, both aimed at maintainers:

- **Merge `main` back into `dev` right after a release.** The `dev → main` merge commit exists only on `main`; without merging it back, `dev` will show as ahead even though the files are identical on both sides — and it falls one more merge behind with every release.
- **Hotfixes go through `dev` too.** Opening a PR straight against `main` is the one thing that can genuinely fork the two branches apart, and once that happens someone has to reconcile them by hand.

Both branches are protected: changes must go through a PR, CI (`backend` and `web`) must pass, force-push and deletion are disabled, and maintainers are bound by the same rules.

## Open an issue first, or just send a PR

| Change | What to do |
|---|---|
| Bug fix, docs, i18n copy, tests | Send a PR directly |
| New feature, dependency change | Open an [issue](https://github.com/deeplethe/utopia/issues) describing the scenario first |
| Data model, ontology contract, public API | Discuss in an issue first, write up an [ADR](docs/decisions/), then start work |

`docs/decisions/` is this project's primary decision record. It doesn't record "what changed" — it records **"why this way and not that way,"** along with what was tried and failed at the time. For a change of any size, that document ends up worth more than the code itself.

## Setting up locally

Dependencies: Docker, Rust 1.85+, Node 20+, pnpm.

```bash
docker compose up -d db                 # Postgres with pgvector
cargo run -p utopia-server              # runs migrations automatically, :1516
cd web && pnpm install && pnpm dev      # :5173, /api proxies to the backend
```

## What to run before submitting

CI is exactly the following; if these pass locally, CI is essentially guaranteed to pass too:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && pnpm install --frozen-lockfile && pnpm build   # build includes type-checking
```

### Tests that need a database **skip** without the env var — they don't fail

This is the easiest thing to misjudge in this repository. A fully green `cargo test --workspace` doesn't mean everything ran — a batch of tests starts like this:

```rust
let Ok(url) = std::env::var("UTOPIA_DATABASE_URL") else {
    eprintln!("skipping: UTOPIA_DATABASE_URL not set");
    return Ok(());
};
```

They guard things **the compiler can't see**: table aliases in SQL, how `NULL` behaves in a comparison, rows an `INNER JOIN` silently drops, whether a recursive CTE expands the same ancestor twice under diamond inheritance. `cargo check` and clippy say nothing about any of this.

If you touched SQL under `crates/utopia-store/`, set the variable and run the suite again:

```bash
export UTOPIA_DATABASE_URL=postgres://utopia:utopia@localhost:5432/utopia
cargo test --workspace
```

## A few things review will catch

**Don't collide migration numbers.** `migrations/` rolls forward by sequence number. Check the latest number on `main` before opening a PR — two branches each writing an `0011_` has happened before, and after the merge neither one runs.

**UI copy goes into i18n.** Add it to both `web/src/i18n/en.ts` and `zh.ts` — don't hardcode strings in components.

**Comments explain why.** This repository's comment density runs high on purpose, and it deliberately records the pitfalls that were already hit ("the first version used 'or,' and it turned out the Elon Musk article pulls a match every 6KB"). Follow that style — a comment that just restates what the code does will be asked to be removed.

**Commit messages: one line, in English, stating the motivation.** No long body. Look at `git log` to get the tone.

**Every workflow declares its own `permissions:`.** The repository default is currently read-write — one workflow needs to commit a generated graph back. A workflow that omits a `permissions:` block inherits that default, so a job that only needs to read quietly ends up with write access too. Write the minimal set the job actually needs: a build- or test-only job gets `contents: read`.

## DCO: every commit needs a sign-off

We use the [DCO](https://developercertificate.org/) (Developer Certificate of Origin), not a CLA. You keep copyright on your own code; you're just certifying you have the right to submit it under Apache-2.0.

Commit with `-s` and git adds the sign-off line automatically:

```bash
git commit -s -m "Fix the thing"
```

This appends to the commit message:

```
Signed-off-by: Your Name <your@email>
```

If you forgot to sign off: for the last commit, `git commit --amend -s`; for several commits, `git rebase --signoff HEAD~3` (swap in the actual count), then `git push -f`.

The name and email used for sign-off should be real and reachable.

## License

By submitting, you agree your contribution is released under [Apache-2.0](LICENSE).
