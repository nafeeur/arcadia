#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Not found")]
    NotFound,
    #[error("Not signed in or invalid credentials")]
    Unauthorized,
    #[error("You don't have permission to do that")]
    Forbidden,
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Validation(String),
    /// A validation error with a stable code. **The message is still the original English
    /// sentence** — it's for clients that don't localize (MCP, CLI) and for logs; the
    /// interface takes the code and looks up its own wording in i18n.
    ///
    /// Interface language now lives on the client, not the backend — the backend no
    /// longer owns a locale (see docs/decisions/0004) — so the string kept here is
    /// permanently English. Anything a user can actually hit should carry a code.
    #[error("{message}")]
    Invalid {
        code: &'static str,
        message: String,
        /// Machine-supplied detail (e.g. a cron parser's error). Wording belongs to the
        /// interface; the specifics belong here.
        detail: Option<String>,
    },
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        AppError::Invalid {
            code,
            message: message.into(),
            detail: None,
        }
    }
    pub fn invalid_detail(
        code: &'static str,
        message: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        AppError::Invalid {
            code,
            message: message.into(),
            detail: Some(detail.into()),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Marks a failure that **will not get better by retrying** (see issue #195).
///
/// The queue's default assumption is "waiting a bit will probably fix it", and that's true
/// of most failures: an endpoint blips, the database is briefly busy, a rate limit clears
/// within a minute. Running out of balance isn't one of those — three retries spaced 30s,
/// 2 minutes, 4.5 minutes apart give nobody seven minutes to go top up the account; retrying
/// just repeats the same error three times, and the "failed" that operators actually need
/// to see gets delayed by those seven minutes.
///
/// **The judgment call stays with the handler, not the queue.** What counts as
/// unrecoverable is domain-specific — `utopia-store` can't see `utopia-llm`'s error types,
/// and shouldn't have to. The handler attaches this marker (`err.context(Terminal)`); the
/// queue only asks "is it attached".
///
/// Attaching it doesn't change anything else: alerting still fires as usual
/// (`observe_job_failure` recognizes this marker too — otherwise failing faster would mean
/// nobody gets told), and `last_error` is still written.
#[derive(Debug, Clone, Copy)]
pub struct Terminal;

impl std::fmt::Display for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("will not recover by retrying")
    }
}

impl std::error::Error for Terminal {}

/// Was this failure marked as not worth retrying? **Search the whole context chain** —
/// after the handler attaches the marker, higher layers keep adding their own
/// `context(...)`, so checking only the outermost layer is the same as not checking at all.
pub fn is_terminal(err: &anyhow::Error) -> bool {
    err.chain().any(|e| e.is::<Terminal>())
}
