use std::fs;
use std::path::Path;
use std::process::Command;

fn read_repo_file(path: &str) -> String {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(repo_root.join(path)).expect("read repository file")
}

fn crate_version() -> String {
    let cargo_toml = read_repo_file("Cargo.toml");

    cargo_toml
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|version| version.strip_suffix('"'))
        .expect("Cargo.toml package.version is present")
        .to_string()
}

fn workflow_pins_action_with_comment(workflow: &str, pinned: &str, version_comment: &str) -> bool {
    let expected = format!("uses: {pinned} # {version_comment}");

    workflow
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("uses:"))
        .any(|line| line == expected)
}

fn workflow_contains_sha_pinned_use(workflow_yaml: &str, action: &str) -> bool {
    let prefix = format!("uses: {action}@");

    workflow_yaml.lines().any(|line| {
        let Some(rest) = line.trim().strip_prefix(&prefix) else {
            return false;
        };
        let sha = rest.split_whitespace().next().unwrap_or_default();

        sha.len() == 40 && sha.chars().all(|character| character.is_ascii_hexdigit())
    })
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
    assert!(
        workflow_yaml.contains("release_ref=\"$(git rev-list -n 1 \"${release_tag}\")\"")
            && workflow_yaml.contains("echo \"release_ref=${GITHUB_SHA}\" >> \"$GITHUB_OUTPUT\"")
            && !workflow_yaml.contains("echo \"release_ref=${GITHUB_REF}\" >> \"$GITHUB_OUTPUT\""),
        "expected release workflow to reuse an existing tag commit or pin release_ref to the resolved commit SHA"
    );
}

#[test]
fn release_workflows_verify_tag_and_crate_version_consistency_and_pin_python() {
    let release_workflow = read_repo_file(".github/workflows/release.yml");
    let publish_workflow = read_repo_file(".github/workflows/publish-crate.yml");
    let quality_workflow = read_repo_file(".github/workflows/quality-deep.yml");
    let codeql_workflow = read_repo_file(".github/workflows/codeql.yml");

    assert!(
        release_workflow.contains("verify-release-version"),
        "expected release workflow to verify crate and tag versions before publishing"
    );
    assert!(
        publish_workflow.contains("verify-release-version"),
        "expected publish workflow to verify crate and tag versions before cargo publish"
    );
    assert!(
        workflow_pins_action_with_comment(
            &release_workflow,
            "actions/setup-python@a309ff8b426b58ec0e2a45f0f869d46889d02405",
            "v6.2.0",
        ),
        "expected release workflow to pin Python for the verification script to a full SHA and preserve the source version comment"
    );
    assert!(
        release_workflow.contains("python-version: \"3.11\""),
        "expected release workflow to require Python 3.11 for tomllib"
    );
    assert!(
        workflow_pins_action_with_comment(
            &publish_workflow,
            "actions/setup-python@a309ff8b426b58ec0e2a45f0f869d46889d02405",
            "v6.2.0",
        ),
        "expected publish workflow to pin Python for the verification script to a full SHA and preserve the source version comment"
    );
    assert!(
        publish_workflow.contains("python-version: \"3.11\""),
        "expected publish workflow to require Python 3.11 for tomllib"
    );
    assert!(
        publish_workflow.contains("Check whether crate version is already published"),
        "expected publish workflow to check crates.io before attempting cargo publish"
    );
    assert!(
        publish_workflow.contains("should_publish"),
        "expected publish workflow to gate cargo publish behind a crates.io version check"
    );
    assert!(
        publish_workflow.contains("cargo-token")
            && publish_workflow.contains("Skip publish without crates.io token"),
        "expected publish workflow to skip cargo publish cleanly when the crates.io token is unavailable"
    );
    assert!(
        !release_workflow.contains("security-events: write"),
        "expected release workflow to avoid unused security-events write permissions"
    );
    assert!(
        release_workflow.contains("already exists at ${RELEASE_REF}; reusing it"),
        "expected release workflow to be rerunnable after creating the release tag"
    );
    assert!(
        !quality_workflow.contains("cargo +nightly miri"),
        "expected deep quality workflow to keep Miri out of the release-critical CI path"
    );
    assert!(
        workflow_contains_sha_pinned_use(&codeql_workflow, "github/codeql-action/init"),
        "expected repository code scanning to run through a SHA-pinned CodeQL init action"
    );
    assert!(
        workflow_contains_sha_pinned_use(&codeql_workflow, "github/codeql-action/analyze"),
        "expected repository code scanning to run through a SHA-pinned CodeQL analyze action"
    );
    assert!(
        codeql_workflow.contains("languages: rust"),
        "expected CodeQL workflow to analyze Rust code"
    );
    assert!(
        codeql_workflow.contains("languages: actions"),
        "expected CodeQL workflow to analyze GitHub Actions workflows"
    );
}

