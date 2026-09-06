//! Types a human has ruled on — the engine may not change them. Runs against a real database.
//!
//! This entire line of defense lives in SQL `WHERE` clauses: three read sites
//! each need a `type_source <> 'human'` clause, and missing one is a silent
//! failure. `cargo check` sees none of it, and 0009 just tripped over the
//! `NULL <> uuid` pitfall in this exact area — the type system counts for
//! Rust, not for SQL.
//!
//! Four assertions, four paths:
//!
//! - Type resolution sourcing (`entities_for_type_resolution`) must not pick up entities a human has ruled on
//! - Adoption after the ontology grows a new class (`adopt_proposed_types`) must not overwrite a human's ruling
//! - Extraction upgrades must not assign a type to an entity a human has decided has none ← the 0009 x P4 intersection
//! - `retype_entities` uses its existing `actor` parameter to tell human from inferred
//!
//! Skipped, not failed, when `UTOPIA_DATABASE_URL` is unset. Builds and tears down its own data — never touches an existing database.

use sqlx::PgPool;
use uuid::Uuid;

struct Fx {
    org: Uuid,
    kb: Uuid,
    org_type: Uuid,
    sub_type: Uuid,
}

/// Builds an ontology that just barely triggers the "third sourcing condition": `organization` has a subclass `startup`.
/// Once a human has set an entity to `organization`, it's this subclass that makes it eligible for re-judgment.
async fn seed(pool: &PgPool) -> anyhow::Result<Fx> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (org_type, sub_type) = (Uuid::now_v7(), Uuid::now_v7());

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'p4a-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'p4a-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'p4a-test')")
        .bind(kb)
        .bind(ws)
        .execute(pool)
        .await?;
    for (id, key) in [(org_type, "organization"), (sub_type, "startup")] {
        sqlx::query("INSERT INTO entity_types (id, kb_id, key, label) VALUES ($1, $2, $3, $3)")
            .bind(id)
            .bind(kb)
            .bind(key)
            .execute(pool)
            .await?;
    }
    sqlx::query("INSERT INTO entity_type_parents (child_id, parent_id) VALUES ($1, $2)")
        .bind(sub_type)
        .bind(org_type)
        .execute(pool)
        .await?;
    Ok(Fx {
        org,
        kb,
        org_type,
        sub_type,
    })
}

async fn entity(
    pool: &PgPool,
    f: &Fx,
    name: &str,
    type_id: Option<Uuid>,
    source: &str,
) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO entities (id, kb_id, type_id, canonical_name, type_source)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(f.kb)
    .bind(type_id)
    .bind(name)
    .bind(source)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn source_of(pool: &PgPool, id: Uuid) -> anyhow::Result<String> {
    Ok(
        sqlx::query_scalar("SELECT type_source FROM entities WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await?,
    )
}

/// Main battleground: the sourcing condition's "include if the current type has subclasses" would also sweep up entities a human has already ruled on.
#[tokio::test]
async fn type_resolution_leaves_human_decisions_alone() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        let by_human = entity(&pool, &f, "Acme", Some(f.org_type), "human").await?;
        let by_engine = entity(&pool, &f, "Globex", Some(f.org_type), "extracted").await?;

        let picked =
            utopia_store::resolution::entities_for_type_resolution(&pool, f.kb, 100, false)
                .await?
                .into_iter()
                .map(|c| c.id)
                .collect::<std::collections::HashSet<_>>();

        assert!(
            picked.contains(&by_engine),
            "抽取定的类型该被重判——organization 有子类 startup，这正是消解的用武之地"
        );
        assert!(!picked.contains(&by_human), "人拍过板的不该被拿去重判");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    run
}

/// Adoption after the ontology grows a new class must likewise not overwrite a human's decision.
///
/// This also incidentally guarantees `unadopt_types` is correct: human rows never enter
/// the adoption batch, so undoing one never encounters them — no need to separately
/// restore `type_source`.
#[tokio::test]
async fn adopting_a_new_class_does_not_claim_human_typed_entities() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // Both entities were proposed as startup by the model, but one has a human-set type
        for (name, source) in [("Acme", "human"), ("Globex", "extracted")] {
            let id = entity(&pool, &f, name, Some(f.org_type), source).await?;
            sqlx::query("UPDATE entities SET proposed_type = 'startup' WHERE id = $1")
                .bind(id)
                .execute(&pool)
                .await?;
        }

        let (_, moved) = utopia_store::resolution::adopt_proposed_types(
            &pool,
            f.kb,
            f.sub_type,
            &["startup".to_string()],
            None,
        )
        .await?;
        assert_eq!(moved, 1, "只该认领那个不是人定的");

        let human: Option<Uuid> = sqlx::query_scalar(
            "SELECT type_id FROM entities WHERE kb_id = $1 AND canonical_name = 'Acme'",
        )
        .bind(f.kb)
        .fetch_one(&pool)
        .await?;
        assert_eq!(human, Some(f.org_type), "人定的类型原样未动");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    run
}

