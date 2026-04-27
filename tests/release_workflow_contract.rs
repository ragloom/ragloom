use std::fs;
use std::path::Path;

fn read_repo_file(path: &str) -> String {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(repo_root.join(path)).expect("read repository file")
}

#[test]
fn release_workflow_supports_version_dispatch_and_release_notes() {
    let workflow = read_repo_file(".github/workflows/release.yml");

    assert!(
        workflow.contains("workflow_dispatch:"),
        "expected release workflow to support manual dispatch"
    );
    assert!(
        workflow.contains("version:"),
        "expected release workflow to declare a version input"
    );
    assert!(
        workflow.contains("generate_release_notes: true"),
        "expected release workflow to generate release notes deterministically"
    );
}

#[test]
fn release_workflows_verify_tag_and_crate_version_consistency() {
    let release_workflow = read_repo_file(".github/workflows/release.yml");
    let publish_workflow = read_repo_file(".github/workflows/publish-crate.yml");

    assert!(
        release_workflow.contains("verify-release-version"),
        "expected release workflow to verify crate and tag versions before publishing"
    );
    assert!(
        publish_workflow.contains("verify-release-version"),
        "expected publish workflow to verify crate and tag versions before cargo publish"
    );
}

#[test]
fn support_docs_describe_release_dispatch_runbook() {
    let support = read_repo_file("SUPPORT.md");

    assert!(
        support.contains("workflow_dispatch"),
        "expected support docs to describe the manual release workflow entrypoint"
    );
    assert!(
        support.contains("Cargo.toml"),
        "expected support docs to document crate-version verification"
    );
}
