//! Sink abstractions.
//!
//! # Why
//! Sinks perform side effects (writes) and must therefore be designed for
//! idempotency and clear error reporting.

pub mod id;
pub mod qdrant;

use async_trait::async_trait;

use crate::error::RagloomError;
pub use crate::sink::id::PointId;

/// Writes vector points into an external storage system.
///
/// # Why
/// The pipeline depends on sinks being idempotent where possible.
#[async_trait]
pub trait Sink {
    async fn upsert_points(&self, points: Vec<VectorPoint>) -> Result<(), RagloomError>;
}

/// A single vector point with deterministic identifier.
///
/// # Why
/// Qdrant upsert is idempotent on `id`, which is how we achieve near exactly-once
/// effects.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorPoint {
    pub id: PointId,
    pub vector: Vec<f32>,
    pub payload: serde_json::Value,
}