/// **The 0009 x P4 intersection, the easiest one to miss.**
///
/// After 0009, "no type" can be a human decision — they looked at the entity and decided
/// no class in the ontology fits. But extraction upgrade's guard originally only checked
/// `type_key.is_none()`, which can't tell "not judged yet" from "a human judged it and
/// decided there is none" — so the next extraction pass would assign it a type anyway.
#[tokio::test]
async fn extraction_does_not_fill_in_a_type_a_human_left_empty() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // A human looked at it and decided no class in the ontology fits -> no type, and it's a decision
        let decided = entity(&pool, &f, "Ambiguous Thing", None, "human").await?;
        // Not judged yet
        let pending = entity(&pool, &f, "Other Thing", None, "extracted").await?;

        // **The upgrade branch only runs when there's a profile similarity to compare**:
        // with an empty ctx, best is None and the whole block is skipped. The first
        // version was written this way — removing the guard made no test fail, because
        // the test wasn't exercising what it claimed to test
        let ctx: Vec<f32> = vec![1.0, 0.0, 0.0];
        for id in [decided, pending] {
            sqlx::query(
                "UPDATE entities SET profile_embedding = $2::vector, profile_n = 1 WHERE id = $1",
            )
            .bind(id)
            .bind("[1,0,0]")
            .execute(&pool)
            .await?;
        }

        // Extraction encounters the same-named mention again and resolves it to organization. Cosine = 1.0, well above SIM_ATTACH
        for id in [decided, pending] {
            let name: String =
                sqlx::query_scalar("SELECT canonical_name FROM entities WHERE id = $1")
                    .bind(id)
                    .fetch_one(&pool)
                    .await?;
            let _ = utopia_store::resolution::resolve_mention(
                &pool,
                f.kb,
                Some(f.org_type),
                &name,
                Some(&ctx),
                None,
                &[],
            )
            .await?;
        }

        // Control: the one nobody has ruled on **should** be upgraded, otherwise this test can't prove the guard is doing anything
        let after_pending: Option<Uuid> =
            sqlx::query_scalar("SELECT type_id FROM entities WHERE id = $1")
                .bind(pending)
                .fetch_one(&pool)
                .await?;
        assert_eq!(
            after_pending,
            Some(f.org_type),
            "还没判过的该被抽取升格——不然下面那条断言是空的"
        );

        let after_decided: Option<Uuid> =
            sqlx::query_scalar("SELECT type_id FROM entities WHERE id = $1")
                .bind(decided)
                .fetch_one(&pool)
                .await?;
        assert_eq!(
            after_decided, None,
            "人说过「就是没有类型」，抽取不该替他填一个"
        );
        assert_eq!(source_of(&pool, decided).await?, "human");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    run
}

/// `retype_entities` uses its existing `actor` parameter to distinguish source: a human's
/// approval click is an endorsement and is protected; the engine's automatic ruling isn't.
/// No new parameter needed for this — it was already there when #112 added actor.
#[tokio::test]
async fn who_approved_a_retype_decides_whether_it_is_protected() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;
    let actor = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, org_id, email, display_name, password_hash)
         VALUES ($1, $2, $1 || '@p4a.test', 'p4a', 'x')",
    )
    .bind(actor)
    .bind(f.org)
    .execute(&pool)
    .await?;

    let run = async {
        let a = entity(&pool, &f, "Approved", Some(f.org_type), "extracted").await?;
        let b = entity(&pool, &f, "Auto", Some(f.org_type), "extracted").await?;

        utopia_store::resolution::retype_entities(&pool, f.kb, &[(a, f.sub_type)], Some(actor))
            .await?;
        utopia_store::resolution::retype_entities(&pool, f.kb, &[(b, f.sub_type)], None).await?;

        assert_eq!(source_of(&pool, a).await?, "human", "人点的批准是背书");
        assert_eq!(source_of(&pool, b).await?, "inferred", "引擎自动裁决的不是");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(actor)
        .execute(&pool)
        .await?;
    run
}

/// **An entity the engine has retyped must still be pickable up next round.**
///
/// This guards against a real incident: when persisting type resolution, "the person who
/// clicked run" was passed as the `actor` for `retype_entities`, and having an actor meant
/// writing `type_source = 'human'`. So **an entity that had gone through resolution once
/// would never be resolved again** — a database with no manual PATCH records at all would,
/// after one run, return an empty preview list on the next, with no error at all.
///
/// "Who clicked run" and "who decided what this entity is" are two different things. The
/// former belongs in the `ontology.types_resolved` audit log; only the latter should decide
/// `type_source`.
#[tokio::test]
async fn an_engine_retype_does_not_lock_the_entity_out_of_the_next_round() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        let e = entity(&pool, &f, "Initech", Some(f.org_type), "extracted").await?;
        // Keep a specific_type: makes it perpetually eligible under the sourcing condition,
        // so exclusion depends only on type_source — otherwise it would already be excluded
        // once retyped to a leaf class, and this test would prove nothing
        sqlx::query("UPDATE entities SET specific_type = 'startup company' WHERE id = $1")
            .bind(e)
            .execute(&pool)
            .await?;
        // Engine's automatic ruling: no human endorsed this one
        utopia_store::resolution::retype_entities(&pool, f.kb, &[(e, f.sub_type)], None).await?;

        assert_eq!(
            source_of(&pool, e).await?,
            "inferred",
            "引擎裁决不是人的背书"
        );

        let picked =
            utopia_store::resolution::entities_for_type_resolution(&pool, f.kb, 100, false)
                .await?
                .into_iter()
                .map(|c| c.id)
                .collect::<std::collections::HashSet<_>>();
        assert!(
            picked.contains(&e),
            "引擎改过一次不该把实体锁死——本体还会长大,它还得能被重判"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    run
}
