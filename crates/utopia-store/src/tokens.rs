//! Personal access tokens (see `docs/decisions/0014`).
//!
//! **A token acts as the person who issued it, but need not carry that person's full authority**:
//!
//! ```text
//! effective permission = this person's role ∩ this token's scope
//! ```
//!
//! Intersection, not union — a viewer's token with write checked still reads only. So
//! this module only answers "who does this string correspond to, how far does this
//! token let them go" — **whether they can touch a given knowledge base is still
//! decided by `access::require_kb`**, unchanged.

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use utopia_core::models::TokenView;
use utopia_core::{AppError, AppResult};
use uuid::Uuid;

/// Prefix for the plaintext token. Kept distinct from `sources.ingest_token`'s `utp_` —
/// the two do very different things, and logs or config files should tell at a glance
/// which kind this is.
const PREFIX: &str = "utp_pat_";
/// The short slice shown to humans in listings (prefix included). Enough to match
/// against what's in a config file, not enough to reconstruct the rest.
const SHOWN: usize = 16;

/// What you get once verification passes: who, and how far this token lets them go.
pub struct Authenticated {
    pub user_id: Uuid,
    pub token_id: Uuid,
    /// read | write
    pub scope: String,
    /// None = every knowledge base this person can access
    pub kb_ids: Option<Vec<Uuid>>,
}

impl Authenticated {
    /// Whether this token allows touching this knowledge base.
    ///
    /// **This is not a permission check, it's a scope check.** Returning true only
    /// means "the token didn't exclude it" — what role that person actually holds in
    /// this knowledge base is still `access::require_kb`'s question to answer.
    pub fn covers(&self, kb_id: Uuid) -> bool {
        match &self.kb_ids {
            None => true,
            Some(ids) => ids.contains(&kb_id),
        }
    }

    pub fn can_write(&self) -> bool {
        self.scope == "write"
    }
}

/// **SHA-256, not argon2.** This differs from how passwords are stored, for two reasons:
///
/// 1. **A token is a high-entropy random string, not a password a human picked.**
///    argon2's slowness exists to make brute-forcing "password123" uneconomical; against
///    244 bits of randomness, a million times slower still cracks nothing — there's
///    nothing to buy with that slowdown.
/// 2. **argon2 salts each row differently, so you can't look one up.** Verification is a
///    hot path (once per tool call), and `WHERE token_hash = $1` hitting a unique index
///    is a single lookup; with argon2 you'd have to pull back every token and verify
///    them one by one — the more you've issued, the slower it gets.
fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Issue one. **The returned plaintext appears exactly once** — only the hash is
/// stored, so losing it means reissuing.
pub async fn issue(
    pool: &PgPool,
    user_id: Uuid,
    name: &str,
    scope: &str,
    kb_ids: Option<&[Uuid]>,
    expires_at: Option<DateTime<Utc>>,
) -> AppResult<(TokenView, String)> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return AppResult::Err(AppError::invalid(
            "bad_token_name",
            "Token name must be 1-64 characters",
        ));
    }
    if !matches!(scope, "read" | "write") {
        return Err(AppError::invalid(
            "bad_token_scope",
            "Scope must be read or write",
        ));
    }
    // Two v4s concatenated ≈ 244 bits of entropy. Same approach as `new_ingest_token`;
    // a different prefix just so logs make it easy to tell which kind is which.
    let plain = format!(
        "{PREFIX}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let id = Uuid::now_v7();
    let view: TokenView = sqlx::query_as(
        "INSERT INTO personal_tokens
             (id, user_id, name, token_hash, token_prefix, scope, kb_ids, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, name, token_prefix, scope, kb_ids, expires_at,
                   last_used_at, revoked_at, created_at",
    )
    .bind(id)
    .bind(user_id)
    .bind(name)
    .bind(hash(&plain))
    .bind(&plain[..SHOWN])
    .bind(scope)
    .bind(kb_ids)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok((view, plain))
}

/// Which tokens I've issued. Revoked ones are listed too — **the fact of revocation
/// itself must stay visible**.
pub async fn list(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<TokenView>> {
    Ok(sqlx::query_as(
        "SELECT id, name, token_prefix, scope, kb_ids, expires_at,
                last_used_at, revoked_at, created_at
           FROM personal_tokens WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

/// Revoke one. **Stamp the row, don't delete it**: delete it and "this key ever
/// existed" becomes unqueryable — exactly the first question a post-incident review
/// asks.
pub async fn revoke(pool: &PgPool, user_id: Uuid, token_id: Uuid) -> AppResult<()> {
    let res = sqlx::query(
        "UPDATE personal_tokens SET revoked_at = now()
          WHERE id = $2 AND user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .bind(token_id)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Plaintext -> who this is and how far they may go.
///
/// **Expiry and revocation are checked in SQL, not in Rust.** Fetch first and compare
/// after, and a token revoked in the gap between "fetched" and "compared" still passes
/// — and MCP connections are long-lived, so that gap can be long enough to matter.
///
/// Also bumps `last_used_at` along the way: before revoking, someone has to be able to
/// answer "is this one still in use" — without that number nobody dares revoke.
pub async fn authenticate(pool: &PgPool, plain: &str) -> AppResult<Authenticated> {
    if !plain.starts_with(PREFIX) {
        return Err(AppError::Unauthorized);
    }
    let row: Option<(Uuid, Uuid, String, Option<Vec<Uuid>>)> = sqlx::query_as(
        "UPDATE personal_tokens SET last_used_at = now()
          WHERE token_hash = $1
            AND revoked_at IS NULL
            AND (expires_at IS NULL OR expires_at > now())
        RETURNING id, user_id, scope, kb_ids",
    )
    .bind(hash(plain))
    .fetch_optional(pool)
    .await?;
    let Some((token_id, user_id, scope, kb_ids)) = row else {
        return Err(AppError::Unauthorized);
    };
    Ok(Authenticated {
        user_id,
        token_id,
        scope,
        kb_ids,
    })
}
