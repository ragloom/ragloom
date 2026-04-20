//! Extension-keyed dispatch to content-aware chunkers.
//!
//! # Why
//! Different file types want different splitting strategies. The Router does
//! one lookup per chunk call and delegates. The returned [`ChunkedDocument`]
//! carries the delegated chunker's fingerprint, so point IDs are stamped
//! with the *actual* strategy used, not the Router itself.

use std::collections::HashMap;
use std::sync::Arc;

use super::code::{CodeChunker, Language};
use super::error::ChunkResult;
use super::fingerprint::StrategyFingerprint;
use super::markdown::MarkdownChunker;
use super::recursive::{RecursiveChunker, RecursiveConfig};
use super::{ChunkHint, ChunkedDocument, Chunker};

pub struct ChunkerRouter {
    by_extension: HashMap<&'static str, Arc<dyn Chunker>>,
    default: Arc<dyn Chunker>,
    fingerprint: StrategyFingerprint,
}

impl ChunkerRouter {
    pub fn builder(default: Arc<dyn Chunker>) -> ChunkerRouterBuilder {
        ChunkerRouterBuilder {
            by_extension: HashMap::new(),
            default,
        }
    }

    /// The router's own configuration fingerprint (for diagnostics only).
    ///
    /// # Why
    /// The router does not stamp this into [`ChunkedDocument`] — each
    /// document carries the delegate chunker's fingerprint instead. This
    /// getter exists purely for startup-time configuration logging. Do not
    /// mix it into any hash that feeds the point-ID scheme.
    pub fn config_fingerprint(&self) -> &StrategyFingerprint {
        &self.fingerprint
    }
}

pub struct ChunkerRouterBuilder {
    by_extension: HashMap<&'static str, Arc<dyn Chunker>>,
    default: Arc<dyn Chunker>,
}

impl ChunkerRouterBuilder {
    pub fn register(mut self, extension: &'static str, chunker: Arc<dyn Chunker>) -> Self {
        debug_assert!(
            !self.by_extension.contains_key(extension),
            "ChunkerRouter::register called twice for extension {:?}",
            extension
        );
        self.by_extension.insert(extension, chunker);
        self
    }

    pub fn build(self) -> ChunkerRouter {
        let mut summary = String::from("router:v1");
        let mut keys: Vec<&&'static str> = self.by_extension.keys().collect();
        keys.sort();
        for key in keys {
            summary.push_str(&format!("|{key}=on"));
        }
        let fingerprint = StrategyFingerprint::new(summary);
        ChunkerRouter {
            by_extension: self.by_extension,
            default: self.default,
            fingerprint,
        }
    }
}

impl Chunker for ChunkerRouter {
    fn chunk(&self, text: &str, hint: &ChunkHint<'_>) -> ChunkResult<ChunkedDocument> {
        let ext = hint.extension.map(|e| e.to_ascii_lowercase());
        let chunker = ext
            .as_deref()
            .and_then(|e| self.by_extension.get(e))
            .cloned()
            .unwrap_or_else(|| Arc::clone(&self.default));
        let extension_label = ext.as_deref().unwrap_or("<none>").to_string();

        let doc = chunker.chunk(text, hint)?;
        tracing::debug!(
            event.name = "ragloom.chunker.router.dispatch",
            extension = %extension_label,
            strategy = %doc.strategy_fingerprint,
            "ragloom.chunker.router.dispatch"
        );
        Ok(doc)
    }
}

