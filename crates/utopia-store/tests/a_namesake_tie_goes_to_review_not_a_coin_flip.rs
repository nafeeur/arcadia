//! A namesake tie in the gray zone must not be decided by a coin flip based on
//! candidate ordering (#270, continuing #221/#296).
//!
//! A kb already has two "Zhang Wei"s with identical profile embeddings (both grown
//! from the same chunk, which is common since #221). Then a "Zhang Wei" mention with
//! no handle at all comes in and scores identically against both candidates. The old
//! `resolve_mention` would, whenever the top score was >= `SIM_ATTACH`, merge straight
//! into **whichever one it encountered first** and open no review pair at all — who it
//! landed on depended entirely on the order candidates came back from the database.
//!
//! When it can't be told apart, don't force it: create a new entity, open a **human**
//! review pair against each of the tied candidates, and never merge silently. This can
//! only be tested against a real database — "whichever it encountered first" is the
//! physical row order under a default `ORDER BY`, invisible to `cargo check`. Skip
//! rather than fail when `UTOPIA_DATABASE_URL` is unset; self-contained, never touches
//! an existing database.

use sqlx::PgPool;
use utopia_store::resolution::ReviewStage;
use uuid::Uuid;

struct Fx {
    org: Uuid,
    kb: Uuid,
    person: Uuid,
    zhang_a: Uuid,
    zhang_b: Uuid,
}

/// Two namesake "Zhang Wei"s with identical profile embeddings; different employers,
/// but nothing here can use that signal right now.
async fn seed(pool: &PgPool) -> anyhow::Result<Fx> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (person, organization) = (Uuid::now_v7(), Uuid::now_v7());
    let works_for = Uuid::now_v7();
    let (platform, finance) = (Uuid::now_v7(), Uuid::now_v7());
    let (zhang_a, zhang_b) = (Uuid::now_v7(), Uuid::now_v7());

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'namesake-tie-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'namesake-tie-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'namesake-tie-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;
    for (id, key, label) in [
        (person, "person", "Person"),
        (organization, "organization", "Organization"),
    ] {
        sqlx::query("INSERT INTO entity_types (id, kb_id, key, label) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(kb)
            .bind(key)
            .bind(label)
            .execute(pool)
            .await?;
    }
    sqlx::query(
        "INSERT INTO relation_types (id, kb_id, key, label, temporal)
         VALUES ($1, $2, 'works_for', 'works for', 'state')",
    )
    .bind(works_for)
    .bind(kb)
    .execute(pool)
    .await?;

    // Two employers, two namesakes. Deliberately one capitalized and one not: recall
    // uses SQL `lower()`, so "Zhang Wei" and "zhang wei" are already a namesake pair,
    // and the tie decision must also be case-insensitive.
    for (id, type_id, name) in [
        (platform, organization, "Platform Engineering"),
        (finance, organization, "Finance"),
        (zhang_a, person, "Zhang Wei"),
        (zhang_b, person, "zhang wei"),
    ] {
        sqlx::query(
            "INSERT INTO entities (id, kb_id, type_id, canonical_name) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(kb)
        .bind(type_id)
        .bind(name)
        .execute(pool)
        .await?;
    }
    // Identical profile embeddings: both were seeded from the same chunk, same centroid
    for id in [zhang_a, zhang_b] {
        sqlx::query("UPDATE entities SET profile_embedding = '[1,0,0]'::vector, profile_n = 1 WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
    }
    // The fact that would tell them apart is sitting right here — department/employer —
    // profile comparison just can't see it
    for (subject, object) in [(zhang_a, platform), (zhang_b, finance)] {
        sqlx::query(
            "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_id, confidence)
             VALUES ($1, $2, $3, $4, $5, 0.9)",
        )
        .bind(Uuid::now_v7())
        .bind(kb)
        .bind(subject)
        .bind(works_for)
        .bind(object)
        .execute(pool)
        .await?;
    }

    Ok(Fx {
        org,
        kb,
        person,
        zhang_a,
        zhang_b,
    })
}

#[tokio::test]
async fn a_namesake_tie_creates_an_entity_and_two_reviews() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // Context consistent with both candidate profiles: cosine score against A and
        // against B is identical
        let ctx: Vec<f32> = vec![1.0, 0.0, 0.0];
        let r = utopia_store::resolution::resolve_mention(
            &pool,
            f.kb,
            Some(f.person),
            "Zhang Wei",
            Some(&ctx),
            None,
            &[],
        )
        .await?;

        // When it can't be told apart, don't merge: a third entity is created, not
        // attached to A or B
        assert!(
            r.created,
            "同名并列不该静默归并到先遇到的那个——该新建实体（#270）"
        );
        assert_ne!(r.entity_id, f.zhang_a, "attach 到了 A：候选顺序掷出的硬币");
        assert_ne!(r.entity_id, f.zhang_b, "attach 到了 B：候选顺序掷出的硬币");

        // A review pair is opened against each of the two tied candidates
        let mut reviewed: Vec<Uuid> = r.reviews.iter().map(|rv| rv.other_id).collect();
        reviewed.sort();
        let mut want = vec![f.zhang_a, f.zhang_b];
        want.sort();
        assert_eq!(reviewed, want, "两个同名候选都该进审核，一个都不能少");

        // Only a human can tell namesake ties apart; the batch adjudicator must never
        // auto-merge two nearly identical profiles
        assert!(
            r.reviews.iter().all(|rv| rv.stage == ReviewStage::Human),
            "同名并列的审核对必须是人工阶段（Human）"
        );

        // Once persisted, there really are two pending human review pairs, both
        // attached to the newly created entity
        for rv in &r.reviews {
            utopia_store::resolution::create_review(
                &pool,
                f.kb,
                r.entity_id,
                rv.other_id,
                rv.score,
                &rv.reason,
                rv.stage,
            )
            .await?;
        }
        let pending: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT CASE WHEN left_id = $2 THEN right_id ELSE left_id END, stage
             FROM resolution_reviews
             WHERE kb_id = $1 AND status = 'pending' AND (left_id = $2 OR right_id = $2)",
        )
        .bind(f.kb)
        .bind(r.entity_id)
        .fetch_all(&pool)
        .await?;
        assert_eq!(pending.len(), 2, "库里该有两条待裁的审核对");
        assert!(
            pending.iter().all(|(_, stage)| stage == "human"),
            "落库的审核对都该是 human 阶段"
        );
        let mut others: Vec<Uuid> = pending.iter().map(|(id, _)| *id).collect();
        others.sort();
        assert_eq!(others, want, "两条审核对分别指向 A 和 B");

        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(f.org)
        .execute(&pool)
        .await?;
    run
}
