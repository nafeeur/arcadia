//! R1 tested against a real database. The pure-logic part (`utopia-reason::derive`)
//! already has 12 cases; this file pins down the four things it can't see:
//!
//! - **Assertion beats derivation.** A triple that's already asserted doesn't get
//!   a derived copy too
//! - **When a premise is retracted, the derivation invalidates with it** — and by
//!   setting `invalidated_at`, not deleting the row, because the record needs to
//!   keep "we once derived this, then the premise went away" (0002 section 3)
//! - **Proof survives storage.** `fact_derivations` records the direct premises by
//!   seq, and R2 unfolds it from there
//! - **Rule identity is stable across reruns.** Otherwise `rule_id` points at a new
//!   id every run and history breaks
//!
//! One more thing only a real database can verify: derived facts must pass the two
//! precision CHECKs on `facts`. When an intersection makes one end unbounded, that
//! end's precision must be cleared too, or the whole INSERT gets rejected.

use sqlx::PgPool;
use utopia_store::reasoning;
use uuid::Uuid;

struct Fixture {
    org: Uuid,
    kb: Uuid,
    part_of: Uuid,
    a: Uuid,
    b: Uuid,
    c: Uuid,
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fixture> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let etype = Uuid::now_v7();
    let part_of = Uuid::now_v7();
    let (a, b, c) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'derive-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'derive-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'derive-test')",
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
        "INSERT INTO relation_types (id, kb_id, key, label, is_transitive)
         VALUES ($1, $2, 'part_of', 'part of', TRUE)",
    )
    .bind(part_of)
    .bind(kb)
    .execute(pool)
    .await?;
    for (id, name) in [(a, "Acme"), (b, "Beta"), (c, "Cyrus")] {
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
        part_of,
        a,
        b,
        c,
    })
}

/// Insert an asserted fact, optionally with a span and precision.
async fn assert_fact(
    pool: &PgPool,
    f: &Fixture,
    s: Uuid,
    o: Uuid,
    span: Option<(&str, &str)>,
) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    match span {
        Some((from, prec)) => {
            sqlx::query(
                "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_id,
                                    valid_from, valid_from_precision)
                 VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7)",
            )
            .bind(id)
            .bind(f.kb)
            .bind(s)
            .bind(f.part_of)
            .bind(o)
            .bind(from)
            .bind(prec)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query(
                "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_id)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(id)
            .bind(f.kb)
            .bind(s)
            .bind(f.part_of)
            .bind(o)
            .execute(pool)
            .await?;
        }
    }
    Ok(id)
}

/// Live derived facts: (subject, object)
async fn live_derived(pool: &PgPool, kb: Uuid) -> anyhow::Result<Vec<(Uuid, Uuid)>> {
    Ok(sqlx::query_as(
        "SELECT subject_id, object_id FROM derived_facts
          WHERE kb_id = $1 AND invalidated_at IS NULL
          ORDER BY subject_id, object_id",
    )
    .bind(kb)
    .fetch_all(pool)
    .await?)
}

