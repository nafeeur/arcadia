//! Consistency checking run against a real database: pull data, judge, persist, rerun.
//!
//! The pure-logic half already has 12 cases in `utopia-reason` that run without a
//! database. What's pinned here is the layer that's otherwise invisible, four things
//! that each live in SQL or a table constraint:
//!
//! - **The three filters on data pulled in**. Facts that were superseded, facts with
//!   no predicate, and attribute facts whose object is a literal value — none of these
//!   should participate; axioms are about relations between entities
//! - **No axioms, no verdict**. A knowledge base that never imported an ontology
//!   package runs out to zero — that's a true state, not a failure
//! - **Rerun is idempotent**. The same contradiction isn't inserted twice, and a row a
//!   human has already dispositioned survives a rerun (`ontology_proposals` tripped on
//!   this once: a rerun flushed a rejected proposal back to pending)
//! - **Stale rows get cleared**. Once a fact is retracted, its violation shouldn't
//!   still be sitting on the Review page
//!
//! Skips rather than fails when `UTOPIA_DATABASE_URL` is unset. Builds and tears down
//! its own data.

use sqlx::PgPool;
use utopia_store::reasoning;
use uuid::Uuid;

struct Fixture {
    org: Uuid,
    kb: Uuid,
    /// declares asymmetric + irreflexive
    owns: Uuid,
    /// declares no axioms at all
    mentions: Uuid,
    a: Uuid,
    b: Uuid,
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fixture> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let etype = Uuid::now_v7();
    let (owns, mentions) = (Uuid::now_v7(), Uuid::now_v7());
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'axioms-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'axioms-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'axioms-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO entity_types (id, kb_id, key, label) VALUES ($1, $2, 'thing', 'Thing')",
    )
    .bind(etype)
    .bind(kb)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO relation_types (id, kb_id, key, label, is_asymmetric, is_irreflexive)
         VALUES ($1, $2, 'owns', 'owns', TRUE, TRUE)",
    )
    .bind(owns)
    .bind(kb)
    .execute(pool)
    .await?;
    // No axioms declared at all — its edges should never be judged as a contradiction
    sqlx::query(
        "INSERT INTO relation_types (id, kb_id, key, label) VALUES ($1, $2, 'mentions', 'mentions')",
    )
    .bind(mentions)
    .bind(kb)
    .execute(pool)
    .await?;
    for (id, name) in [(a, "Acme"), (b, "Beta")] {
        sqlx::query(
            "INSERT INTO entities (id, kb_id, type_id, canonical_name) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(kb)
        .bind(etype)
        .bind(name)
        .execute(pool)
        .await?;
    }
    Ok(Fixture {
        org,
        kb,
        owns,
        mentions,
        a,
        b,
    })
}

/// Persist a relation fact, return its id.
async fn fact(pool: &PgPool, kb: Uuid, s: Uuid, p: Uuid, o: Uuid) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_id)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(kb)
    .bind(s)
    .bind(p)
    .bind(o)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn open_kinds(pool: &PgPool, kb: Uuid) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT kind FROM axiom_violations WHERE kb_id = $1 AND status = 'open' ORDER BY kind",
    )
    .bind(kb)
    .fetch_all(pool)
    .await?)
}

