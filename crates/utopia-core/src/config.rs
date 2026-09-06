use figment::{
    providers::{Env, Serialized},
    Figment,
};
use serde::{Deserialize, Serialize};

/// Global config. Source priority: environment variables (prefix `UTOPIA_`) > defaults.
/// The `.env` file is preloaded into the environment by the binary entrypoint via dotenvy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    /// Connection string used for running migrations. Migrations need to create tables
    /// and triggers; the runtime doesn't need those privileges — once split, the app can
    /// connect with a restricted role that only reads/writes business tables and can only
    /// append to the ledger, never modify it.
    /// Falls back to `database_url` when unset, so existing deployments upgrade unchanged.
    pub migration_url: Option<String>,
    pub bind_addr: String,
    /// JWT signing secret. Left empty, it's generated on first startup and stored in
    /// deployment_settings — requiring the deployer to hand-fill a random string in
    /// practice just means the default value ships to production as-is.
    /// An explicit value here takes priority over the one in the database: key rotation
    /// and explicit alignment across multiple instances go through this path.
    pub jwt_secret: Option<String>,
    /// Credential sealing key (32 bytes, 64-char hex or base64). Left empty, falls back to
    /// `secret.key` under the data directory, generated on first startup. **The key never
    /// goes into the database**: a leaked database not implying leaked credentials is the
    /// whole point of encryption at rest — take the key along when backing up the data
    /// directory, since without it the credentials in the database can't be read.
    pub secret_key: Option<String>,
    /// Frontend build output directory; when present the server hosts the SPA (with
    /// history fallback).
    pub web_dist: String,
    /// Data directory: raw files (files/) and the Tantivy index (index/).
    pub data_dir: String,
    /// Database connection pool cap. Defaults to 32, aligned with the default worker
    /// concurrency — when the pool is smaller than the concurrency, the symptom is
    /// slower requests, not any message saying "pool exhausted", so this has to be tunable.
    pub db_max_connections: Option<u32>,
    /// Force the Secure flag on session cookies. Defaults to false: determined by the
    /// request's X-Forwarded-Proto, set only over TLS. Only needs forcing on here when
    /// the proxy doesn't send that header.
    pub cookie_secure: bool,
    /// Whether registration is open. When false, only the first user (bootstrapping the
    /// deployment) can register; everyone else needs an admin to open it up.
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

/// A blank environment variable is treated as **unset** (#343).
///
/// In container orchestration, `UTOPIA_X: ${UTOPIA_X:-}` passes an empty string into the
/// container when the variable isn't set — not "nothing passed". Figment takes it at face
/// value, so `Option<String>` ends up with `Some("")`, a `String` field's default gets
/// overwritten by the empty string, and `bool`/numeric fields fail to deserialize at all.
///
/// The check lives at the environment-reading layer rather than at each call site: an
/// empty string isn't a valid value for any field here, and guarding field-by-field just
/// misses whichever one gets added next — `jwt_secret` and `secret_key` each guard
/// themselves, and `migration_url` was the one that got missed, with the symptom being an
/// empty string handed to sqlx producing "relative URL without a base", exiting right at
/// the first step of a deployment following the README quickstart. `init-app-role.sh`
/// already checks the same thing with `[ -z ]`; this is what was missing on the Rust side.
fn blank_is_unset(env: Env) -> Env {
    // iter() yields keys with the prefix stripped and lowercased; filter receives them
    // with the prefix stripped but not lowercased, so the comparison here is
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
    /// Migration connection string: falls back to the runtime one when not configured
    /// separately.
    pub fn migration_url(&self) -> &str {
        self.migration_url.as_deref().unwrap_or(&self.database_url)
    }
}

#[cfg(test)]
// Jail's closure must return figment::Result, and we don't control the size of that Err variant
#[allow(clippy::result_large_err)]
mod tests {
    use super::AppConfig;
    use figment::Jail;

    /// #343: in `docker-compose.yml`, `UTOPIA_MIGRATION_URL: ${UTOPIA_MIGRATION_URL:-}`
    /// passes an **empty string** into the container when the variable isn't set, not
    /// "nothing passed". Once the empty string overwrites the fallback,
    /// `migration_url()` returns "", sqlx reports "relative URL without a base", and the
    /// deployment exits right at the first step of the README quickstart.
    #[test]
    fn a_blank_migration_url_falls_back_to_the_database_url() {
        Jail::expect_with(|jail| {
            jail.set_env("UTOPIA_DATABASE_URL", "postgres://u:p@db:5432/utopia");
            jail.set_env("UTOPIA_MIGRATION_URL", "");
            let cfg = AppConfig::load().unwrap();
            assert_eq!(cfg.migration_url, None, "a blank string should be treated as unset");
            assert_eq!(cfg.migration_url(), cfg.database_url);
            Ok(())
        });
    }

    /// Whitespace-only also counts as blank: `UTOPIA_MIGRATION_URL=" "` isn't a
    /// connection string either.
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

    /// A value that's actually set still takes effect — this test guards against the
    /// filter above overreaching.
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

    /// An empty string doesn't only trip up `Option<String>`: a `String` field has no
    /// fallback to speak of, so the empty string overwrites its default outright.
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

    /// Non-string fields fail even earlier: an empty string can't even deserialize,
    /// so `load()` errors outright — and it's figment's type error that gets reported,
    /// which doesn't say which environment variable was passed blank.
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

    /// The converse of the above: given a value, it still parses.
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