/// Production default: Markdown for `md/markdown/mdx`, CodeChunker for 10
/// languages, RecursiveChunker as fallback.
pub fn default_router(base: RecursiveConfig) -> ChunkResult<ChunkerRouter> {
    let default: Arc<dyn Chunker> = Arc::new(RecursiveChunker::new(base)?);
    let md: Arc<dyn Chunker> = Arc::new(MarkdownChunker::new(base)?);
    let mk_code =
        |lang| -> ChunkResult<Arc<dyn Chunker>> { Ok(Arc::new(CodeChunker::new(lang, base)?)) };

    let rust = mk_code(Language::Rust)?;
    let py = mk_code(Language::Python)?;
    let js = mk_code(Language::JavaScript)?;
    let ts = mk_code(Language::TypeScript)?;
    let tsx = mk_code(Language::Tsx)?;
    let go = mk_code(Language::Go)?;
    let java = mk_code(Language::Java)?;
    let c_lang = mk_code(Language::C)?;
    let cpp = mk_code(Language::Cpp)?;
    let ruby = mk_code(Language::Ruby)?;
    let bash = mk_code(Language::Bash)?;

    Ok(ChunkerRouter::builder(default)
        .register("md", Arc::clone(&md))
        .register("markdown", Arc::clone(&md))
        .register("mdx", md)
        .register("rs", rust)
        .register("py", Arc::clone(&py))
        .register("pyi", py)
        .register("js", Arc::clone(&js))
        .register("mjs", Arc::clone(&js))
        .register("cjs", Arc::clone(&js))
        .register("jsx", js)
        .register("ts", Arc::clone(&ts))
        .register("tsx", tsx)
        .register("go", go)
        .register("java", java)
        .register("c", Arc::clone(&c_lang))
        .register("h", c_lang)
        .register("cpp", Arc::clone(&cpp))
        .register("cc", Arc::clone(&cpp))
        .register("cxx", Arc::clone(&cpp))
        .register("hpp", Arc::clone(&cpp))
        .register("hh", Arc::clone(&cpp))
        .register("hxx", cpp)
        .register("rb", ruby)
        .register("sh", Arc::clone(&bash))
        .register("bash", bash)
        .build())
}

