//! Oracle engine module.

/// Core oracle engine implementation.
pub mod oracle;
pub use oracle::Oracle;

/// Background scheduler for continuous refresh loop.
pub mod scheduler;
