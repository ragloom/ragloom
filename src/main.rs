//! Ragloom CLI runner.
//!
//! # Why
//! The library crate contains most logic and is reusable by other programs.
//! This binary provides the minimum wiring to run the real I/O pipeline in a
//! single daemon-style process.

use std::time::Duration;

use ragloom::doc::FsUtf8Loader;
use ragloom::embed::http_client::{HttpEmbeddingClient, HttpEmbeddingConfig};
use ragloom::error::{RagloomError, RagloomErrorKind};
use ragloom::pipeline::runtime::{
    AckingExecutor, AsyncRuntime, PipelineExecutor, Runtime, run_worker,
};
use ragloom::sink::qdrant::{QdrantConfig, QdrantSink};
use ragloom::source::dir_scanner::DirectoryScannerSource;

/// Runtime configuration constructed from CLI arguments.
///
/// # Why
/// Keeping configuration in a struct makes the CLI parsing testable and keeps
/// `main()` focused on wiring.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RunConfig {
    pub dir: String,
    pub embed_backend: EmbedBackend,
    pub qdrant_url: String,
    pub collection: String,
    pub chunker_strategy: String,
    pub size_metric: String,
    pub size_max: usize,
    pub size_min: usize,
    pub size_overlap: usize,
    pub tokenizer: String,
    pub chunker_mode: String,
    pub chunker_single: Option<String>,
    pub enable_semantic: bool,
    pub semantic_provider: String,
    pub semantic_percentile: u8,
}

/// Embedding backend selection.
///
/// # Why
/// Keeping selection as an enum makes backend-specific required flags explicit
/// and prevents invalid combinations from reaching wiring.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum EmbedBackend {
    OpenAi {
        endpoint: String,
        api_key: String,
        model: String,
    },
    Http {
        url: String,
        model: String,
    },
}

