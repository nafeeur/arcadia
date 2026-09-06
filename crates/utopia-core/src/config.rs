use figment::{
    providers::{Env, Serialized},
    Figment,
};
use serde::{Deserialize, Serialize};

/// Global configuration. Source priority: environment variables (prefixed `UTOPIA_`) > defaults.
/// The `.env` file is preloaded into the environment by the binary entrypoint via dotenvy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    /// Connection string used to run migrations. Migrations need to create tables and
    /// triggers; the runtime doesn't need those privileges — once separated, the app can
    /// connect through a restricted role that can read/write business tables and only
    /// append to the ledger. Falls back to `database_url` when unset, so existing
    /// deployments can upgrade without any change.
    pub migration_url: Option<String>,
    pub bind_addr: String,
    /// JWT signing key. Left blank, one is generated on first start and stored in
    /// deployment_settings — requiring the operator to hand-type a random string just
    /// results, in practice, in the default value going straight to production.
    /// An explicit value here takes priority over the one in the database: this is the
    /// path for key rotation and for explicitly aligning multiple instances.
    pub jwt_secret: Option<String>,
    /// Credential sealing key (32 bytes, hex or base64). Left blank, `secret.key` under
    /// the data directory is used, generated on first start. **The key never enters the
    /// database** — a database leak not implying a credential leak is the entire point of
    /// encryption at rest. Back it up together with the data directory; without it,
    /// credentials stored in the database cannot be read back.
    pub secret_key: Option<String>,
    /// Frontend build output directory; when present, the server hosts the SPA (with
    /// history fallback).
    pub web_dist: String,
    /// Data directory: raw files (files/) and the Tantivy index (index/).
    pub data_dir: String,
    /// Database connection pool limit. Defaults to 32, matching the default worker
    /// concurrency — when the pool is smaller than the concurrency, the symptom is
    /// requests slowing down, not any error saying "pool exhausted", so this needs to be
    /// tunable.
    pub db_max_connections: Option<u32>,
    /// Force `Secure` on the session cookie. Defaults to false: decided from the
    /// request's X-Forwarded-Proto, set only over TLS. Only needs to be forced on here
    /// when the proxy doesn't send that header.
    pub cookie_secure: bool,
    /// Whether registration is open. When false, only the first user (bootstrapping the
    /// deployment) can register; anyone else needs an administrator to open it up.
    pub open_registration: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            database_url: "postgres://utopia:utopia@localhost:1517/utopia".into(),
            migration_url: None,
            bind_addr: "0.0.0.0:1516".into(),
            jwt_secret: None,
            secret_key: None,
            web_dist: "web/dist".into(),
            data_dir: "data".into(),
            db_max_connections: None,
            cookie_secure: false,
            open_registration: true,
        }
    }
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let cfg = Figment::from(Serialized::defaults(AppConfig::default()))
            .merge(blank_is_unset(Env::prefixed("UTOPIA_")))
            .extract()?;
        Ok(cfg)
    }
}

/// An environment variable whose value is empty is treated as **unset** (#343).
///
/// In container orchestration, `UTOPIA_X: ${UTOPIA_X:-}` passes an empty string into the
/// container when the variable isn't set — not "don't pass it at all". Figment accepts
/// that as-is, so `Option<String>` ends up with `Some("")`, a `String` field gets its
/// default overwritten by the empty string, and `bool`/numeric fields fail to deserialize
/// at all.
///
/// The check lives at the environment-reading layer rather than at each call site: an
/// empty string isn't a valid value for any of these fields, and guarding field-by-field
/// only ever misses the next field someone adds — `jwt_secret` and `secret_key` each had
/// their own guard, and `migration_url` was the one that got missed, with the symptom
/// being the empty string reaching sqlx as "relative URL without a base", killing the
/// deployment at the very first step of the README quick start. `init-app-role.sh` already
/// handles the same thing with `[ -z ]`; this was the missing piece on the Rust side.
fn blank_is_unset(env: Env) -> Env {
    // iter() yields keys with the prefix stripped and lowercased; filter receives keys
    // with the prefix stripped but NOT lowercased, so the comparison here is
    // case-insensitive.
    let blank: Vec<String> = env
        .iter()
        .filter(|(_, value)| value.trim().is_empty())
        .map(|(key, _)| key.to_string())
        .collect();
    env.filter(move |key| {
        !blank
            .iter()
            .any(|blank_key| key.as_str().eq_ignore_ascii_case(blank_key))
    })
}

