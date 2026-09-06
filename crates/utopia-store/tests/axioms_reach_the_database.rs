//! Axioms declared by the ontology need to actually land in the database — run against
//! a real database.
//!
//! This guard exists because of a real defect that once existed:
//! `create_relation_types_bulk`'s doc comment says "`functional` / `inverse_functional`
//! must be written through exactly as the vocabulary declares them, never defaulted to
//! false," while its SQL hard-coded `FALSE, FALSE` — two arrays were bound but never
//! made it into the `UNNEST`. In practice, after importing FOAF (which declares 17
//! FunctionalProperty terms), **not a single one** ended up functional=true in the
//! database.
//!
//! Those two flags are what the temporal engine relies on to auto-close facts. Getting
//! the direction wrong has different consequences depending on which way: marking
//! false as true manufactures conflicts in bulk (the `part_of` incident, 59 of them),
//! while this is the opposite direction — everything that should be true became
//! false, so **temporal conflicts that should be detected go undetected**, silently.
//!
//! `cargo check` can't see this kind of bug (the types all check out), and unit tests
//! can't either (it lives inside a SQL string). Only writing a row for real and reading
//! it back catches it.

use sqlx::PgPool;
use uuid::Uuid;

/// Build a minimal knowledge base: whether axioms land correctly has nothing to do
/// with ontology size, one row is enough.
async fn kb(pool: &PgPool) -> anyhow::Result<(Uuid, Uuid)> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'ax-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'ax-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'ax-test')")
        .bind(kb)
        .bind(ws)
        .execute(pool)
        .await?;
    Ok((kb, org))
}

#[tokio::test]
async fn every_axiom_survives_the_bulk_insert() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let (kb_id, org) = kb(&pool).await?;

    let run = async {
        let row = |key: &str, f: bool, i: bool, t: bool, s: bool, a: bool, r: bool| {
            utopia_store::ontology::BulkRelation {
                key: key.into(),
                label: key.into(),
                description: String::new(),
                iri: format!("http://ax.test/#{key}"),
                kind: "relation",
                datatype: None,
                functional: f,
                inverse_functional: i,
                transitive: t,
                symmetric: s,
                asymmetric: a,
                irreflexive: r,
            }
        };
        // One row all-true, one row all-false: the all-false row guards "an undeclared flag must not come out true"
        utopia_store::ontology::create_relation_types_bulk(
            &pool,
            kb_id,
            &[
                row("all_true", true, true, true, true, true, true),
                row("all_false", false, false, false, false, false, false),
            ],
        )
        .await?;

        let got: Vec<(String, bool, bool, bool, bool, bool, bool)> = sqlx::query_as(
            "SELECT key, functional, inverse_functional,
                    is_transitive, is_symmetric, is_asymmetric, is_irreflexive
             FROM relation_types WHERE kb_id = $1 ORDER BY key",
        )
        .bind(kb_id)
        .fetch_all(&pool)
        .await?;

        let t = got
            .iter()
            .find(|r| r.0 == "all_true")
            .expect("all_true landed in the database");
        assert!(
            t.1 && t.2 && t.3 && t.4 && t.5 && t.6,
            "all six axiom flags should be written through as declared, got {t:?}"
        );
        let f = got
            .iter()
            .find(|r| r.0 == "all_false")
            .expect("all_false landed in the database");
        assert!(
            !f.1 && !f.2 && !f.3 && !f.4 && !f.5 && !f.6,
            "an undeclared axiom must not come out true, got {f:?}"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(org)
        .execute(&pool)
        .await?;
    run
}
