use std::fs;
use std::io::Write;
use std::time::Duration;

use ragloom::source::{DirectoryScannerSource, Source};
use tempfile::tempdir;

#[test]
fn scanner_emits_event_for_new_file() {
    let tmp = tempdir().expect("create tempdir");
    let mut scanner = DirectoryScannerSource::new(tmp.path()).expect("create scanner");

    let path = tmp.path().join("a.txt");
    {
        let mut f = fs::File::create(&path).expect("create file");
        write!(f, "hello").expect("write file");
    }

    let events = scanner.poll();
    assert_eq!(events.len(), 1);
    assert!(events[0].fingerprint.canonical_path.ends_with("a.txt"));
}

#[test]
fn scanner_does_not_emit_duplicate_event_without_changes() {
    let tmp = tempdir().expect("create tempdir");
    let path = tmp.path().join("a.txt");
    {
        let mut f = fs::File::create(&path).expect("create file");
        write!(f, "hello").expect("write file");
    }

    let mut scanner = DirectoryScannerSource::new(tmp.path()).expect("create scanner");

    let first = scanner.poll();
    assert_eq!(first.len(), 1);

    // Ensure filesystem timestamp resolution won't cause false positives.
    std::thread::sleep(Duration::from_secs(1));

    let second = scanner.poll();
    assert_eq!(second.len(), 0);
}

#[test]
fn scanner_ignores_directories() {
    let tmp = tempdir().expect("create tempdir");
    fs::create_dir(tmp.path().join("nested")).expect("create nested dir");

    let mut scanner = DirectoryScannerSource::new(tmp.path()).expect("create scanner");

    let events = scanner.poll();
    assert_eq!(events.len(), 0);
}