#[test]
fn quality_deep_workflow_exposes_pre_merge_stability_gate() {
    let workflow_yaml = read_repo_file(".github/workflows/quality-deep.yml");
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&workflow_yaml).expect("quality workflow is valid YAML");

    let on = workflow
        .get("on")
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected quality workflow to define an `on` mapping");
    let jobs = workflow
        .get("jobs")
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected quality workflow to define jobs");

    assert!(
        on.contains_key(serde_yaml::Value::String("pull_request".to_string())),
        "expected deep quality workflow to run on pull requests before merge"
    );
    assert!(
        on.contains_key(serde_yaml::Value::String("workflow_call".to_string())),
        "expected deep quality workflow to remain reusable from release paths"
    );
    assert!(
        workflow_yaml.contains("if: github.event_name != 'pull_request'"),
        "expected coverage to stay outside the PR-visible stability gate"
    );

    let fastembed = jobs
        .get(serde_yaml::Value::String("fastembed".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected quality workflow to define a fastembed job");
    let loom = jobs
        .get(serde_yaml::Value::String("loom".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected quality workflow to define a loom job");
    let docs_and_security = jobs
        .get(serde_yaml::Value::String("docs-and-security".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected quality workflow to define a docs-and-security job");

    let fastembed_yaml = serde_yaml::to_string(fastembed).expect("serialize fastembed job");
    let loom_yaml = serde_yaml::to_string(loom).expect("serialize loom job");
    let docs_and_security_yaml =
        serde_yaml::to_string(docs_and_security).expect("serialize docs-and-security job");
    let fastembed_steps = fastembed
        .get(serde_yaml::Value::String("steps".to_string()))
        .and_then(serde_yaml::Value::as_sequence)
        .expect("expected fastembed job to define steps");
    let fastembed_checkout = fastembed_steps
        .iter()
        .find(|step| step.get("name").and_then(serde_yaml::Value::as_str) == Some("Checkout"))
        .expect("expected fastembed job to define a Checkout step");
    let fastembed_checkout_yaml =
        serde_yaml::to_string(fastembed_checkout).expect("serialize fastembed checkout step");

    assert!(
        fastembed_yaml.contains("cargo build --features fastembed")
            && fastembed_yaml.contains("cargo test --workspace --all-targets --features fastembed"),
        "expected PR-visible stability checks to cover fastembed build and tests"
    );
    assert!(
        loom_yaml.contains("cargo test --workspace --features loom"),
        "expected PR-visible stability checks to cover loom tests"
    );
    assert!(
        docs_and_security_yaml.contains("cargo deny check")
            && docs_and_security_yaml.contains("cargo audit")
            && docs_and_security_yaml.contains("cargo doc --workspace --no-deps"),
        "expected PR-visible stability checks to cover docs, cargo-deny, and cargo-audit"
    );
    assert!(
        fastembed_checkout_yaml.contains("persist-credentials: false"),
        "expected fastembed checkout to preserve disabled credential persistence"
    );
    assert!(
        fastembed_checkout_yaml.contains("ref: ${{ github.ref }}"),
        "expected fastembed checkout to pin pull_request runs to the merge ref explicitly"
    );
    assert!(
        !workflow_yaml.contains("pull_request_target"),
        "expected deep quality workflow to avoid pull_request_target for PR code execution"
    );
}

#[test]
fn release_version_verifier_accepts_v_prefixed_manual_version_input() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_file = tempfile::NamedTempFile::new().expect("create temporary GITHUB_OUTPUT");
    let version = crate_version();

    let output = Command::new("python")
        .arg(".github/scripts/verify-release-version.py")
        .current_dir(repo_root)
        .env("EXPECTED_VERSION", format!("v{version}"))
        .env("EXPECTED_TAG", "")
        .env("GITHUB_OUTPUT", output_file.path())
        .output()
        .expect("run release version verifier with Python");

    assert!(
        output.status.success(),
        "expected v-prefixed manual version input to verify successfully; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let github_output = fs::read_to_string(output_file.path()).expect("read GITHUB_OUTPUT");
    let output_lines: Vec<_> = github_output.lines().collect();
    assert!(
        output_lines.contains(&format!("version={version}").as_str())
            && output_lines.contains(&format!("tag=v{version}").as_str()),
        "expected verifier to normalize the version output and derive the release tag"
    );
}

#[test]
fn release_workflow_packages_named_assets_and_keeps_macos_best_effort() {
    let workflow_yaml = read_repo_file(".github/workflows/release.yml");
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&workflow_yaml).expect("release workflow is valid YAML");

    let jobs = workflow
        .get("jobs")
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected release workflow to define jobs");

    let release_binaries = jobs
        .get(serde_yaml::Value::String("release-binaries".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected release workflow to define supported release binaries");
    let best_effort = jobs
        .get(serde_yaml::Value::String(
            "release-binaries-best-effort".to_string(),
        ))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected release workflow to define best-effort macOS binaries");
    let publish_release = jobs
        .get(serde_yaml::Value::String("publish-release".to_string()))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected release workflow to define publish-release");

    let supported_job_yaml =
        serde_yaml::to_string(release_binaries).expect("serialize supported release job");
    let best_effort_yaml =
        serde_yaml::to_string(best_effort).expect("serialize best-effort release job");
    let publish_release_yaml =
        serde_yaml::to_string(publish_release).expect("serialize publish-release job");
    let publish_best_effort = jobs
        .get(serde_yaml::Value::String(
            "publish-best-effort-release-assets".to_string(),
        ))
        .and_then(serde_yaml::Value::as_mapping)
        .expect("expected release workflow to define best-effort asset publication");
    let publish_best_effort_yaml = serde_yaml::to_string(publish_best_effort)
        .expect("serialize best-effort asset publication job");

    assert!(
        workflow_yaml.contains("command -v sha256sum") && workflow_yaml.contains("shasum -a 256"),
        "expected release workflow to use a portable checksum command across Linux and macOS"
    );
    assert!(
        workflow_yaml
            .contains("ragloom-${{ needs.prepare-release.outputs.tag }}-${{ matrix.target }}")
            && workflow_yaml.contains("archive_extension: tar.gz")
            && workflow_yaml.contains("archive_extension: zip"),
        "expected release workflow to publish target-specific archives instead of raw unnamed binaries"
    );
    assert!(
        workflow_yaml.contains("Compress-Archive")
            && workflow_yaml.contains("Package Windows release archive"),
        "expected release workflow to package Windows assets as zip archives"
    );
    assert!(
        !workflow_yaml.contains("anchore/sbom-action") && !workflow_yaml.contains(".spdx.json"),
        "expected the release workflow to avoid publishing low-value SBOM sidecar assets"
    );
    assert!(
        supported_job_yaml.contains("x86_64-unknown-linux-gnu")
            && supported_job_yaml.contains("aarch64-unknown-linux-gnu")
            && supported_job_yaml.contains("x86_64-pc-windows-msvc"),
        "expected supported release job to gate Linux and Windows artifacts"
    );
    assert!(
        !supported_job_yaml.contains("apple-darwin"),
        "expected supported release job to exclude macOS targets"
    );
    assert!(
        best_effort_yaml.contains("x86_64-apple-darwin")
            && best_effort_yaml.contains("aarch64-apple-darwin")
            && best_effort_yaml.contains("continue-on-error: true"),
        "expected macOS artifacts to remain best-effort and non-blocking"
    );
    assert!(
        publish_release_yaml.contains("pattern: release-*")
            && publish_release_yaml.contains("merge-multiple: true")
            && !publish_release_yaml.contains("name: lcov"),
        "expected publish-release to download only named release artifacts instead of unrelated workflow artifacts"
    );
    assert!(
        !publish_release_yaml.contains("release-binaries-best-effort"),
        "expected publish-release to depend only on release-blocking targets"
    );
    assert!(
        publish_best_effort_yaml.contains("pattern: release-*-apple-darwin")
            && workflow_pins_action_with_comment(
                &workflow_yaml,
                "softprops/action-gh-release@718ea10b132b3b2eba29c1007bb80653f286566b",
                "v3.0.1",
            ),
        "expected successful macOS artifacts to be appended to the GitHub Release after the blocking targets publish with a full SHA pin and preserved source version comment"
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
    assert!(
        support.contains("Best-effort") || support.contains("best-effort"),
        "expected support docs to describe macOS release artifacts as best-effort"
    );
    assert!(
        support.contains("ragloom-v") && support.contains(".tar.gz") && support.contains(".zip"),
        "expected support docs to describe the published release archive naming convention"
    );
}
