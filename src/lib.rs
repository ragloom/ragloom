//! Ragloom is a minimalist RAG ingestion engine.
//!
//! # Why a library crate?
//! The binary (`main.rs`) should stay thin and focus on orchestration.
//! The library crate provides reusable building blocks (types, errors, and
//! domain logic) that can be tested and consumed by other binaries.

pub mod config;
pub mod doc;
pub mod embed;
pub mod error;
pub mod ids;
pub mod pipeline;
pub mod sink;
pub mod source;
pub mod state;
pub mod transform;

pub mod observability;

pub use crate::error::{RagloomError, RagloomErrorKind};
