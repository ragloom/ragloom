//! Vector point identifiers.
//!
//! # Why
//! Sink idempotency relies on stable, non-empty point IDs. Making the type
//! explicit prevents accidental misuse (e.g., empty IDs) and keeps the sink
//! contract self-documenting.

use crate::error::{RagloomError, RagloomErrorKind};

/// A validated vector point identifier.
///
/// # Why
/// Qdrant upsert is keyed by point id. An empty or whitespace-only id would
/// violate idempotency assumptions and lead to hard-to-debug behavior.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PointId(String);

impl PointId {
    /// Parses and validates a point id.
    ///
    /// # Why
    /// Validation is a boundary concern; we do it once when constructing work
    /// items, not repeatedly in hot loops.
    pub fn parse(raw: impl Into<String>) -> Result<Self, RagloomError> {
        let raw = raw.into();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(RagloomError::from_kind(RagloomErrorKind::InvalidInput)
                .with_context("point id is empty"));
        }

        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_empty() {
        let err = PointId::parse("   ").expect_err("should reject empty id");
        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
    }

    #[test]
    fn parse_trims_whitespace() {
        let id = PointId::parse("  abc  ").expect("ok");
        assert_eq!(id.as_str(), "abc");
    }
}