/// Parse CLI arguments into a [`RunConfig`].
///
/// # Why
/// Using `std::env::args` keeps the binary dependency-free while still allowing
/// deterministic unit tests for argument handling.
pub fn parse_args(args: &[String]) -> Result<RunConfig, RagloomError> {
    let mut dir: Option<String> = None;
    let mut embed_backend: Option<String> = None;

    let mut embed_url: Option<String> = None;
    let mut embed_model: Option<String> = None;

    let mut openai_endpoint: Option<String> = None;
    let mut openai_api_key: Option<String> = None;
    let mut openai_model: Option<String> = None;

    let mut qdrant_url: Option<String> = None;
    let mut collection: Option<String> = None;

    let mut chunker_strategy: Option<String> = None;
    let mut size_metric: Option<String> = None;
    let mut size_max: Option<String> = None;
    let mut size_min: Option<String> = None;
    let mut size_overlap: Option<String> = None;
    let mut tokenizer: Option<String> = None;
    let mut chunker_mode: Option<String> = None;
    let mut chunker_single: Option<String> = None;
    let mut enable_semantic = false;
    let mut semantic_provider: Option<String> = None;
    let mut semantic_percentile: Option<String> = None;

    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let (flag, inline_value) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (arg.as_str(), None),
        };

        let mut next_value = || {
            inline_value
                .map(str::to_string)
                .or_else(|| iter.next().cloned())
        };

        match flag {
            "--dir" => dir = next_value(),

            "--embed-backend" => embed_backend = next_value(),

            "--embed-url" => embed_url = next_value(),
            "--embed-model" => embed_model = next_value(),

            "--openai-endpoint" => openai_endpoint = next_value(),
            "--openai-api-key" => openai_api_key = next_value(),
            "--openai-model" => openai_model = next_value(),

            "--qdrant-url" => qdrant_url = next_value(),
            "--collection" => collection = next_value(),

            "--chunker-strategy" => chunker_strategy = next_value(),
            "--size-metric" => size_metric = next_value(),
            "--size-max" => size_max = next_value(),
            "--size-min" => size_min = next_value(),
            "--size-overlap" => size_overlap = next_value(),
            "--tokenizer" => tokenizer = next_value(),
            "--chunker-mode" => chunker_mode = next_value(),
            "--chunker-single" => chunker_single = next_value(),
            "--enable-semantic" => {
                enable_semantic = true;
            }
            "--semantic-provider" => semantic_provider = next_value(),
            "--semantic-percentile" => semantic_percentile = next_value(),
            "--help" | "-h" => {
                return Err(RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(
                    "usage: ragloom --dir <path> --qdrant-url <url> --collection <name> [--embed-backend <openai|http>]",
                ));
            }
            unknown => {
                return Err(RagloomError::from_kind(RagloomErrorKind::InvalidInput)
                    .with_context(format!("unknown flag: {unknown}")));
            }
        }
    }

    let dir = dir.ok_or_else(|| {
        RagloomError::from_kind(RagloomErrorKind::Config)
            .with_context("missing required flag: --dir")
    })?;

    let qdrant_url = qdrant_url.ok_or_else(|| {
        RagloomError::from_kind(RagloomErrorKind::Config)
            .with_context("missing required flag: --qdrant-url")
    })?;
    let collection = collection.ok_or_else(|| {
        RagloomError::from_kind(RagloomErrorKind::Config)
            .with_context("missing required flag: --collection")
    })?;

    let backend = embed_backend.unwrap_or_else(|| "openai".to_string());

    tracing::info!(
        event.name = "ragloom.start",
        dir = %dir,
        embed_backend = %backend,
        qdrant_collection = %collection,
        "ragloom.start"
    );

    let embed_backend = match backend.as_str() {
        "openai" => {
            let endpoint = openai_endpoint
                .unwrap_or_else(|| "https://api.openai.com/v1/embeddings".to_string());
            let api_key = openai_api_key.ok_or_else(|| {
                RagloomError::from_kind(RagloomErrorKind::Config)
                    .with_context("missing required flag for openai backend: --openai-api-key")
            })?;
            let model = openai_model.unwrap_or_else(|| "text-embedding-3-small".to_string());
            EmbedBackend::OpenAi {
                endpoint,
                api_key,
                model,
            }
        }
        "http" => {
            let url = embed_url.ok_or_else(|| {
                RagloomError::from_kind(RagloomErrorKind::Config)
                    .with_context("missing required flag for http backend: --embed-url")
            })?;
            let model = embed_model.unwrap_or_else(|| "default".to_string());
            EmbedBackend::Http { url, model }
        }
        other => {
            return Err(
                RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(format!(
                    "invalid value for --embed-backend: {other} (expected: openai|http)"
                )),
            );
        }
    };

    let chunker_strategy = chunker_strategy.unwrap_or_else(|| "recursive".to_string());
    match chunker_strategy.as_str() {
        "recursive" | "legacy" => {}
        other => {
            return Err(
                RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(format!(
                    "invalid --chunker-strategy: {other} (expected: recursive|legacy)"
                )),
            );
        }
    }

    let size_metric = size_metric.unwrap_or_else(|| "chars".to_string());
    match size_metric.as_str() {
        "chars" | "tokens" => {}
        other => {
            return Err(
                RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(format!(
                    "invalid --size-metric: {other} (expected: chars|tokens)"
                )),
            );
        }
    }

    let size_max = size_max
        .map(|s| {
            s.parse::<usize>().map_err(|e| {
                RagloomError::from_kind(RagloomErrorKind::InvalidInput)
                    .with_context(format!("--size-max must be integer: {e}"))
            })
        })
        .transpose()?
        .unwrap_or(if size_metric == "tokens" { 512 } else { 2000 });

    let size_min = size_min
        .map(|s| {
            s.parse::<usize>().map_err(|e| {
                RagloomError::from_kind(RagloomErrorKind::InvalidInput)
                    .with_context(format!("--size-min must be integer: {e}"))
            })
        })
        .transpose()?
        .unwrap_or(0);

    let size_overlap = size_overlap
        .map(|s| {
            s.parse::<usize>().map_err(|e| {
                RagloomError::from_kind(RagloomErrorKind::InvalidInput)
                    .with_context(format!("--size-overlap must be integer: {e}"))
            })
        })
        .transpose()?
        .unwrap_or(0);

    let tokenizer = tokenizer.unwrap_or_else(|| "tiktoken-cl100k".to_string());
    match tokenizer.as_str() {
        "tiktoken-cl100k" => {}
        other => {
            return Err(
                RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(format!(
                    "invalid --tokenizer: {other} (expected: tiktoken-cl100k)"
                )),
            );
        }
    }

    let chunker_mode = chunker_mode.unwrap_or_else(|| "router".to_string());
    match chunker_mode.as_str() {
        "router" | "single" => {}
        other => {
            return Err(
                RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(format!(
                    "invalid --chunker-mode: {other} (expected: router|single)"
                )),
            );
        }
    }
    if chunker_mode == "single" && chunker_single.is_none() {
        return Err(RagloomError::from_kind(RagloomErrorKind::Config)
            .with_context("--chunker-mode=single requires --chunker-single"));
    }

    if enable_semantic && chunker_mode == "single" && chunker_single.as_deref() != Some("semantic")
    {
        return Err(
            RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(
                "--enable-semantic is only honored with --chunker-mode=router or \
             --chunker-mode=single with --chunker-single=semantic",
            ),
        );
    }

    let semantic_provider = semantic_provider.unwrap_or_else(|| "adapter".to_string());
    match semantic_provider.as_str() {
        "adapter" => {}
        "fastembed" => {
            #[cfg(not(feature = "fastembed"))]
            {
                return Err(
                    RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(
                        "--semantic-provider=fastembed requires the \"fastembed\" Cargo feature",
                    ),
                );
            }
        }
        other => {
            return Err(
                RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(format!(
                    "invalid --semantic-provider: {other} (expected: adapter|fastembed)"
                )),
            );
        }
    }

    let semantic_percentile = semantic_percentile
        .map(|s| {
            s.parse::<u8>().map_err(|e| {
                RagloomError::from_kind(RagloomErrorKind::InvalidInput)
                    .with_context(format!("--semantic-percentile must be 1..=99: {e}"))
            })
        })
        .transpose()?
        .unwrap_or(95);
    if !(1..=99).contains(&semantic_percentile) {
        return Err(
            RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(format!(
                "--semantic-percentile must be in 1..=99, got {semantic_percentile}"
            )),
        );
    }

    Ok(RunConfig {
        dir,
        embed_backend,
        qdrant_url,
        collection,
        chunker_strategy,
        size_metric,
        size_max,
        size_min,
        size_overlap,
        tokenizer,
        chunker_mode,
        chunker_single,
        enable_semantic,
        semantic_provider,
        semantic_percentile,
    })
}

