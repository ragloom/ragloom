use std::fs;
use std::path::Path;

fn read_repo_file(path: &str) -> String {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(repo_root.join(path)).expect("read repository file")
}

#[test]
fn readme_describes_v0_4_sources_formats_and_s3_example() {
    let readme = read_repo_file("README.md");

    assert!(
        readme.contains("v0.4"),
        "expected README to frame the current support story around v0.4"
    );
    assert!(
        readme.contains("local filesystem source")
            && readme.contains("kind: \"filesystem\"")
            && readme.contains("root: \"./docs\"")
            && readme.contains("source.kind: s3"),
        "expected README to document both filesystem and S3 source shapes with concrete examples"
    );
    assert!(
        readme.contains("PDF extraction is text-only")
            && readme.contains("DOCX extraction is also text-only"),
        "expected README to keep the PDF and DOCX loader limits explicit"
    );
    assert!(
        readme.contains("bucket: \"docs-bucket\"") && readme.contains("prefix: \"kb/\""),
        "expected README to include a concrete S3 configuration example"
    );
}

#[test]
fn support_policy_describes_current_v0_4_support_boundary() {
    let support = read_repo_file("SUPPORT.md");

    assert!(
        support.contains("v0.4"),
        "expected SUPPORT.md to describe the current support boundary in v0.4 terms"
    );
    assert!(
        support.contains("local filesystem ingestion under one configured directory, plus polling S3 ingestion under one configured bucket/prefix"),
        "expected SUPPORT.md to describe both supported source shapes"
    );
    assert!(
        support.contains("deterministic PDF text loading")
            && support.contains("deterministic DOCX text extraction"),
        "expected SUPPORT.md to keep PDF and DOCX support boundaries explicit"
    );
    assert!(
        support.contains("Current PDF support is limited to embedded text extraction")
            && support.contains("Current DOCX support is limited to deterministic extracted text"),
        "expected SUPPORT.md to preserve the parser limitation details"
    );
}

#[test]
fn changelog_records_v0_4_release_alignment() {
    let changelog = read_repo_file("CHANGELOG.md");

    assert!(
        changelog.contains("## [0.4.0] - 2026-05-26")
            && changelog.contains("### Docs")
            && changelog.contains("v0.4")
            && changelog.contains("support matrix"),
        "expected the changelog to record the v0.4 release docs alignment work"
    );
}

#[test]
fn semantic_support_boundary_is_consistent_across_docs() {
    let readme = read_repo_file("README.md");
    let support = read_repo_file("SUPPORT.md");
    let changelog = read_repo_file("CHANGELOG.md");

    let support_phrase = "semantic chunking remains experimental and opt-in";
    let fastembed_phrase = "`fastembed` remains a feature-gated semantic provider";

    assert!(
        readme.contains(support_phrase) && readme.contains(fastembed_phrase),
        "expected README to describe the experimental semantic support boundary"
    );
    assert!(
        support.contains(support_phrase) && support.contains(fastembed_phrase),
        "expected SUPPORT.md to describe the experimental semantic support boundary"
    );
    assert!(
        changelog.contains(support_phrase) && changelog.contains(fastembed_phrase),
        "expected CHANGELOG.md to record the semantic support-boundary decision"
    );
}

#[test]
fn state_compatibility_contract_is_consistent_across_docs() {
    let readme = read_repo_file("README.md");
    let support = read_repo_file("SUPPORT.md");
    let changelog = read_repo_file("CHANGELOG.md");

    let readme_direct_read_phrase = "directly reads the supported released `v0.4.x` WAL format";
    let support_direct_read_phrase = "directly reads supported released `v0.4.x` WAL state";
    let min_version_phrase = "v0.4.0";
    let fail_closed_phrase = "truncated final writes";

    assert!(
        readme.contains("State compatibility contract")
            && readme.contains(readme_direct_read_phrase)
            && readme.contains(min_version_phrase)
            && readme.contains(fail_closed_phrase),
        "expected README to define the v0.5 state compatibility contract"
    );
    assert!(
        support.contains("on-disk state compatibility contract")
            && support.contains(support_direct_read_phrase)
            && support.contains(min_version_phrase)
            && support.contains(fail_closed_phrase),
        "expected SUPPORT.md to treat the state compatibility contract as supported surface"
    );
    assert!(
        changelog.contains("on-disk compatibility contract")
            && changelog.contains(min_version_phrase)
            && changelog.contains(fail_closed_phrase),
        "expected CHANGELOG.md to record the state compatibility contract work"
    );
}

#[test]
fn v0_5_compatibility_boundary_is_consistent_across_docs() {
    let readme = read_repo_file("README.md");
    let support = read_repo_file("SUPPORT.md");
    let changelog = read_repo_file("CHANGELOG.md");

    for (name, document) in [
        ("README.md", readme.as_str()),
        ("SUPPORT.md", support.as_str()),
        ("CHANGELOG.md", changelog.as_str()),
    ] {
        assert!(
            document.contains("v0.5 compatibility boundary"),
            "expected {name} to name the v0.5 compatibility boundary"
        );
        assert!(
            document.contains("Qdrant point ID is the chunk identity"),
            "expected {name} to define the point-ID compatibility surface"
        );
        assert!(
            document.contains("not duplicated as a `chunk_id` payload field"),
            "expected {name} to distinguish point identity from payload fields"
        );
        assert!(
            document
                .contains("Strategy-fingerprint changes intentionally open a new point-ID space."),
            "expected {name} to explain when reindexing is required"
        );
        assert!(
            document.contains("`canonical_path`")
                && document.contains("`doc_id`")
                && document.contains("`chunk_index`")
                && document.contains("`total_chunks`")
                && document.contains("`previous_chunk_id`")
                && document.contains("`next_chunk_id`")
                && document.contains("`strategy_fingerprint`")
                && document.contains("`chunk_text_sha256`"),
            "expected {name} to name the stable Qdrant payload fields"
        );
        assert!(
            document.contains("`chunk_text` is optional compatibility data"),
            "expected {name} to distinguish optional payload text"
        );
        assert!(
            document.contains("default embedding backend remains OpenAI")
                && document.contains("default chunker mode remains `router`")
                && document.contains("semantic chunking remains experimental and opt-in"),
            "expected {name} to preserve the stable CLI defaults and experimental boundary"
        );
        assert!(
            document.contains("Incompatible changes require release-note migration guidance."),
            "expected {name} to require migration guidance for incompatible changes"
        );
    }
}