/// Router variant that replaces Markdown and the default fallback with
/// a caller-supplied `SemanticChunker`. Code extensions retain their
/// Phase 2 `CodeChunker` instances.
pub fn semantic_router(
    base: RecursiveConfig,
    semantic: Arc<dyn Chunker>,
) -> ChunkResult<ChunkerRouter> {
    use super::code::{CodeChunker, Language};

    let mk_code =
        |lang| -> ChunkResult<Arc<dyn Chunker>> { Ok(Arc::new(CodeChunker::new(lang, base)?)) };

    let rust = mk_code(Language::Rust)?;
    let py = mk_code(Language::Python)?;
    let js = mk_code(Language::JavaScript)?;
    let ts = mk_code(Language::TypeScript)?;
    let tsx = mk_code(Language::Tsx)?;
    let go = mk_code(Language::Go)?;
    let java = mk_code(Language::Java)?;
    let c_lang = mk_code(Language::C)?;
    let cpp = mk_code(Language::Cpp)?;
    let ruby = mk_code(Language::Ruby)?;
    let bash = mk_code(Language::Bash)?;

    Ok(ChunkerRouter::builder(Arc::clone(&semantic))
        .register("md", Arc::clone(&semantic))
        .register("markdown", Arc::clone(&semantic))
        .register("mdx", Arc::clone(&semantic))
        .register("rs", rust)
        .register("py", Arc::clone(&py))
        .register("pyi", py)
        .register("js", Arc::clone(&js))
        .register("mjs", Arc::clone(&js))
        .register("cjs", Arc::clone(&js))
        .register("jsx", js)
        .register("ts", Arc::clone(&ts))
        .register("tsx", tsx)
        .register("go", go)
        .register("java", java)
        .register("c", Arc::clone(&c_lang))
        .register("h", c_lang)
        .register("cpp", Arc::clone(&cpp))
        .register("cc", Arc::clone(&cpp))
        .register("cxx", Arc::clone(&cpp))
        .register("hpp", Arc::clone(&cpp))
        .register("hh", Arc::clone(&cpp))
        .register("hxx", cpp)
        .register("rb", ruby)
        .register("sh", Arc::clone(&bash))
        .register("bash", bash)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::chunker::size::SizeMetric;

    fn cfg() -> RecursiveConfig {
        RecursiveConfig {
            metric: SizeMetric::Chars,
            max_size: 1000,
            min_size: 0,
            overlap: 0,
        }
    }

    #[test]
    fn unknown_extension_falls_back_to_default() {
        let router = default_router(cfg()).unwrap();
        let hint = ChunkHint::from_path("/tmp/notes.txt");
        let doc = router.chunk("hello world", &hint).unwrap();
        assert!(
            doc.strategy_fingerprint
                .as_str()
                .starts_with("recursive:v1")
        );
    }

    #[test]
    fn markdown_extension_routes_to_markdown() {
        let router = default_router(cfg()).unwrap();
        let hint = ChunkHint::from_path("/tmp/notes.md");
        let doc = router.chunk("# x\n\nbody\n", &hint).unwrap();
        assert!(doc.strategy_fingerprint.as_str().starts_with("markdown:v1"));
    }

    #[test]
    fn rust_extension_routes_to_code_rust() {
        let router = default_router(cfg()).unwrap();
        let hint = ChunkHint::from_path("/tmp/main.rs");
        let doc = router.chunk("fn a() {}\nfn b() {}\n", &hint).unwrap();
        assert!(doc.strategy_fingerprint.as_str().contains("lang=rust"));
    }

    #[test]
    fn extension_is_case_insensitive() {
        let router = default_router(cfg()).unwrap();
        let hint = ChunkHint::from_path("/tmp/MAIN.RS");
        let doc = router.chunk("fn a() {}\nfn b() {}\n", &hint).unwrap();
        assert!(doc.strategy_fingerprint.as_str().contains("lang=rust"));
    }
}

#[cfg(test)]
mod semantic_tests {
    use super::*;
    use crate::transform::chunker::semantic::{
        SemanticChunker, SemanticSignalProvider, signal::SemanticError,
    };
    use crate::transform::chunker::size::SizeMetric;
    use std::sync::Arc;

    struct StubSignal;
    impl SemanticSignalProvider for StubSignal {
        fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
            Ok(inputs.iter().map(|_| vec![1.0_f32, 0.0]).collect())
        }
        fn fingerprint(&self) -> &str {
            "stub:router"
        }
    }

    fn cfg() -> RecursiveConfig {
        RecursiveConfig {
            metric: SizeMetric::Chars,
            max_size: 1000,
            min_size: 0,
            overlap: 0,
        }
    }

    #[test]
    fn txt_routes_to_semantic_chunker() {
        let semantic: Arc<dyn Chunker> =
            Arc::new(SemanticChunker::new(Arc::new(StubSignal), cfg(), 95).unwrap());
        let router = semantic_router(cfg(), semantic).unwrap();
        let hint = ChunkHint::from_path("/tmp/notes.txt");
        let doc = router.chunk("A. B. C.", &hint).unwrap();
        assert!(doc.strategy_fingerprint.as_str().starts_with("semantic:v1"));
    }

    #[test]
    fn rs_keeps_code_rust_chunker() {
        let semantic: Arc<dyn Chunker> =
            Arc::new(SemanticChunker::new(Arc::new(StubSignal), cfg(), 95).unwrap());
        let router = semantic_router(cfg(), semantic).unwrap();
        let hint = ChunkHint::from_path("/tmp/main.rs");
        let doc = router.chunk("fn a() {}\nfn b() {}\n", &hint).unwrap();
        assert!(doc.strategy_fingerprint.as_str().contains("lang=rust"));
    }

    #[test]
    fn md_routes_to_semantic_chunker() {
        let semantic: Arc<dyn Chunker> =
            Arc::new(SemanticChunker::new(Arc::new(StubSignal), cfg(), 95).unwrap());
        let router = semantic_router(cfg(), semantic).unwrap();
        let hint = ChunkHint::from_path("/tmp/notes.md");
        let doc = router.chunk("# hi\n\nA. B.", &hint).unwrap();
        assert!(doc.strategy_fingerprint.as_str().starts_with("semantic:v1"));
    }
}