fn parse_code_lang(s: &str) -> Result<ragloom::transform::chunker::code::Language, RagloomError> {
    use ragloom::transform::chunker::code::Language;
    match s {
        "rust" => Ok(Language::Rust),
        "python" => Ok(Language::Python),
        "javascript" => Ok(Language::JavaScript),
        "typescript" => Ok(Language::TypeScript),
        "tsx" => Ok(Language::Tsx),
        "go" => Ok(Language::Go),
        "java" => Ok(Language::Java),
        "c" => Ok(Language::C),
        "cpp" => Ok(Language::Cpp),
        "ruby" => Ok(Language::Ruby),
        "bash" => Ok(Language::Bash),
        other => Err(RagloomError::from_kind(RagloomErrorKind::InvalidInput)
            .with_context(format!("unsupported language: {other}"))),
    }
}

fn embedding_fingerprint(cfg: &RunConfig) -> String {
    match &cfg.embed_backend {
        EmbedBackend::OpenAi { model, .. } => format!("openai:{}", model),
        EmbedBackend::Http { model, .. } => format!("http:{}", model),
    }
}

#[tokio::main]
async fn main() {
    if let Err(err) = try_main().await {
        tracing::error!(
            error.message = %err,
            error.kind = %err.kind,
            "ragloom.fatal"
        );
        std::process::exit(1);
    }
}

