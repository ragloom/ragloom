//! Loom sanity test.
//!
//! # Why
//! We introduce Loom early to ensure the repository can run deterministic
//! concurrency model checks. Later pipeline runtime tests will depend on this.

#![cfg(feature = "loom")]

use loom::sync::Arc;
use loom::thread;

#[test]
fn loom_can_model_two_threads_incrementing_shared_state() {
    loom::model(|| {
        let value = Arc::new(loom::sync::atomic::AtomicUsize::new(0));
        let a = Arc::clone(&value);
        let b = Arc::clone(&value);

        let t1 = thread::spawn(move || {
            a.fetch_add(1, loom::sync::atomic::Ordering::SeqCst);
        });
        let t2 = thread::spawn(move || {
            b.fetch_add(1, loom::sync::atomic::Ordering::SeqCst);
        });

        t1.join().expect("thread 1 should join");
        t2.join().expect("thread 2 should join");

        let final_value = value.load(loom::sync::atomic::Ordering::SeqCst);
        assert_eq!(final_value, 2);
    });
}
