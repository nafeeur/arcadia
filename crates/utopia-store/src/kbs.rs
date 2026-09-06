use sqlx::PgPool;
use utopia_core::models::{KnowledgeBase, Readiness};
use utopia_core::{AppError, AppResult};
use uuid::Uuid;

pub async fn list(pool: &PgPool, workspace_id: Uuid) -> AppResult<Vec<KnowledgeBase>> {
    let rows =
        sqlx::query_as("SELECT * FROM knowledge_bases WHERE workspace_id = $1 ORDER BY created_at")
            .bind(workspace_id)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

/// KBs visible to the user: sysadmins see everything; everyone else sees
/// open KBs plus restricted KBs where they're in the matrix.
pub async fn list_visible(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    is_admin: bool,
) -> AppResult<Vec<KnowledgeBase>> {
    let rows = sqlx::query_as(
        "SELECT * FROM knowledge_bases k
         WHERE k.workspace_id = $1
           AND ($3
                OR k.visibility = 'open'
                OR EXISTS (SELECT 1 FROM kb_members m
                           WHERE m.kb_id = k.id AND m.user_id = $2))
         ORDER BY k.created_at",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(is_admin)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    kind: &str,
    description: Option<&str>,
) -> AppResult<KnowledgeBase> {
    // The deployment's first KB automatically becomes the default: a public
    // space, always open, undeletable (enforced by both the API and a DB CHECK)
    //
    // ontology_lang takes the deployment default: a Chinese deployment shouldn't
    // need a manual pick on every new KB. It's changeable per KB afterward —
    // a single deployment can well have one KB reading Chinese contracts and
    // another reading English papers
    let kb = sqlx::query_as(
        "INSERT INTO knowledge_bases
             (id, workspace_id, name, kind, description, is_default, ontology_lang)
         VALUES ($1, $2, $3, $4, $5,
                 NOT EXISTS (SELECT 1 FROM knowledge_bases WHERE workspace_id = $2),
                 COALESCE((SELECT default_ontology_lang FROM deployment_settings LIMIT 1), 'en'))
         RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(name)
    .bind(kind)
    .bind(description)
    .fetch_one(pool)
    .await?;
    Ok(kb)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<KnowledgeBase> {
    sqlx::query_as("SELECT * FROM knowledge_bases WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

#[allow(clippy::too_many_arguments)]
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    visibility: Option<&str>,
    auto_extend_ontology: Option<bool>,
    ontology_lang: Option<&str>,
    materialize_inferences: Option<bool>,
    inference_interval_minutes: Option<i32>,
    auto_type_resolution: Option<bool>,
) -> AppResult<KnowledgeBase> {
    // Changing the language doesn't rewrite existing classes retroactively —
    // they're already this KB's data, and someone may have hand-tuned them.
    // From here on this column only governs what language **new** descriptions
    // (auto ontology extension, AI suggestions) get written in
    if let Some(l) = ontology_lang {
        if !matches!(l, "en" | "zh") {
            return Err(AppError::invalid("bad_lang", "language must be en or zh"));
        }
    }
    if let Some(v) = visibility {
        if !matches!(v, "open" | "restricted") {
            return Err(AppError::Validation(
                "visibility must be open or restricted".into(),
            ));
        }
        // The default KB is always open: the public-space semantics must be
        // reliable (renaming/re-describing it is unrestricted)
        if v == "restricted" {
            let current = get(pool, id).await?;
            if current.is_default {
                return Err(AppError::invalid(
                    "default_kb_open",
                    "The default knowledge base stays open to everyone.",
                ));
            }
        }
    }
    sqlx::query_as(
        "UPDATE knowledge_bases
         SET name = COALESCE($2, name),
             description = COALESCE($3, description),
             visibility = COALESCE($4, visibility),
             auto_extend_ontology = COALESCE($5, auto_extend_ontology),
             ontology_lang = COALESCE($6, ontology_lang),
             materialize_inferences = COALESCE($7, materialize_inferences),
             inference_interval_minutes = COALESCE($8, inference_interval_minutes),
             auto_type_resolution = COALESCE($9, auto_type_resolution),
             updated_at = now()
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(visibility)
    .bind(auto_extend_ontology)
    .bind(ontology_lang)
    .bind(materialize_inferences)
    .bind(inference_interval_minutes)
    .bind(auto_type_resolution)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn readiness(pool: &PgPool, kb_id: Uuid) -> AppResult<Readiness> {
    // One query gets everything; even if all four panels each fire a request,
    // each is still just a single point lookup
    let r: Readiness = sqlx::query_as(
        "SELECT
           EXISTS (
             SELECT 1 FROM llm_settings s
             JOIN knowledge_bases k ON k.workspace_id = s.workspace_id
             WHERE k.id = $1 AND coalesce(s.chat_model, '') <> ''
           ) AS has_chat_model,
           (SELECT count(*) FROM documents
             WHERE kb_id = $1 AND deleted_at IS NULL) AS documents,
           -- Each axis has its own in-progress state: content not yet ingested
           -- (status), or graph extraction not yet finished (graph_status)
           (SELECT count(*) FROM documents
             WHERE kb_id = $1 AND deleted_at IS NULL
               AND (status IN ('pending', 'parsing', 'indexing', 'embedding')
                    OR graph_status IN ('queued', 'extracting'))) AS processing,
           (SELECT count(*) FROM documents
             WHERE kb_id = $1 AND deleted_at IS NULL
               AND (status = 'failed' OR graph_status = 'failed')) AS failed,
           (SELECT count(*) FROM entities
             WHERE kb_id = $1 AND merged_into IS NULL) AS entities",
    )
    .bind(kb_id)
    .fetch_one(pool)
    .await?;
    Ok(r)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let current = get(pool, id).await?;
    if current.is_default {
        return Err(AppError::invalid(
            "default_kb_undeletable",
            "The default knowledge base cannot be deleted.",
        ));
    }
    let res = sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}
