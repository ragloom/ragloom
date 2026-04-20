//! Error types used across Ragloom.
//!
//! # Why a custom error type?
//! A single crate-level error improves ergonomics for callers while still
//! allowing the internals to differentiate failure categories via
//! [`RagloomErrorKind`]. Context is stored alongside the underlying cause to
//! preserve the "why" of a failure at the point it happened.

use std::error::Error as StdError;

use thiserror::Error;

/// The top-level error type for this crate.
///
/// # Why
/// Callers typically want a single error type to bubble up through layers.
/// [`RagloomError`] provides that stable surface while still exposing a
/// machine-readable [`RagloomErrorKind`] and an optional human-readable
/// context string.
#[derive(Debug, Error)]
#[error("{kind}{context}")]
pub struct RagloomError {
    /// A coarse, machine-readable category for the failure.
    pub kind: RagloomErrorKind,
    /// Additional human-readable context captured at the failure site.
    context: RagloomErrorContext,
    /// The underlying cause, if any.
    #[source]
    source: Option<Box<dyn StdError + Send + Sync + 'static>>,
}

impl RagloomError {
    /// Creates a new error with a kind and a source error.
    ///
    /// # Why
    /// Most failures originate from lower-level libraries. This constructor
    /// keeps the original cause while tagging it with a crate-level category.
    pub fn new(
        kind: RagloomErrorKind,
        source: impl Into<Box<dyn StdError + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind,
            context: RagloomErrorContext::empty(),
            source: Some(source.into()),
        }
    }

    /// Creates a new error with only a kind.
    ///
    /// # Why
    /// Some errors are generated without an underlying library error.
    pub fn from_kind(kind: RagloomErrorKind) -> Self {
        Self {
            kind,
            context: RagloomErrorContext::empty(),
            source: None,
        }
    }

    /// Attaches human-readable context to this error.
    ///
    /// # Why
    /// Context should describe *why* an operation failed in terms meaningful
    /// to the caller (e.g. which step, which input, which resource).
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = RagloomErrorContext::new(context.into());
        self
    }
}

/// A machine-readable categorization of [`RagloomError`].
///
/// # Why
/// Categories enable callers to branch on intent (retry, report, ignore) without
/// having to parse strings.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum RagloomErrorKind {
    /// An input was invalid or inconsistent.
    InvalidInput,
    /// An I/O operation failed.
    Io,
    /// A configuration value was missing or malformed.
    Config,
    /// An internal invariant was violated.
    Internal,
    /// Embedding provider operations failed.
    Embed,
    /// Sink operations failed.
    Sink,
    /// Persistent state storage failed.
    State,
}

impl std::fmt::Display for RagloomErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::InvalidInput => "invalid input",
            Self::Io => "i/o",
            Self::Config => "config",
            Self::Internal => "internal",
            Self::Embed => "embed",
            Self::Sink => "sink",
            Self::State => "state",
        };
        f.write_str(label)
    }
}

/// A lightweight wrapper used to control how context is rendered.
///
/// # Why
/// Error display should remain readable and stable. Centralizing the formatting
/// avoids ad-hoc string concatenation across the codebase.
#[derive(Debug, Clone, Default)]
pub(crate) struct RagloomErrorContext(Option<String>);

impl RagloomErrorContext {
    /// Creates an empty context.
    ///
    /// # Why
    /// Most errors start without context and only gain it where it adds value.
    pub fn empty() -> Self {
        Self(None)
    }

    /// Creates a non-empty context.
    ///
    /// # Why
    /// Keeps a single place to normalize context strings.
    pub fn new(context: String) -> Self {
        let normalized = context.trim().to_owned();
        if normalized.is_empty() {
            return Self::empty();
        }
        Self(Some(normalized))
    }
}

impl std::fmt::Display for RagloomErrorContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.as_deref() {
            Some(context) => write!(f, ": {context}"),
            None => Ok(()),
        }
    }
}

impl From<crate::transform::chunker::ChunkError> for RagloomError {
    fn from(err: crate::transform::chunker::ChunkError) -> Self {
        RagloomError::new(RagloomErrorKind::Internal, err).with_context("chunker failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_includes_context() {
        let error = RagloomError::from_kind(RagloomErrorKind::InvalidInput)
            .with_context("while parsing user profile");

        let formatted = error.to_string();
        assert!(formatted.contains("while parsing user profile"));
    }

    #[test]
    fn error_display_has_no_trailing_colon_without_context() {
        let invalid_input = RagloomError::from_kind(RagloomErrorKind::InvalidInput);
        assert_eq!(invalid_input.to_string(), "invalid input");

        let io = RagloomError::from_kind(RagloomErrorKind::Io);
        assert_eq!(io.to_string(), "i/o");

        let config = RagloomError::from_kind(RagloomErrorKind::Config);
        assert_eq!(config.to_string(), "config");

        let internal = RagloomError::from_kind(RagloomErrorKind::Internal);
        assert_eq!(internal.to_string(), "internal");

        let embed = RagloomError::from_kind(RagloomErrorKind::Embed);
        assert_eq!(embed.to_string(), "embed");

        let sink = RagloomError::from_kind(RagloomErrorKind::Sink);
        assert_eq!(sink.to_string(), "sink");

        let state = RagloomError::from_kind(RagloomErrorKind::State);
        assert_eq!(state.to_string(), "state");
    }

    #[test]
    fn whitespace_context_is_normalized_to_empty() {
        let error = RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context("   \n\t");

        let formatted = error.to_string();
        assert_eq!(formatted, "invalid input");
    }

    #[test]
    fn source_can_be_retrieved_via_error_source_chain() {
        let root_cause = std::io::Error::other("disk full");
        let error = RagloomError::new(RagloomErrorKind::Io, root_cause);

        let source = StdError::source(&error)
            .expect("RagloomError::source() should return the underlying cause");
        assert_eq!(source.to_string(), "disk full");
    }
}
