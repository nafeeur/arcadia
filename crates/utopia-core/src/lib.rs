//! utopia-core: domain models, error types, and configuration.

pub mod config;
pub mod error;
pub mod models;
pub mod secrets;

pub use error::{is_terminal, AppError, AppResult, Terminal};