#[tokio::test]
async fn what_the_engine_adds_it_can_also_take_back() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // ---- 1. A ⊂ B ⊂ C ⟹ A ⊂ C
        let ab = assert_fact(&pool, &f, f.a, f.b, None).await?;
        let bc = assert_fact(&pool, &f, f.b, f.c, None).await?;
        let r = reasoning::materialize(&pool, f.kb).await?;
        assert_eq!(r.rules, 1, "one transitive rule");
        assert_eq!(r.edges, 2);
        assert_eq!(r.inserted, 1);
        assert_eq!(live_derived(&pool, f.kb).await?, vec![(f.a, f.c)]);

        // Proof: two premises, in order
        let derived: Uuid = sqlx::query_scalar("SELECT id FROM derived_facts WHERE kb_id = $1")
            .bind(f.kb)
            .fetch_one(&pool)
            .await?;
        let premises: Vec<Uuid> = sqlx::query_scalar(
            "SELECT premise_fact_id FROM fact_derivations
              WHERE derived_fact_id = $1 ORDER BY seq",
        )
        .bind(derived)
        .fetch_all(&pool)
        .await?;
        assert_eq!(premises, vec![ab, bc], "the proof must record direct premises in derivation order");

        // ---- 2. Rerunning is idempotent, rule id unchanged
        let rule_before: Uuid = sqlx::query_scalar("SELECT id FROM rules WHERE kb_id = $1")
            .bind(f.kb)
            .fetch_one(&pool)
            .await?;
        let again = reasoning::materialize(&pool, f.kb).await?;
        assert_eq!(again.inserted, 0, "the same derivation shouldn't insert another copy on a second run");
        assert_eq!(again.invalidated, 0);
        let rule_after: Uuid = sqlx::query_scalar("SELECT id FROM rules WHERE kb_id = $1")
            .bind(f.kb)
            .fetch_one(&pool)
            .await?;
        assert_eq!(rule_before, rule_after, "recompiling must recognize it as the same rule");

        // ---- 3. Assertion wins: once A ⊂ C is also asserted, the derived copy should step aside
        assert_fact(&pool, &f, f.a, f.c, None).await?;
        let asserted = reasoning::materialize(&pool, f.kb).await?;
        assert_eq!(asserted.derived, 0, "an asserted triple shouldn't also be derived");
        assert_eq!(asserted.invalidated, 1, "the previously derived one must be invalidated");
        assert!(live_derived(&pool, f.kb).await?.is_empty());
        // Invalidating isn't deleting — it stays on the record
        let ghost: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM derived_facts
              WHERE kb_id = $1 AND invalidated_at IS NOT NULL",
        )
        .bind(f.kb)
        .fetch_one(&pool)
        .await?;
        assert_eq!(ghost, 1, "invalidating a derivation must leave a trace, the same shape as rejecting a fact");

        // ---- 4. When the premise is retracted, the derivation follows
        sqlx::query(
            "UPDATE facts SET invalidated_at = now() WHERE subject_id = $1 AND object_id = $2",
        )
        .bind(f.a)
        .bind(f.c)
        .execute(&pool)
        .await?;
        let back = reasoning::materialize(&pool, f.kb).await?;
        assert_eq!(back.inserted, 1, "once the assertion is retracted, the derivation should come back");
        sqlx::query("UPDATE facts SET invalidated_at = now() WHERE id = $1")
            .bind(bc)
            .execute(&pool)
            .await?;
        let gone = reasoning::materialize(&pool, f.kb).await?;
        assert_eq!(gone.invalidated, 1, "once the premise is gone, the derivation must invalidate with it");
        assert!(live_derived(&pool, f.kb).await?.is_empty());

        // ---- 5. Precision: when the intersection makes the end unbounded, that end's
        // precision must be cleared too, or it hits facts_to_precision_matches_date
        sqlx::query("UPDATE facts SET invalidated_at = NULL WHERE id = $1")
            .bind(bc)
            .execute(&pool)
            .await?;
        sqlx::query(
            "UPDATE facts SET valid_from = '2020-01-01'::timestamptz,
                                      valid_from_precision = 'year' WHERE id = $1",
        )
        .bind(ab)
        .execute(&pool)
        .await?;
        sqlx::query(
            "UPDATE facts SET valid_from = '2022-06-01'::timestamptz,
                                      valid_from_precision = 'day' WHERE id = $1",
        )
        .bind(bc)
        .execute(&pool)
        .await?;
        let dated = reasoning::materialize(&pool, f.kb).await?;
        assert_eq!(dated.inserted, 1);
        let (from, fp, tp): (
            Option<chrono::DateTime<chrono::Utc>>,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT valid_from, valid_from_precision, valid_to_precision FROM derived_facts
              WHERE kb_id = $1 AND invalidated_at IS NULL",
        )
        .bind(f.kb)
        .fetch_one(&pool)
        .await?;
        assert_eq!(
            from.map(|x| x.format("%Y-%m-%d").to_string()).as_deref(),
            Some("2022-06-01"),
            "the intersection takes the later of the two start points"
        );
        // Precision follows the premise that wins this end (0024): the start point is
        // b ⊂ c's June 1st, so the precision is its day. It used to take the coarsest
        // precision across all premises — year, labeling a June 1st value — which
        // contradicted itself, and the database's CHECK wouldn't allow it now anyway
        assert_eq!(
            fp.as_deref(),
            Some("day"),
            "precision follows the premise that wins this end, not the coarsest one"
        );
        assert_eq!(tp, None, "the end is unbounded, so precision must be null");

        // ---- 6. The axiom is retracted: facts derived from it invalidate, while the rule row **stays**
        sqlx::query("UPDATE relation_types SET is_transitive = FALSE WHERE id = $1")
            .bind(f.part_of)
            .execute(&pool)
            .await?;
        let blind = reasoning::materialize(&pool, f.kb).await?;
        assert_eq!(blind.rules, 0, "without the axiom, no rule can be compiled");
        assert_eq!(blind.invalidated, 1, "facts derived from it must be invalidated");
        assert!(live_derived(&pool, f.kb).await?.is_empty());
        let rules_left: i64 = sqlx::query_scalar("SELECT count(*) FROM rules WHERE kb_id = $1")
            .bind(f.kb)
            .fetch_one(&pool)
            .await?;
        assert_eq!(
            rules_left, 1,
            "the rule row stays — the just-invalidated derivations still point to it, and explaining \"which rule this was derived from\" needs it to still exist"
        );
        // Add the axiom back, and the derivation should come back too (same rule id as before)
        sqlx::query("UPDATE relation_types SET is_transitive = TRUE WHERE id = $1")
            .bind(f.part_of)
            .execute(&pool)
            .await?;
        let revived = reasoning::materialize(&pool, f.kb).await?;
        assert_eq!(revived.rules, 1);
        assert_eq!(revived.inserted, 1, "axiom back, derivation back too");
        let rule_now: Uuid = sqlx::query_scalar("SELECT id FROM rules WHERE kb_id = $1")
            .bind(f.kb)
            .fetch_one(&pool)
            .await?;
        assert_eq!(rule_now, rule_before, "retracted then added back, still the same rule");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(f.org)
        .execute(&pool)
        .await?;
    run
}
