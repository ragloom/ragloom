//! Configuration reload sources.
//!
//! # Why
//! Hot reload must be testable and composable. By abstracting the trigger
//! mechanism (SIGHUP vs file watch), the pipeline runtime remains open for
//! extension without modification.

use std::path::PathBuf;

/// A source of reload signals.
///
/// # Why
/// The runtime should not couple itself to an OS mechanism. This trait provides
/// the minimum surface needed to locate the config file and later attach a
/// trigger implementation.
pub trait ReloadSource: Send + Sync {
    fn config_path(&self) -> PathBuf;
}
