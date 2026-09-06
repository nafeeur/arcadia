//! Entry point for integration tests that connect to a database (#248).
//!
//! Every database test starts with the same line: skip rather than fail when
//! `UTOPIA_DATABASE_URL` isn't set, so a casual local `cargo test` doesn't require a database
//! to be running first. But CI can skip the same way, and then green becomes a lie: the
//! backend job has no database, so all 24 store integration tests silently return, while the
//! migrations job (which does have a database) only runs one of them.
//!
//! So skipping has to depend on context: wherever `UTOPIA_TEST_REQUIRE_DB` is set (CI's
//! database-connected job), no database is a failure — "should have run but didn't" needs to
//! be visible.

/// Database URL for database-backed tests. `None` = skip this run.
///
/// Panics if `UTOPIA_TEST_REQUIRE_DB` is set but there's no URL: this is for CI — there,
/// skipping would mean the test never ran at all, and that must not show up green.
pub fn url() -> Option<String> {
    match std::env::var("UTOPIA_DATABASE_URL") {
        Ok(u) if !u.trim().is_empty() => Some(u),
        _ => {
            if std::env::var_os("UTOPIA_TEST_REQUIRE_DB").is_some() {
                panic!(
                    "UTOPIA_TEST_REQUIRE_DB is set but UTOPIA_DATABASE_URL is not:                      this run must not skip database-backed tests"
                );
            }
            eprintln!(
                "Skipping: UTOPIA_DATABASE_URL not set (set UTOPIA_TEST_REQUIRE_DB=1 to turn a skip into a failure)"
            );
            None
        }
    }
}