impl AppConfig {
    /// Migration connection string: falls back to the runtime one when not configured separately.
    pub fn migration_url(&self) -> &str {
        self.migration_url.as_deref().unwrap_or(&self.database_url)
    }
}

#[cfg(test)]
// Jail's closure must return figment::Result; we don't control the size of that Err variant
#[allow(clippy::result_large_err)]
mod tests {
    use super::AppConfig;
    use figment::Jail;

    /// #343: in `docker-compose.yml`, `UTOPIA_MIGRATION_URL: ${UTOPIA_MIGRATION_URL:-}`
    /// passes an **empty string** into the container when the variable isn't set, not
    /// "don't pass it". The empty string overwrote the fallback, so `migration_url()`
    /// returned "", sqlx reported "relative URL without a base", and the deployment died
    /// at the very first step of the README quick start.
    #[test]
    fn a_blank_migration_url_falls_back_to_the_database_url() {
        Jail::expect_with(|jail| {
            jail.set_env("UTOPIA_DATABASE_URL", "postgres://u:p@db:5432/utopia");
            jail.set_env("UTOPIA_MIGRATION_URL", "");
            let cfg = AppConfig::load().unwrap();
            assert_eq!(cfg.migration_url, None, "an empty string must be treated as unset");
            assert_eq!(cfg.migration_url(), cfg.database_url);
            Ok(())
        });
    }

    /// Whitespace-only also counts as blank: `UTOPIA_MIGRATION_URL=" "` is not a connection string either.
    #[test]
    fn whitespace_counts_as_blank() {
        Jail::expect_with(|jail| {
            jail.set_env("UTOPIA_DATABASE_URL", "postgres://u:p@db:5432/utopia");
            jail.set_env("UTOPIA_MIGRATION_URL", "   ");
            let cfg = AppConfig::load().unwrap();
            assert_eq!(cfg.migration_url, None);
            Ok(())
        });
    }

    /// A genuinely provided value still wins as usual — this guards against the filter above overreaching.
    #[test]
    fn a_migration_url_that_is_set_still_wins() {
        Jail::expect_with(|jail| {
            jail.set_env("UTOPIA_DATABASE_URL", "postgres://app:p@db:5432/utopia");
            jail.set_env("UTOPIA_MIGRATION_URL", "postgres://owner:p@db:5432/utopia");
            let cfg = AppConfig::load().unwrap();
            assert_eq!(
                cfg.migration_url.as_deref(),
                Some("postgres://owner:p@db:5432/utopia")
            );
            assert_ne!(cfg.migration_url(), cfg.database_url);
            Ok(())
        });
    }

    /// The empty-string trap isn't limited to `Option<String>`: a `String` field has no
    /// fallback to speak of, so the empty string overwrites the default directly.
    #[test]
    fn a_blank_string_field_keeps_its_default() {
        Jail::expect_with(|jail| {
            jail.set_env("UTOPIA_DATA_DIR", "");
            jail.set_env("UTOPIA_WEB_DIST", "");
            jail.set_env("UTOPIA_BIND_ADDR", "");
            let cfg = AppConfig::load().unwrap();
            assert_eq!(cfg.data_dir, "data");
            assert_eq!(cfg.web_dist, "web/dist");
            assert_eq!(cfg.bind_addr, "0.0.0.0:1516");
            Ok(())
        });
    }

    /// Non-string fields fail even earlier: an empty string can't even deserialize, so
    /// `load()` errors out directly — and the error is figment's type error, which doesn't
    /// show which environment variable was passed empty.
    #[test]
    fn a_blank_typed_field_does_not_break_loading() {
        Jail::expect_with(|jail| {
            jail.set_env("UTOPIA_DB_MAX_CONNECTIONS", "");
            jail.set_env("UTOPIA_COOKIE_SECURE", "");
            let cfg = AppConfig::load().unwrap();
            assert_eq!(cfg.db_max_connections, None);
            assert!(!cfg.cookie_secure);
            Ok(())
        });
    }

    /// The converse of the above: a value that is set still parses.
    #[test]
    fn a_typed_field_that_is_set_still_parses() {
        Jail::expect_with(|jail| {
            jail.set_env("UTOPIA_DB_MAX_CONNECTIONS", "8");
            jail.set_env("UTOPIA_COOKIE_SECURE", "true");
            let cfg = AppConfig::load().unwrap();
            assert_eq!(cfg.db_max_connections, Some(8));
            assert!(cfg.cookie_secure);
            Ok(())
        });
    }
}
