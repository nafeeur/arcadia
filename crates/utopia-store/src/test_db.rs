//! Entry point for integration tests that hit the database (#248).
//!
//! Every database-backed test starts with the same line: skip instead of fail when
//! `UTOPIA_DATABASE_URL` isn't set, so a casual local `cargo test` doesn't need the
//! database running first. But skip that way on CI too and green becomes fake: the
//! backend job has no database, all 24 store integration tests silently return early,
//! and the migrations job that does have a database only runs one.
//!
//! So skipping has to be occasion-dependent: wherever `UTOPIA_TEST_REQUIRE_DB` is set
//! (CI's database job), no database means failure — "should have run but didn't" has
//! to be visible.

/// The database URL for database-backed tests. `None` = skip this run.
///
/// Panics if `UTOPIA_TEST_REQUIRE_DB` is set but there's no URL: that's for CI's
/// benefit — there, skipping means the test never ran at all, and that must not show
/// as green.
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
                "跳过：未设 UTOPIA_DATABASE_URL（设 UTOPIA_TEST_REQUIRE_DB=1 让跳过变成失败）"
            );
            None
        }
    }
}
