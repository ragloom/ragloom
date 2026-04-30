use std::fs;
use std::io::Write;
use std::path::Path;
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

#[test]
fn scanner_recursively_discovers_nested_files_once() {
    let tmp = tempdir().expect("create tempdir");
    let nested = tmp.path().join("nested").join("deeper");
    fs::create_dir_all(&nested).expect("create nested dirs");

    let root_file = tmp.path().join("root.txt");
    let nested_file = nested.join("child.txt");
    write_text_file(&root_file, "root");
    write_text_file(&nested_file, "child");

    let mut scanner = DirectoryScannerSource::new(tmp.path()).expect("create scanner");

    let first = scanner.poll();
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .any(|event| event.fingerprint.canonical_path.ends_with("root.txt"))
    );
    assert!(
        first
            .iter()
            .any(|event| event.fingerprint.canonical_path.ends_with("child.txt"))
    );

    let second = scanner.poll();
    assert_eq!(second.len(), 0);
}

#[test]
fn scanner_skips_directory_symlinks() {
    let tmp = tempdir().expect("create tempdir");
    let real_dir = tmp.path().join("real");
    let link_dir = tmp.path().join("linked");
    fs::create_dir(&real_dir).expect("create real dir");
    write_text_file(&real_dir.join("inside.txt"), "nested");

    if let Err(error) = create_dir_symlink(&real_dir, &link_dir) {
        eprintln!("skipping symlink test: {error}");
        return;
    }

    let mut scanner = DirectoryScannerSource::new(tmp.path()).expect("create scanner");

    let events = scanner.poll();
    assert_eq!(events.len(), 1);
    assert!(events[0].fingerprint.canonical_path.ends_with("inside.txt"));
    assert!(
        !events[0]
            .fingerprint
            .canonical_path
            .contains(link_dir.to_string_lossy().as_ref())
    );
}

fn write_text_file(path: &Path, contents: &str) {
    let mut file = fs::File::create(path).expect("create file");
    write!(file, "{contents}").expect("write file");
}

#[cfg(unix)]
fn create_dir_symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

#[cfg(windows)]
fn create_dir_symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(original, link)
}
