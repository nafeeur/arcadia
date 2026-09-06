//! Retrieval candidates must **bring their ancestors with them**, or the prompt has no generalized base class to work with.
//!
//! Why this needs a real database: `ancestors_of` lives entirely inside a recursive SQL query — whether
//! multiple inheritance and diamonds work, or whether the same ancestor gets expanded twice, is something
//! `cargo check` says nothing about.
//!
//! What this guards is a causal chain observed in practice: vector retrieval naturally favors leaf classes
//! that appear literally in the text (a chunk about Sutskever, out of 976 classes, ranks `researcher` 4th
//! and `person` 359th) — not one generalized base class makes the top 40. Two symptoms then show up together:
//! the entity gets classified as `researcher` (which, in schema.org, is a subclass of `Audience`), while
//! `employee (organization → person)`'s signature degrades to `(* → *)`, because the model never saw the
//! direction constraint at all.
//!
//! This floor used to be propped up by "built-in classes always exist"; once the seed classes were retired,
//! the guarantee was left hanging.
//!
//! Skips rather than fails when `UTOPIA_DATABASE_URL` is absent. Self-seeding, self-cleaning.

use sqlx::PgPool;
use uuid::Uuid;

/// Build a diamond: `researcher → audience → thing`, `corporation → organization → thing`,
/// plus a multiply-inherited `agent` (hung under both thing and organization).
///
/// The diamond is the point: a recursion without dedup would expand `thing` twice.
async fn seed(pool: &PgPool) -> anyhow::Result<(Uuid, Vec<(&'static str, Uuid)>)> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'floor-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'floor-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'floor-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;

    let keys = [
        "thing",
        "audience",
        "researcher",
        "organization",
        "corporation",
        "agent",
    ];
    let mut ids: Vec<(&'static str, Uuid)> = Vec::new();
    for k in keys {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO entity_types (id, kb_id, key, label) VALUES ($1, $2, $3, $3)")
            .bind(id)
            .bind(kb)
            .bind(k)
            .execute(pool)
            .await?;
        ids.push((k, id));
    }
    let get = |k: &str| ids.iter().find(|(n, _)| *n == k).unwrap().1;
    for (child, parent) in [
        ("audience", "thing"),
        ("researcher", "audience"),
        ("organization", "thing"),
        ("corporation", "organization"),
        // Multiple inheritance + diamond: both of agent's paths lead to thing
        ("agent", "thing"),
        ("agent", "organization"),
    ] {
        sqlx::query(
            "INSERT INTO entity_type_parents (child_id, parent_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(get(child))
        .bind(get(parent))
        .execute(pool)
        .await?;
    }
    Ok((kb, ids))
}

#[tokio::test]
async fn a_retrieved_leaf_brings_its_ancestors() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    // Sweep before starting: an assertion panic would skip teardown
    sqlx::query("DELETE FROM organizations WHERE name = 'floor-test'")
        .execute(&pool)
        .await?;
    let (kb, ids) = seed(&pool).await?;
    let id = |k: &str| ids.iter().find(|(n, _)| *n == k).unwrap().1;

    let run = async {
        // Retrieval only pulled up the leaves — the text says "a researcher at the corporation"
        let leaves = vec![id("researcher"), id("corporation")];
        let anc = utopia_store::ontology::ancestors_of(&pool, &leaves).await?;

        for k in ["audience", "thing", "organization"] {
            assert!(anc.contains(&id(k)), "祖先里少了 {k}——地板没补上");
        }
        // A class is not its own ancestor: the caller will union both sets, so duplication is pure noise
        for k in ["researcher", "corporation"] {
            assert!(!anc.contains(&id(k)), "{k} 是它自己，不该出现在祖先里");
        }

        // **Diamond dedup**: agent has two paths to thing, and thing should appear only once
        let anc2 = utopia_store::ontology::ancestors_of(&pool, &[id("agent")]).await?;
        let things = anc2.iter().filter(|x| **x == id("thing")).count();
        assert_eq!(things, 1, "菱形继承把 thing 展开了 {things} 次");
        assert!(anc2.contains(&id("organization")), "多继承的另一条腿丢了");

        // Empty input must not blow up, and must not scan the whole table
        assert!(utopia_store::ontology::ancestors_of(&pool, &[])
            .await?
            .is_empty());
        Ok::<(), anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM organizations WHERE name = 'floor-test'")
        .execute(&pool)
        .await?;
    let _ = kb;
    run
}