async fn try_main() -> Result<(), RagloomError> {
    let obs_cfg = ragloom::observability::load_from_process_env()?;
    let dispatch = ragloom::observability::init_subscriber(&obs_cfg)?;
    tracing::dispatcher::set_global_default(dispatch).map_err(|e| {
        RagloomError::new(RagloomErrorKind::Internal, e)
            .with_context("failed to install tracing subscriber")
    })?;

    tracing::info!(
        event.name = "ragloom.log_config",
        log_format = ?obs_cfg.format,
        log_filter = %obs_cfg.filter_directives,
        "ragloom.log_config"
    );

    let args: Vec<String> = std::env::args().collect();
    let cfg = parse_args(&args)?;

    let source = DirectoryScannerSource::new(&cfg.dir).map_err(|e| {
        RagloomError::new(RagloomErrorKind::Io, e)
            .with_context("failed to create directory scanner source")
    })?;

    let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
        ragloom::state::wal::InMemoryWal::new(),
    ));

    let runtime = Runtime::with_shared_wal(source, std::sync::Arc::clone(&wal));
    let (queue, shutdown) = AsyncRuntime::new(runtime, 128).start();

    let embed_fingerprint = embedding_fingerprint(&cfg);

    let embedding: std::sync::Arc<dyn ragloom::embed::EmbeddingProvider + Send + Sync> =
        match cfg.embed_backend {
            EmbedBackend::OpenAi {
                endpoint,
                api_key,
                model,
            } => {
                let client = ragloom::embed::openai_client::OpenAiEmbeddingClient::new(
                    ragloom::embed::openai_client::OpenAiEmbeddingConfig {
                        endpoint,
                        api_key,
                        model,
                        timeout: Duration::from_secs(30),
                    },
                )
                .map_err(|e| e.with_context("failed to build OpenAI embedding client"))?;
                std::sync::Arc::new(client)
            }
            EmbedBackend::Http { url, model } => {
                let client = HttpEmbeddingClient::new(HttpEmbeddingConfig {
                    endpoint: url,
                    model,
                    timeout: Duration::from_secs(30),
                })
                .map_err(|e| e.with_context("failed to build HTTP embedding client"))?;
                std::sync::Arc::new(client)
            }
        };

    let sink = QdrantSink::new(QdrantConfig {
        base_url: cfg.qdrant_url,
        collection: cfg.collection,
        timeout: Duration::from_secs(30),
    })
    .map_err(|e| e.with_context("failed to build Qdrant sink"))?;

    let metric = match cfg.size_metric.as_str() {
        "chars" => ragloom::transform::chunker::size::SizeMetric::Chars,
        "tokens" => ragloom::transform::chunker::size::SizeMetric::Tokens,
        _ => unreachable!("validated in parse_args"),
    };

    if cfg.tokenizer != "tiktoken-cl100k" {
        return Err(
            RagloomError::from_kind(RagloomErrorKind::Config).with_context(format!(
                "unsupported --tokenizer: {} (phase 1 supports only: tiktoken-cl100k)",
                cfg.tokenizer
            )),
        );
    }
    tracing::info!(
        event.name = "ragloom.chunker.tokenizer_selected",
        tokenizer = %cfg.tokenizer,
        "ragloom.chunker.tokenizer_selected"
    );

    let rec_cfg = ragloom::transform::chunker::recursive::RecursiveConfig {
        metric,
        max_size: cfg.size_max,
        min_size: cfg.size_min,
        overlap: cfg.size_overlap,
    };

    if cfg.chunker_strategy == "legacy" {
        tracing::warn!(
            event.name = "ragloom.chunker.legacy_alias",
            "--chunker-strategy=legacy currently routes through the recursive chunker; \
             retained as a rollback seam for future phases"
        );
    }

    use ragloom::transform::chunker::{
        Chunker, EmbeddingProviderAdapter, MarkdownChunker, SemanticChunker,
        SemanticSignalProvider, default_router, recursive::RecursiveChunker, semantic_router,
    };

    let chunker: std::sync::Arc<dyn Chunker> = if cfg.chunker_mode == "router"
        && cfg.enable_semantic
    {
        let signal: std::sync::Arc<dyn SemanticSignalProvider> =
            match cfg.semantic_provider.as_str() {
                "adapter" => std::sync::Arc::new(EmbeddingProviderAdapter::new(
                    std::sync::Arc::clone(&embedding),
                    embed_fingerprint.clone(),
                )),
                #[cfg(feature = "fastembed")]
                "fastembed" => std::sync::Arc::new(
                    ragloom::transform::chunker::FastembedSignalProvider::new().map_err(|e| {
                        RagloomError::new(RagloomErrorKind::Config, e)
                            .with_context("fastembed init")
                    })?,
                ),
                other => {
                    return Err(RagloomError::from_kind(RagloomErrorKind::InvalidInput)
                        .with_context(format!("unsupported --semantic-provider: {other}")));
                }
            };
        let semantic_chunker: std::sync::Arc<dyn Chunker> = std::sync::Arc::new(
            SemanticChunker::new(signal, rec_cfg, cfg.semantic_percentile).map_err(|e| {
                RagloomError::new(RagloomErrorKind::Config, e)
                    .with_context("invalid semantic config")
            })?,
        );
        std::sync::Arc::new(semantic_router(rec_cfg, semantic_chunker).map_err(|e| {
            RagloomError::new(RagloomErrorKind::Config, e)
                .with_context("invalid semantic router config")
        })?)
    } else {
        match cfg.chunker_mode.as_str() {
            "router" => std::sync::Arc::new(default_router(rec_cfg).map_err(|e| {
                RagloomError::new(RagloomErrorKind::Config, e).with_context("invalid router config")
            })?),
            "single" => {
                let kind = cfg.chunker_single.as_deref().unwrap();
                match kind {
                    "semantic" => {
                        let signal: std::sync::Arc<dyn SemanticSignalProvider> =
                            std::sync::Arc::new(EmbeddingProviderAdapter::new(
                                std::sync::Arc::clone(&embedding),
                                embed_fingerprint.clone(),
                            ));
                        std::sync::Arc::new(
                            SemanticChunker::new(signal, rec_cfg, cfg.semantic_percentile)
                                .map_err(|e| {
                                    RagloomError::new(RagloomErrorKind::Config, e)
                                        .with_context("invalid semantic config")
                                })?,
                        )
                    }
                    "recursive" => {
                        std::sync::Arc::new(RecursiveChunker::new(rec_cfg).map_err(|e| {
                            RagloomError::new(RagloomErrorKind::Config, e)
                                .with_context("invalid chunker config")
                        })?)
                    }
                    "markdown" => {
                        std::sync::Arc::new(MarkdownChunker::new(rec_cfg).map_err(|e| {
                            RagloomError::new(RagloomErrorKind::Config, e)
                                .with_context("invalid markdown config")
                        })?)
                    }
                    s if s.starts_with("code:") => {
                        let lang = parse_code_lang(&s[5..])?;
                        std::sync::Arc::new(
                            ragloom::transform::chunker::CodeChunker::new(lang, rec_cfg).map_err(
                                |e| {
                                    RagloomError::new(RagloomErrorKind::Config, e)
                                        .with_context("invalid code config")
                                },
                            )?,
                        )
                    }
                    other => {
                        return Err(RagloomError::from_kind(RagloomErrorKind::InvalidInput)
                            .with_context(format!("invalid --chunker-single: {other}")));
                    }
                }
            }
            _ => unreachable!("validated in parse_args"),
        }
    };

    let pipeline = PipelineExecutor::with_chunker(
        embedding,
        std::sync::Arc::new(sink),
        std::sync::Arc::new(FsUtf8Loader),
        chunker,
    );

    let executor = AckingExecutor {
        inner: pipeline,
        wal: std::sync::Arc::clone(&wal),
    };

    let worker = tokio::spawn(async move {
        run_worker(queue, executor).await;
    });

    tokio::signal::ctrl_c().await.map_err(|e| {
        RagloomError::new(RagloomErrorKind::Internal, e)
            .with_context("failed to install Ctrl-C handler")
    })?;

    shutdown.shutdown();
    let _ = worker.await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_returns_error_when_required_flags_missing() {
        let args = vec!["ragloom".to_string()];
        let err = parse_args(&args).expect_err("expected error");
        assert_eq!(err.kind, RagloomErrorKind::Config);
        assert!(err.to_string().contains("missing required flag"));
    }

    #[test]
    fn parse_args_defaults_to_openai_backend_and_requires_api_key() {
        let args = vec![
            "ragloom".to_string(),
            "--dir".to_string(),
            "/tmp/docs".to_string(),
            "--qdrant-url".to_string(),
            "http://qdrant".to_string(),
            "--collection".to_string(),
            "docs".to_string(),
        ];

        let err = parse_args(&args).expect_err("expected error");
        assert_eq!(err.kind, RagloomErrorKind::Config);
        assert!(
            err.to_string()
                .contains("missing required flag for openai backend")
        );
    }

    #[test]
    fn parse_args_returns_config_when_all_flags_are_present() {
        let args = vec![
            "ragloom".to_string(),
            "--dir".to_string(),
            "/tmp/docs".to_string(),
            "--embed-backend".to_string(),
            "http".to_string(),
            "--embed-url".to_string(),
            "http://embed".to_string(),
            "--embed-model".to_string(),
            "default".to_string(),
            "--qdrant-url".to_string(),
            "http://qdrant".to_string(),
            "--collection".to_string(),
            "docs".to_string(),
        ];

        let cfg = parse_args(&args).expect("config");
        assert_eq!(
            cfg,
            RunConfig {
                dir: "/tmp/docs".to_string(),
                embed_backend: EmbedBackend::Http {
                    url: "http://embed".to_string(),
                    model: "default".to_string(),
                },
                qdrant_url: "http://qdrant".to_string(),
                collection: "docs".to_string(),
                chunker_strategy: "recursive".to_string(),
                size_metric: "chars".to_string(),
                size_max: 2000,
                size_min: 0,
                size_overlap: 0,
                tokenizer: "tiktoken-cl100k".to_string(),
                chunker_mode: "router".to_string(),
                chunker_single: None,
                enable_semantic: false,
                semantic_provider: "adapter".to_string(),
                semantic_percentile: 95,
            }
        );
    }

    #[test]
    fn enable_semantic_errors_in_single_mode_without_semantic() {
        let args = vec![
            "ragloom".to_string(),
            "--dir".to_string(),
            "/tmp/docs".to_string(),
            "--embed-backend".to_string(),
            "http".to_string(),
            "--embed-url".to_string(),
            "http://embed".to_string(),
            "--embed-model".to_string(),
            "default".to_string(),
            "--qdrant-url".to_string(),
            "http://qdrant".to_string(),
            "--collection".to_string(),
            "docs".to_string(),
            "--chunker-mode".to_string(),
            "single".to_string(),
            "--chunker-single".to_string(),
            "recursive".to_string(),
            "--enable-semantic".to_string(),
        ];
        let err = parse_args(&args).expect_err("must reject");
        assert!(err.to_string().contains("--enable-semantic"));
    }
}
