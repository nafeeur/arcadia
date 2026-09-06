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
    /// A validation error with a stable code. **message is always the English original**
    /// — it's for clients that don't localize (MCP, CLI) and for logs; the UI takes the
    /// code and looks up wording in i18n.
    ///
    /// UI language now lives client-side, so the backend no longer owns a locale (see
    /// docs/decisions/0004), which is why the string kept here is permanently English.
    /// Anything a user can run into should carry a code.
    #[error("{message}")]
    Invalid {
        code: &'static str,
        message: String,
        /// Machine-supplied extra detail (e.g. a cron parser's error). Wording belongs
        /// to the UI, detail belongs here
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

/// Marks a failure that **retrying will not fix** (see issue #195).
///
/// The queue's default assumption is "waiting a bit might fix it", and that's true for
/// most failures: an endpoint blips, the database is briefly busy, a rate limit clears
/// within a minute. Running out of balance isn't one of those — three retries spaced
/// 30s, 2m, and 4m30s apart mean nobody's going to top up the account within those seven
/// minutes; retrying just repeats the same error three times, and the "failed" signal
/// ops actually needs to see gets delayed by those same seven minutes.
///
/// **The judgment call stays on the handler's side, not in the queue.** What counts as
/// unrecoverable is domain-specific — `utopia-store` can't see `utopia-llm`'s error
/// types, and shouldn't. The handler attaches this marker (`err.context(Terminal)`);
/// the queue only asks whether it's attached.
///
/// Attaching it doesn't change anything else: alerts still fire (`observe_job_failure`
/// also honors this marker — otherwise failing faster would mean nobody gets notified),
/// `last_error` is still written.
#[derive(Debug, Clone, Copy)]
pub struct Terminal;

impl std::fmt::Display for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("will not recover by retrying")
    }
}

impl std::error::Error for Terminal {}

/// Was this failure marked as not worth retrying? **Searches the whole context chain**
/// — after the handler attaches the marker, higher layers keep adding their own
/// `context(...)`, so checking only the outermost layer is the same as not checking
pub fn is_terminal(err: &anyhow::Error) -> bool {
    err.chain().any(|e| e.is::<Terminal>())
}
