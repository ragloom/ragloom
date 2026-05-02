use std::fs;
use std::path::Path;

fn read_repo_file(path: &str) -> String {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(repo_root.join(path)).expect("read repository file")
}

#[test]
fn release_workflow_supports_version_dispatch_and_release_notes() {
    let workflow_yaml = read_repo_file(".github/workflows/release.yml");
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&workflow_yaml).expect("release workflow is valid YAML");

    let on = workflow
        .get("on")
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected release workflow to define an `on` mapping");

    let workflow_dispatch = on
        .get(serde_yaml::Value::String("workflow_dispatch".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected release workflow to support manual `workflow_dispatch`");

    let inputs = workflow_dispatch
        .get(serde_yaml::Value::String("inputs".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected `workflow_dispatch` to define `inputs`");

    let version_input = inputs
        .get(serde_yaml::Value::String("version".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected `workflow_dispatch.inputs` to define a `version` input");

    assert!(
        version_input
            .get(serde_yaml::Value::String("required".to_string()))
            .and_then(serde_yaml::Value::as_bool)
            .unwrap_or(false),
        "expected `workflow_dispatch.inputs.version.required` to be true"
    );
    assert!(
        matches!(
            version_input.get(serde_yaml::Value::String("type".to_string())),
            Some(serde_yaml::Value::String(kind)) if kind == "string"
        ),
        "expected `workflow_dispatch.inputs.version.type` to be `string`"
    );
    assert!(
        workflow_yaml.contains("generate_release_notes: true"),
        "expected release workflow to generate release notes deterministically"
    );
}

#[test]
fn release_workflows_verify_tag_and_crate_version_consistency_and_pin_python() {
    let release_workflow = read_repo_file(".github/workflows/release.yml");
    let publish_workflow = read_repo_file(".github/workflows/publish-crate.yml");
    let quality_workflow = read_repo_file(".github/workflows/quality-deep.yml");

    assert!(
        release_workflow.contains("verify-release-version"),
        "expected release workflow to verify crate and tag versions before publishing"
    );
    assert!(
        publish_workflow.contains("verify-release-version"),
        "expected publish workflow to verify crate and tag versions before cargo publish"
    );
    assert!(
        release_workflow.contains("actions/setup-python@v5"),
        "expected release workflow to pin Python for the verification script"
    );
    assert!(
        release_workflow.contains("python-version: \"3.11\""),
        "expected release workflow to require Python 3.11 for tomllib"
    );
    assert!(
        publish_workflow.contains("actions/setup-python@v5"),
        "expected publish workflow to pin Python for the verification script"
    );
    assert!(
        publish_workflow.contains("python-version: \"3.11\""),
        "expected publish workflow to require Python 3.11 for tomllib"
    );
    assert!(
        quality_workflow.contains("security-events: write"),
        "expected deep quality workflow to request security-events permission"
    );
    assert!(
        release_workflow.contains("security-events: write"),
        "expected release workflow to grant security-events permission to reusable jobs"
    );
    assert!(
        quality_workflow.contains("timeout-minutes: 90"),
        "expected deep quality Miri job to stay bounded in CI"
    );
    assert!(
        quality_workflow.contains("cargo +nightly miri test -p ragloom --lib"),
        "expected deep quality Miri job to use the bounded crate-level command"
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
