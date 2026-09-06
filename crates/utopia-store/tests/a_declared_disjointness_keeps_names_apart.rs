//! 0016 B3: `owl:disjointWith` feeds resolution — once the ontology declares two
//! classes mutually exclusive, even a shared name no longer enters the review queue.
//!
//! Resolution decides "can these two classes refer to the same thing" on three
//! layers: the hard-coded `CONFUSABLE_TYPE_KEYS` table, the class hierarchy (same
//! lineage counts as confusable, #226), and ontology-declared disjointness. What's
//! guarded here is that **the declaration outranks the other two layers**:
//!
//! 1. Behavior is unchanged without a declaration: organization vs project still
//!    enters the queue via the hard-coded table; corporation vs federal_agency still
//!    enters via the class hierarchy (shared non-root ancestor organization).
//! 2. Once organization ⟂ project is declared, a same-named organization / project
//!    pair is kept apart and no longer enters the queue.
//! 3. Once corporation ⟂ agency is declared, federal_agency (a subclass of agency)
//!    is also kept apart from corporation — **disjointness is inherited**; declaring
//!    it on the parent is enough.
//!
//! Skips rather than fails when `UTOPIA_DATABASE_URL` is unset. Builds and tears
//! down its own data; never touches an existing database.

use sqlx::PgPool;
use utopia_store::{ontology, resolution};
use uuid::Uuid;

struct Fx {
    org: Uuid,
    kb: Uuid,
    organization: Uuid,
    project: Uuid,
    corporation: Uuid,
    agency: Uuid,
    federal_agency: Uuid,
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fx> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    // There must be a root at the top: the class-hierarchy rule's "shared ancestor"
    // doesn't count the root itself (in schema.org everything is a Thing, and
    // counting it would make Person and Organization kin too), so organization needs a parent class
    let (thing, organization, project, corporation, agency, federal_agency) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'disjoint-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'disjoint-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'disjoint-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;
    for (id, key) in [
        (thing, "thing"),
        (organization, "organization"),
        (project, "project"),
        (corporation, "corporation"),
        (agency, "agency"),
        (federal_agency, "federal_agency"),
    ] {
        sqlx::query("INSERT INTO entity_types (id, kb_id, key, label) VALUES ($1, $2, $3, $3)")
            .bind(id)
            .bind(kb)
            .bind(key)
            .execute(pool)
            .await?;
    }
    for (child, parent) in [
        (organization, thing),
        (project, thing),
        (corporation, organization),
        (agency, organization),
        (federal_agency, agency),
    ] {
        sqlx::query("INSERT INTO entity_type_parents (child_id, parent_id) VALUES ($1, $2)")
            .bind(child)
            .bind(parent)
            .execute(pool)
            .await?;
    }
    Ok(Fx {
        org,
        kb,
        organization,
        project,
        corporation,
        agency,
        federal_agency,
    })
}

async fn entity(pool: &PgPool, f: &Fx, name: &str, type_id: Uuid) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO entities (id, kb_id, type_id, canonical_name) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(f.kb)
    .bind(type_id)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Resolve a mention, returning who its "type drift" review pairs it against
async fn drift_reviews(
    pool: &PgPool,
    f: &Fx,
    name: &str,
    type_id: Uuid,
) -> anyhow::Result<Vec<Uuid>> {
    let r = resolution::resolve_mention(pool, f.kb, Some(type_id), name, None, None, &[]).await?;
    assert!(
        r.created,
        "a cross-type same name is a new entity: keep apart, never merge"
    );
    Ok(r.reviews
        .iter()
        .filter(|x| x.reason.starts_with("type_drift|"))
        .map(|x| x.other_id)
        .collect())
}

#[tokio::test]
async fn a_declared_disjointness_keeps_names_apart() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // 1. No declaration: the hard-coded table and class hierarchy behave as before
        let orion = entity(&pool, &f, "Orion", f.organization).await?;
        assert_eq!(
            drift_reviews(&pool, &f, "Orion", f.project).await?,
            vec![orion],
            "organization vs project is confusable by the hard-coded list"
        );
        let acme = entity(&pool, &f, "Acme", f.corporation).await?;
        assert_eq!(
            drift_reviews(&pool, &f, "Acme", f.federal_agency).await?,
            vec![acme],
            "corporation vs federal_agency share the ancestor organization: kin, so Review"
        );

        // 2. Declare organization ⟂ project: the hard-coded table says confusable, the ontology says disjoint -- the ontology wins
        ontology::set_disjoint_for(&pool, f.kb, f.organization, &[f.project]).await?;
        let _vega = entity(&pool, &f, "Vega", f.organization).await?;
        assert!(
            drift_reviews(&pool, &f, "Vega", f.project)
                .await?
                .is_empty(),
            "a declared disjointness wins over the hard-coded list"
        );

        // 3. Declare corporation ⟂ agency: federal_agency is a subclass of agency, so
        //    the disjointness is inherited -- the class hierarchy calling them kin no longer counts
        ontology::set_disjoint_for(&pool, f.kb, f.corporation, &[f.agency]).await?;
        let _beta = entity(&pool, &f, "Beta", f.corporation).await?;
        assert!(
            drift_reviews(&pool, &f, "Beta", f.federal_agency)
                .await?
                .is_empty(),
            "a disjointness declared on the parent reaches the child and wins over kinship"
        );
        // Asking the other way round holds too: the table has one row per direction, inheritance follows the ancestor chain on the other end
        let _gamma = entity(&pool, &f, "Gamma", f.federal_agency).await?;
        assert!(
            drift_reviews(&pool, &f, "Gamma", f.corporation)
                .await?
                .is_empty(),
            "the declaration holds from either side"
        );

        // 4. Retract the declaration, back to the undeclared behavior -- an edit must be revertible
        ontology::set_disjoint_for(&pool, f.kb, f.corporation, &[]).await?;
        let delta = entity(&pool, &f, "Delta", f.corporation).await?;
        assert_eq!(
            drift_reviews(&pool, &f, "Delta", f.federal_agency).await?,
            vec![delta],
            "with the declaration gone, kinship sends the pair to Review again"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    let _ = sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(f.org)
        .execute(&pool)
        .await;
    run
}
