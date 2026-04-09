//! Persistent state primitives.
//!
//! # Why
//! Ragloom needs crash recovery and idempotency tracking. The core pipeline should
//! not be tied to a specific storage engine, so we define a small trait surface
//! that keeps the system open for extension.

pub mod wal;

/// Persistent state store abstraction.
///
/// # Why
/// The ingestion runtime must be able to record durable progress and query
/// outstanding work after restart. This trait defines that minimal contract
/// without locking the core into a concrete database.
pub trait StateStore: Send + Sync {}