#[tokio::test]
async fn the_ontology_is_the_only_judge() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // ---- 1. A database with no contradictions runs out to zero
        let quiet = fact(&pool, f.kb, f.a, f.owns, f.b).await?;
        let r = reasoning::run(&pool, f.kb).await?;
        assert_eq!(r.edges, 1);
        assert_eq!(r.predicates_with_axioms, 1, "mentions declares no axioms, it shouldn't be counted");
        assert_eq!(r.found, 0, "a single one-directional owns constitutes no contradiction");

        // ---- 2. Only what the axioms say counts
        // B owns A — forms an asymmetry violation with the fact above
        let back = fact(&pool, f.kb, f.b, f.owns, f.a).await?;
        // A mentions B / B mentions A — bidirectional, but mentions has no axioms, shouldn't be reported
        fact(&pool, f.kb, f.a, f.mentions, f.b).await?;
        fact(&pool, f.kb, f.b, f.mentions, f.a).await?;
        // A owns A — self-loop violation
        let loop_fact = fact(&pool, f.kb, f.a, f.owns, f.a).await?;

        let r = reasoning::run(&pool, f.kb).await?;
        assert_eq!(r.edges, 5);
        assert_eq!(
            open_kinds(&pool, f.kb).await?,
            vec!["asymmetry", "self_loop"],
            "the bidirectional mentions shouldn't be reported — its predicate carries no axioms"
        );
        assert_eq!(r.inserted, 2);

        // For the self-loop: both columns point at the same fact
        let (l, rr): (Uuid, Uuid) = sqlx::query_as(
            "SELECT left_fact, right_fact FROM axiom_violations
              WHERE kb_id = $1 AND kind = 'self_loop'",
        )
        .bind(f.kb)
        .fetch_one(&pool)
        .await?;
        assert_eq!(l, loop_fact);
        assert_eq!(l, rr, "a fact contradicting itself doesn't need a second fact");

        // ---- 3. Rerunning doesn't double-insert
        let again = reasoning::run(&pool, f.kb).await?;
        assert_eq!(again.found, 2);
        assert_eq!(again.inserted, 0, "the same contradiction shouldn't be inserted again on a second run");
        assert_eq!(again.cleared, 0, "and shouldn't clear the previous round's rows either");

        // ---- 4. A row a human has already dispositioned survives a rerun
        sqlx::query(
            "UPDATE axiom_violations SET status = 'resolved', resolution = 'accepted'
              WHERE kb_id = $1 AND kind = 'asymmetry'",
        )
        .bind(f.kb)
        .execute(&pool)
        .await?;
        let after = reasoning::run(&pool, f.kb).await?;
        assert_eq!(after.inserted, 0, "a row someone already dispositioned shouldn't get reinserted");
        let resolved: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM axiom_violations WHERE kb_id = $1 AND status = 'resolved'",
        )
        .bind(f.kb)
        .fetch_one(&pool)
        .await?;
        assert_eq!(resolved, 1, "a human decision must survive a rerun");

        // ---- 5. Once a fact is retracted, its stale open row gets cleared
        sqlx::query("UPDATE facts SET invalidated_at = now() WHERE id = $1")
            .bind(loop_fact)
            .execute(&pool)
            .await?;
        let swept = reasoning::run(&pool, f.kb).await?;
        assert_eq!(swept.cleared, 1, "a superseded fact shouldn't still carry a violation");
        assert!(
            !open_kinds(&pool, f.kb).await?.contains(&"self_loop".to_string()),
            "once that fact is retracted the self-loop violation no longer holds"
        );

        // ---- 6. Attribute facts don't participate: the object is a literal, axioms are about relations between entities
        sqlx::query(
            "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_value)
             VALUES ($1, $2, $3, $4, '\"2015\"'::jsonb)",
        )
        .bind(Uuid::now_v7())
        .bind(f.kb)
        .bind(f.a)
        .bind(f.owns)
        .execute(&pool)
        .await?;
        let attrs = reasoning::run(&pool, f.kb).await?;
        assert_eq!(attrs.edges, 4, "the literal-valued-object row shouldn't be pulled in as an edge");

        // ---- 7. A knowledge base with no ontology package: the verdict is "no criteria," not "no contradictions"
        sqlx::query("UPDATE relation_types SET is_asymmetric = FALSE, is_irreflexive = FALSE WHERE kb_id = $1")
            .bind(f.kb)
            .execute(&pool)
            .await?;
        let blind = reasoning::run(&pool, f.kb).await?;
        assert_eq!(blind.predicates_with_axioms, 0);
        assert_eq!(blind.found, 0);
        assert!(
            open_kinds(&pool, f.kb).await?.is_empty(),
            "once the axioms are withdrawn, the violations reported under them should go too"
        );
        let _ = (quiet, back);
        Ok::<_, anyhow::Error>(())
    }
    .await;

    // Delete the org too — deleting just the kb would leave organizations / workspaces behind in the dev database
    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(f.org)
        .execute(&pool)
        .await?;
    run
}
