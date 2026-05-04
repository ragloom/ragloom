//! Pipeline configuration.
//!
//! # Why
//! Ragloom is designed to be operated as a single binary configured via a file.
//! A typed config model makes validation explicit and enables safe hot reload
//! by validating changes before applying them.

use serde::Deserialize;

use crate::error::{RagloomError, RagloomErrorKind};

/// Top-level pipeline configuration.
///
/// # Why
/// We keep operational settings (endpoints, limits, paths) in one tree so
/// validation and reload can be performed atomically.
#[derive(Debug, Clone, Deserialize)]
pub struct PipelineConfig {
    pub source: SourceConfig,
    pub embed: EmbedConfig,
    pub sink: SinkConfig,
    #[serde(default)]
    pub state: StateConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceConfig {
    pub root: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbedConfig {
    pub endpoint: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SinkConfig {
    pub qdrant_url: String,
    pub collection: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StateConfig {
    #[serde(default = "default_state_path")]
    pub path: String,
}

impl Default for StateConfig {
    fn default() -> Self {
        Self {
            path: default_state_path(),
        }
    }
}

fn default_state_path() -> String {
    ".ragloom/wal.ndjson".to_string()
}

impl PipelineConfig {
    /// Parses a YAML document into a typed config.
    ///
    /// # Why
    /// Parsing is a boundary operation; failures must be reported with enough
    /// context for operators to fix the configuration quickly.
    pub fn from_yaml_str(yaml: &str) -> Result<Self, RagloomError> {
        serde_yaml::from_str(yaml).map_err(|e| {
            RagloomError::from_kind(RagloomErrorKind::Config)
                .with_context(format!("invalid yaml: {e}"))
        })
    }

    /// Validates invariants that are required for a safe runtime.
    ///
    /// # Why
    /// Reload applies configs at runtime. Validation prevents partial/unsafe
    /// configs from being activated.
    pub fn validate(&self) -> Result<(), RagloomError> {
        if self.source.root.trim().is_empty() {
            return Err(RagloomError::from_kind(RagloomErrorKind::Config)
                .with_context("source.root is empty"));
        }
        if self.embed.endpoint.trim().is_empty() {
            return Err(RagloomError::from_kind(RagloomErrorKind::Config)
                .with_context("embed.endpoint is empty"));
        }
        if self.sink.qdrant_url.trim().is_empty() {
            return Err(RagloomError::from_kind(RagloomErrorKind::Config)
                .with_context("sink.qdrant_url is empty"));
        }
        if self.sink.collection.trim().is_empty() {
            return Err(RagloomError::from_kind(RagloomErrorKind::Config)
                .with_context("sink.collection is empty"));
        }
        if self.state.path.trim().is_empty() {
            return Err(RagloomError::from_kind(RagloomErrorKind::Config)
                .with_context("state.path is empty"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pipeline_yaml_and_validates_required_fields() {
        let yaml = r#"
source:
  root: "/data"
embed:
  endpoint: "http://localhost:8080/embed"
sink:
  qdrant_url: "http://localhost:6333"
  collection: "docs"
state:
  path: ".ragloom/wal.ndjson"
"#;
        let cfg = PipelineConfig::from_yaml_str(yaml).expect("parse");
        cfg.validate().expect("validate");
        assert_eq!(cfg.state.path, ".ragloom/wal.ndjson");
    }
}
