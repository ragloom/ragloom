//! Pipeline orchestration primitives.
//!
//! # Why
//! The pipeline is built from small, testable units. A dedicated planning step
//! expands high-level discovery events (file versions) into concrete work items
//! (chunks) without coupling sources to downstream processing.

pub mod planner;
pub mod runtime;
