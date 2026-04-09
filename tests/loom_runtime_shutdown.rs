//! Loom model test for runtime shutdown ordering.
//!
//! # Why
//! Concurrency bugs often hide in shutdown paths (drops, channel closes, joins).
//! Loom explores interleavings deterministically to catch these early.

#![cfg(feature = "loom")]

use loom::sync::Arc;
use loom::sync::atomic::{AtomicBool, Ordering};
use loom::thread;

#[test]
fn shutdown_flag_is_observed_by_worker() {
    loom::model(|| {
        let shutdown = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(AtomicBool::new(false));

        let shutdown_worker = Arc::clone(&shutdown);
        let seen_worker = Arc::clone(&seen);

        let worker = thread::spawn(move || {
            while !shutdown_worker.load(Ordering::SeqCst) {
                thread::yield_now();
            }
            seen_worker.store(true, Ordering::SeqCst);
        });

        shutdown.store(true, Ordering::SeqCst);

        worker.join().expect("worker should join");
        assert!(seen.load(Ordering::SeqCst));
    });
}
