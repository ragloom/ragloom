//! Ragloom CLI runner.
//!
//! # Why
//! The library crate contains most logic and is reusable by other programs.
//! This binary provides the minimum wiring to run the real I/O pipeline in a
//! single daemon-style process.

use std::time::Duration;

use ragloom::config::PipelineConfig;
use ragloom::doc::FsUtf8Loader;
use ragloom::embed::http_client::{HttpEmbeddingClient, HttpEmbeddingConfig};
use ragloom::error::{RagloomError, RagloomErrorKind};
use ragloom::pipeline::runtime::{
    AckingExecutor, AsyncRuntime, IngestionSummary, PipelineExecutor, Runtime, run_worker,
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
    pub state_path: String,
    pub create_collection_if_missing: bool,
    pub collection_vector_size: Option<usize>,
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

const DEFAULT_STATE_PATH: &str = ".ragloom/wal.ndjson";
const USAGE: &str = "usage: ragloom [--config <path>] --dir <path> --qdrant-url <url> --collection <name> [--state-path <path>] [--embed-backend <openai|http>]";

/// Top-level CLI command selected by argument parsing.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ParsedCommand {
    // Box the run config to keep this enum small enough for clippy's
    // `large_enum_variant` lint while still modeling early-exit commands cleanly.
    Run(Box<RunConfig>),
    Help,
    Version,
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

/// Parse CLI arguments into a top-level command.
///
/// # Why
/// Using `std::env::args` keeps the binary dependency-free while still allowing
/// deterministic unit tests for argument handling.
pub fn parse_args(args: &[String]) -> Result<ParsedCommand, RagloomError> {
    let mut config_path: Option<String> = None;
    let mut dir: Option<String> = None;
    let mut embed_backend: Option<String> = None;

    let mut embed_url: Option<String> = None;
    let mut embed_model: Option<String> = None;

    let mut openai_endpoint: Option<String> = None;
    let mut openai_api_key: Option<String> = None;
    let mut openai_model: Option<String> = None;

    let mut qdrant_url: Option<String> = None;
    let mut collection: Option<String> = None;
    let mut state_path: Option<String> = None;
    let mut create_collection_if_missing = false;
    let mut collection_vector_size: Option<String> = None;

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
            "--config" => config_path = next_value(),
            "--dir" => dir = next_value(),

            "--embed-backend" => embed_backend = next_value(),

            "--embed-url" => embed_url = next_value(),
            "--embed-model" => embed_model = next_value(),

            "--openai-endpoint" => openai_endpoint = next_value(),
            "--openai-api-key" => openai_api_key = next_value(),
            "--openai-model" => openai_model = next_value(),

            "--qdrant-url" => qdrant_url = next_value(),
            "--collection" => collection = next_value(),
            "--state-path" => state_path = next_value(),
            "--create-collection-if-missing" => {
                if inline_value.is_some() {
                    return Err(cli_invalid_input(
                        "--create-collection-if-missing does not accept a value",
                    ));
                }
                create_collection_if_missing = true;
            }
            "--collection-vector-size" => {
                collection_vector_size = Some(next_value().ok_or_else(|| {
                    cli_invalid_input("missing required value: --collection-vector-size")
                })?);
            }

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
            "--help" | "-h" => return Ok(ParsedCommand::Help),
            "--version" | "-V" => return Ok(ParsedCommand::Version),
            unknown => return Err(cli_invalid_input(format!("unknown flag: {unknown}"))),
        }
    }

    let file_config = config_path
        .as_deref()
        .map(load_pipeline_config)
        .transpose()?;

    let dir = dir
        .or_else(|| file_config.as_ref().map(|c| c.source.root.clone()))
        .ok_or_else(|| {
            cli_config_error("missing required value: --dir or source.root in --config")
        })?;

    let qdrant_url = qdrant_url
        .or_else(|| file_config.as_ref().map(|c| c.sink.qdrant_url.clone()))
        .ok_or_else(|| {
            cli_config_error("missing required value: --qdrant-url or sink.qdrant_url in --config")
        })?;
    let collection = collection
        .or_else(|| file_config.as_ref().map(|c| c.sink.collection.clone()))
        .ok_or_else(|| {
            cli_config_error("missing required value: --collection or sink.collection in --config")
        })?;
    let state_path = state_path
        .or_else(|| file_config.as_ref().map(|c| c.state.path.clone()))
        .unwrap_or_else(|| DEFAULT_STATE_PATH.to_string());
    if state_path.trim().is_empty() {
        return Err(cli_config_error("--state-path or state.path is empty"));
    }
    let collection_vector_size = collection_vector_size
        .map(|s| {
            s.parse::<usize>().map_err(|e| {
                RagloomError::from_kind(RagloomErrorKind::InvalidInput)
                    .with_context(format!("--collection-vector-size must be integer: {e}"))
            })
        })
        .transpose()?;
    if collection_vector_size == Some(0) {
        return Err(cli_invalid_input(
            "--collection-vector-size must be positive",
        ));
    }

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
                .or_else(|| file_config.as_ref().map(|c| c.embed.endpoint.clone()))
                .unwrap_or_else(|| "https://api.openai.com/v1/embeddings".to_string());
            let api_key = openai_api_key.ok_or_else(|| {
                cli_config_error("missing required flag for openai backend: --openai-api-key")
            })?;
            let model = openai_model.unwrap_or_else(|| "text-embedding-3-small".to_string());
            EmbedBackend::OpenAi {
                endpoint,
                api_key,
                model,
            }
        }
        "http" => {
            let url = embed_url
                .or_else(|| file_config.as_ref().map(|c| c.embed.endpoint.clone()))
                .ok_or_else(|| {
                    cli_config_error(
                        "missing required value for http backend: --embed-url or embed.endpoint in --config",
                    )
                })?;
            let model = embed_model.unwrap_or_else(|| "default".to_string());
            EmbedBackend::Http { url, model }
        }
        other => {
            return Err(cli_invalid_input(format!(
                "invalid value for --embed-backend: {other} (expected: openai|http)"
            )));
        }
    };

    let chunker_strategy = chunker_strategy.unwrap_or_else(|| "recursive".to_string());
    match chunker_strategy.as_str() {
        "recursive" | "legacy" => {}
        other => {
            return Err(cli_invalid_input(format!(
                "invalid --chunker-strategy: {other} (expected: recursive|legacy)"
            )));
        }
    }

    let size_metric = size_metric.unwrap_or_else(|| "chars".to_string());
    match size_metric.as_str() {
        "chars" | "tokens" => {}
        other => {
            return Err(cli_invalid_input(format!(
                "invalid --size-metric: {other} (expected: chars|tokens)"
            )));
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
            return Err(cli_invalid_input(format!(
                "invalid --tokenizer: {other} (expected: tiktoken-cl100k)"
            )));
        }
    }

    let chunker_mode = chunker_mode.unwrap_or_else(|| "router".to_string());
    match chunker_mode.as_str() {
        "router" | "single" => {}
        other => {
            return Err(cli_invalid_input(format!(
                "invalid --chunker-mode: {other} (expected: router|single)"
            )));
        }
    }
    if chunker_mode == "single" && chunker_single.is_none() {
        return Err(cli_config_error(
            "--chunker-mode=single requires --chunker-single",
        ));
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

    Ok(ParsedCommand::Run(Box::new(RunConfig {
        dir,
        embed_backend,
        qdrant_url,
        collection,
        state_path,
        create_collection_if_missing,
        collection_vector_size,
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
    })))
}

fn load_pipeline_config(path: &str) -> Result<PipelineConfig, RagloomError> {
    let yaml = std::fs::read_to_string(path).map_err(|e| {
        RagloomError::new(RagloomErrorKind::Io, e)
            .with_context(format!("failed to read config file: {path}"))
    })?;

    let cfg = PipelineConfig::from_yaml_str(&yaml)
        .map_err(|e| e.with_context(format!("failed to parse config file: {path}")))?;

    cfg.validate()
        .map_err(|e| e.with_context(format!("invalid config file: {path}")))?;

    Ok(cfg)
}

fn cli_invalid_input(message: impl Into<String>) -> RagloomError {
    let message = message.into();
    RagloomError::from_kind(RagloomErrorKind::InvalidInput)
        .with_context(format!("{message}\n{USAGE}"))
}

fn cli_config_error(message: impl Into<String>) -> RagloomError {
    let message = message.into();
    RagloomError::from_kind(RagloomErrorKind::Config).with_context(format!("{message}\n{USAGE}"))
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

fn resolve_collection_vector_size(cfg: &RunConfig) -> Result<usize, RagloomError> {
    if let Some(size) = cfg.collection_vector_size {
        return Ok(size);
    }

    match &cfg.embed_backend {
        EmbedBackend::OpenAi { model, .. } => match model.as_str() {
            "text-embedding-3-small" => Ok(1536),
            "text-embedding-3-large" => Ok(3072),
            "text-embedding-ada-002" => Ok(1536),
            other => Err(RagloomError::from_kind(RagloomErrorKind::Config).with_context(
                format!(
                    "unknown OpenAI model for collection vector size: {other}; pass --collection-vector-size"
                ),
            )),
        },
        EmbedBackend::Http { .. } => Err(RagloomError::from_kind(RagloomErrorKind::Config)
            .with_context("http backend requires --collection-vector-size")),
    }
}

async fn bootstrap_collection_if_needed(
    cfg: &RunConfig,
    sink: &QdrantSink,
) -> Result<(), RagloomError> {
    if !cfg.create_collection_if_missing {
        return Ok(());
    }

    let vector_size = resolve_collection_vector_size(cfg).map_err(|e| {
        RagloomError::new(e.kind, e).with_context("failed to bootstrap Qdrant collection")
    })?;
    sink.ensure_collection_exists(vector_size)
        .await
        .map_err(|e| {
            RagloomError::new(e.kind, e).with_context("failed to bootstrap Qdrant collection")
        })
}

struct PreparedStartup {
    embedding: std::sync::Arc<dyn ragloom::embed::EmbeddingProvider + Send + Sync>,
    sink: QdrantSink,
    source: DirectoryScannerSource,
    chunker: std::sync::Arc<dyn ragloom::transform::chunker::Chunker>,
}

async fn prepare_startup(cfg: &RunConfig) -> Result<PreparedStartup, RagloomError> {
    let embed_fingerprint = embedding_fingerprint(cfg);

    let embedding: std::sync::Arc<dyn ragloom::embed::EmbeddingProvider + Send + Sync> =
        match &cfg.embed_backend {
            EmbedBackend::OpenAi {
                endpoint,
                api_key,
                model,
            } => {
                let client = ragloom::embed::openai_client::OpenAiEmbeddingClient::new(
                    ragloom::embed::openai_client::OpenAiEmbeddingConfig {
                        endpoint: endpoint.clone(),
                        api_key: api_key.clone(),
                        model: model.clone(),
                        timeout: Duration::from_secs(30),
                    },
                )
                .map_err(|e| e.with_context("failed to build OpenAI embedding client"))?;
                std::sync::Arc::new(client)
            }
            EmbedBackend::Http { url, model } => {
                let client = HttpEmbeddingClient::new(HttpEmbeddingConfig {
                    endpoint: url.clone(),
                    model: model.clone(),
                    timeout: Duration::from_secs(30),
                })
                .map_err(|e| e.with_context("failed to build HTTP embedding client"))?;
                std::sync::Arc::new(client)
            }
        };

    let sink = QdrantSink::new(QdrantConfig {
        base_url: cfg.qdrant_url.clone(),
        collection: cfg.collection.clone(),
        timeout: Duration::from_secs(30),
    })
    .map_err(|e| e.with_context("failed to build Qdrant sink"))?;

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

    let metric = match cfg.size_metric.as_str() {
        "chars" => ragloom::transform::chunker::size::SizeMetric::Chars,
        "tokens" => ragloom::transform::chunker::size::SizeMetric::Tokens,
        _ => unreachable!("validated in parse_args"),
    };

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

    let source = DirectoryScannerSource::new(&cfg.dir).map_err(|e| {
        RagloomError::new(RagloomErrorKind::Io, e)
            .with_context("failed to create directory scanner source")
    })?;

    bootstrap_collection_if_needed(cfg, &sink).await?;

    Ok(PreparedStartup {
        embedding,
        sink,
        source,
        chunker,
    })
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
    let cfg = match parse_args(&args)? {
        ParsedCommand::Help => {
            println!("{USAGE}");
            return Ok(());
        }
        ParsedCommand::Version => {
            println!("ragloom {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        ParsedCommand::Run(cfg) => *cfg,
    };

    let PreparedStartup {
        embedding,
        sink,
        source,
        chunker,
    } = prepare_startup(&cfg).await?;

    let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
        ragloom::state::wal::FileWal::open(&cfg.state_path)
            .map_err(|e| e.with_context("failed to initialize persistent WAL"))?,
    ));

    let runtime = Runtime::with_shared_wal(source, std::sync::Arc::clone(&wal));
    let summary = IngestionSummary::default();
    let (queue, shutdown) = AsyncRuntime::new(runtime, 128)
        .with_summary(summary.clone())
        .start();

    let pipeline = PipelineExecutor::with_chunker(
        embedding,
        std::sync::Arc::new(sink),
        std::sync::Arc::new(FsUtf8Loader),
        chunker,
    )
    .with_summary(summary.clone());

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
    summary.emit_if_dirty("shutdown");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex};
    use tempfile::NamedTempFile;

    #[derive(Debug, Clone)]
    struct TestResponse {
        status: u16,
        reason: &'static str,
        body: &'static str,
    }

    struct RequestCounterServer {
        stop: Sender<()>,
        handle: std::thread::JoinHandle<usize>,
    }

    fn spawn_qdrant_bootstrap_server(
        responses: Vec<TestResponse>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));

        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();

            loop {
                let response = {
                    let mut guard = responses.lock().expect("lock");
                    guard.pop_front()
                };

                let Some(response) = response else {
                    break;
                };

                let (mut stream, _) = listener.accept().expect("accept");
                let mut buf = [0_u8; 8192];
                let mut request = Vec::new();

                loop {
                    let read = std::io::Read::read(&mut stream, &mut buf).expect("read");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..read]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let header_end = request
                            .windows(4)
                            .position(|w| w == b"\r\n\r\n")
                            .expect("header end")
                            + 4;
                        let headers = String::from_utf8_lossy(&request[..header_end]);
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                if name.eq_ignore_ascii_case("content-length") {
                                    value.trim().parse::<usize>().ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        while request.len() < header_end + content_length {
                            let read =
                                std::io::Read::read(&mut stream, &mut buf).expect("read body");
                            if read == 0 {
                                break;
                            }
                            request.extend_from_slice(&buf[..read]);
                        }
                        break;
                    }
                }

                requests.push(String::from_utf8_lossy(&request).into_owned());
                write!(
                    stream,
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                    response.status,
                    response.reason,
                    response.body.len(),
                    response.body
                )
                .expect("write response");
            }

            requests
        });

        (format!("http://{}", addr), handle)
    }

    fn spawn_qdrant_request_counter_server() -> (String, RequestCounterServer) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let addr = listener.local_addr().expect("addr");
        let (stop_tx, stop_rx) = mpsc::channel();

        let handle = std::thread::spawn(move || {
            let mut requests = 0;

            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buf = [0_u8; 8192];
                        let _ = std::io::Read::read(&mut stream, &mut buf);
                        requests += 1;
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: 15\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{{\"status\":\"ok\"}}"
                        )
                        .expect("write response");
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if stop_rx.try_recv().is_ok() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept failed: {err}"),
                }
            }

            requests
        });

        (
            format!("http://{}", addr),
            RequestCounterServer {
                stop: stop_tx,
                handle,
            },
        )
    }

    #[test]
    fn parse_args_returns_error_when_required_flags_missing() {
        let args = vec!["ragloom".to_string()];
        let err = parse_args(&args).expect_err("expected error");
        assert_eq!(err.kind, RagloomErrorKind::Config);
        assert!(err.to_string().contains("missing required value"));
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
            ParsedCommand::Run(Box::new(RunConfig {
                dir: "/tmp/docs".to_string(),
                embed_backend: EmbedBackend::Http {
                    url: "http://embed".to_string(),
                    model: "default".to_string(),
                },
                qdrant_url: "http://qdrant".to_string(),
                collection: "docs".to_string(),
                state_path: DEFAULT_STATE_PATH.to_string(),
                create_collection_if_missing: false,
                collection_vector_size: None,
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
            }))
        );
    }

    #[test]
    fn parse_args_defaults_state_path() {
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
        let ParsedCommand::Run(cfg) = cfg else {
            panic!("expected run config");
        };
        assert_eq!(cfg.state_path, DEFAULT_STATE_PATH);
    }

    #[test]
    fn parse_args_accepts_state_path_inline_value() {
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
            "--state-path=.state/ragloom.ndjson".to_string(),
        ];

        let cfg = parse_args(&args).expect("config");
        let ParsedCommand::Run(cfg) = cfg else {
            panic!("expected run config");
        };
        assert_eq!(cfg.state_path, ".state/ragloom.ndjson");
    }

    #[test]
    fn parse_args_returns_version_command_for_long_flag() {
        let args = vec!["ragloom".to_string(), "--version".to_string()];

        let cmd = parse_args(&args).expect("version command");
        assert_eq!(cmd, ParsedCommand::Version);
    }

    #[test]
    fn parse_args_returns_version_command_for_short_flag() {
        let args = vec!["ragloom".to_string(), "-V".to_string()];

        let cmd = parse_args(&args).expect("version command");
        assert_eq!(cmd, ParsedCommand::Version);
    }

    #[test]
    fn parse_args_returns_version_command_before_required_flag_validation() {
        let args = vec![
            "ragloom".to_string(),
            "--version".to_string(),
            "--dir".to_string(),
            "/tmp/docs".to_string(),
        ];

        let cmd = parse_args(&args).expect("version command");
        assert_eq!(cmd, ParsedCommand::Version);
    }

    #[test]
    fn parse_args_returns_help_command_for_help_flag() {
        let args = vec!["ragloom".to_string(), "--help".to_string()];

        let cmd = parse_args(&args).expect("help command");
        assert_eq!(cmd, ParsedCommand::Help);
    }

    #[test]
    fn parse_args_supports_inline_version_flag_before_required_validation() {
        let args = vec![
            "ragloom".to_string(),
            "--version".to_string(),
            "--qdrant-url=http://qdrant".to_string(),
        ];

        let cmd = parse_args(&args).expect("version command");
        assert_eq!(cmd, ParsedCommand::Version);
    }

    #[test]
    fn parse_args_defaults_bootstrap_flags_to_disabled_and_none() {
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
        let ParsedCommand::Run(cfg) = cfg else {
            panic!("expected run config");
        };
        assert!(!cfg.create_collection_if_missing);
        assert_eq!(cfg.collection_vector_size, None);
    }

    #[test]
    fn parse_args_accepts_collection_bootstrap_flags() {
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
            "--create-collection-if-missing".to_string(),
            "--collection-vector-size".to_string(),
            "768".to_string(),
        ];

        let cfg = parse_args(&args).expect("config");
        let ParsedCommand::Run(cfg) = cfg else {
            panic!("expected run config");
        };
        assert!(cfg.create_collection_if_missing);
        assert_eq!(cfg.collection_vector_size, Some(768));
    }

    #[test]
    fn parse_args_rejects_inline_value_for_create_collection_if_missing() {
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
            "--create-collection-if-missing=false".to_string(),
        ];

        let err = parse_args(&args).expect_err("expected invalid boolean flag usage");
        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("--create-collection-if-missing does not accept a value")
        );
    }

    #[test]
    fn parse_args_accepts_collection_vector_size_inline_value() {
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
            "--collection-vector-size=768".to_string(),
        ];

        let cfg = parse_args(&args).expect("config");
        let ParsedCommand::Run(cfg) = cfg else {
            panic!("expected run config");
        };
        assert_eq!(cfg.collection_vector_size, Some(768));
    }

    #[test]
    fn parse_args_rejects_missing_collection_vector_size_value() {
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
            "--collection-vector-size".to_string(),
        ];

        let err = parse_args(&args).expect_err("expected missing vector size value");
        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("missing required value: --collection-vector-size")
        );
    }

    #[test]
    fn parse_args_rejects_invalid_collection_vector_size() {
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
            "--collection-vector-size".to_string(),
            "0".to_string(),
        ];

        let err = parse_args(&args).expect_err("expected invalid vector size");
        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("--collection-vector-size must be positive")
        );
    }

    #[test]
    fn resolve_collection_vector_size_prefers_explicit_override() {
        let cfg = RunConfig {
            dir: "/tmp/docs".to_string(),
            embed_backend: EmbedBackend::OpenAi {
                endpoint: "https://api.openai.com/v1/embeddings".to_string(),
                api_key: "test-key".to_string(),
                model: "text-embedding-3-small".to_string(),
            },
            qdrant_url: "http://qdrant".to_string(),
            collection: "docs".to_string(),
            state_path: DEFAULT_STATE_PATH.to_string(),
            create_collection_if_missing: true,
            collection_vector_size: Some(768),
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
        };

        let size = resolve_collection_vector_size(&cfg).expect("vector size");
        assert_eq!(size, 768);
    }

    #[test]
    fn resolve_collection_vector_size_infers_known_openai_model_size() {
        let cfg = RunConfig {
            dir: "/tmp/docs".to_string(),
            embed_backend: EmbedBackend::OpenAi {
                endpoint: "https://api.openai.com/v1/embeddings".to_string(),
                api_key: "test-key".to_string(),
                model: "text-embedding-3-small".to_string(),
            },
            qdrant_url: "http://qdrant".to_string(),
            collection: "docs".to_string(),
            state_path: DEFAULT_STATE_PATH.to_string(),
            create_collection_if_missing: true,
            collection_vector_size: None,
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
        };

        let size = resolve_collection_vector_size(&cfg).expect("vector size");
        assert_eq!(size, 1536);
    }

    #[test]
    fn resolve_collection_vector_size_rejects_http_backend_without_override() {
        let cfg = RunConfig {
            dir: "/tmp/docs".to_string(),
            embed_backend: EmbedBackend::Http {
                url: "http://embed".to_string(),
                model: "default".to_string(),
            },
            qdrant_url: "http://qdrant".to_string(),
            collection: "docs".to_string(),
            state_path: DEFAULT_STATE_PATH.to_string(),
            create_collection_if_missing: true,
            collection_vector_size: None,
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
        };

        let err = resolve_collection_vector_size(&cfg).expect_err("expected config error");
        assert_eq!(err.kind, RagloomErrorKind::Config);
        assert!(
            err.to_string()
                .contains("requires --collection-vector-size")
        );
    }

    #[cfg_attr(miri, ignore = "Miri does not support TCP socket tests")]
    #[tokio::test]
    async fn prepare_startup_skips_bootstrap_when_embedding_client_construction_fails() {
        let (base_url, server) = spawn_qdrant_request_counter_server();
        let cfg = RunConfig {
            dir: ".".to_string(),
            embed_backend: EmbedBackend::OpenAi {
                endpoint: "https://api.openai.com/v1/embeddings".to_string(),
                api_key: "bad\nkey".to_string(),
                model: "text-embedding-3-small".to_string(),
            },
            qdrant_url: base_url,
            collection: "docs".to_string(),
            state_path: DEFAULT_STATE_PATH.to_string(),
            create_collection_if_missing: true,
            collection_vector_size: None,
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
        };

        let err = match prepare_startup(&cfg).await {
            Ok(_) => panic!("expected embedding client construction error"),
            Err(err) => err,
        };
        server.stop.send(()).expect("stop");
        let request_count = server.handle.join().expect("join");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("failed to build OpenAI embedding client")
        );
        assert_eq!(request_count, 0);
    }

    #[cfg_attr(miri, ignore = "Miri does not support TCP socket tests")]
    #[tokio::test]
    async fn bootstrap_collection_if_needed_infers_known_openai_model_size_in_startup_path() {
        let (base_url, server) = spawn_qdrant_bootstrap_server(vec![
            TestResponse {
                status: 404,
                reason: "Not Found",
                body: r#"{"status":"not_found"}"#,
            },
            TestResponse {
                status: 200,
                reason: "OK",
                body: r#"{"status":"ok"}"#,
            },
        ]);

        let cfg = RunConfig {
            dir: "/tmp/docs".to_string(),
            embed_backend: EmbedBackend::OpenAi {
                endpoint: "https://api.openai.com/v1/embeddings".to_string(),
                api_key: "test-key".to_string(),
                model: "text-embedding-3-small".to_string(),
            },
            qdrant_url: base_url.clone(),
            collection: "docs".to_string(),
            state_path: DEFAULT_STATE_PATH.to_string(),
            create_collection_if_missing: true,
            collection_vector_size: None,
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
        };

        let sink = QdrantSink::new(QdrantConfig {
            base_url,
            collection: "docs".to_string(),
            timeout: Duration::from_secs(5),
        })
        .expect("sink");

        bootstrap_collection_if_needed(&cfg, &sink)
            .await
            .expect("bootstrap");

        let requests = server.join().expect("join");
        assert_eq!(requests.len(), 2);
        assert!(requests[1].starts_with("PUT /collections/docs HTTP/1.1"));
        assert!(requests[1].contains(r#""size":1536"#));
    }

    #[cfg_attr(miri, ignore = "Miri does not support TCP socket tests")]
    #[tokio::test]
    async fn bootstrap_collection_if_needed_uses_explicit_http_vector_size_in_startup_path() {
        let (base_url, server) = spawn_qdrant_bootstrap_server(vec![
            TestResponse {
                status: 404,
                reason: "Not Found",
                body: r#"{"status":"not_found"}"#,
            },
            TestResponse {
                status: 200,
                reason: "OK",
                body: r#"{"status":"ok"}"#,
            },
        ]);

        let cfg = RunConfig {
            dir: "/tmp/docs".to_string(),
            embed_backend: EmbedBackend::Http {
                url: "http://embed".to_string(),
                model: "default".to_string(),
            },
            qdrant_url: base_url.clone(),
            collection: "docs".to_string(),
            state_path: DEFAULT_STATE_PATH.to_string(),
            create_collection_if_missing: true,
            collection_vector_size: Some(768),
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
        };

        let sink = QdrantSink::new(QdrantConfig {
            base_url,
            collection: "docs".to_string(),
            timeout: Duration::from_secs(5),
        })
        .expect("sink");

        bootstrap_collection_if_needed(&cfg, &sink)
            .await
            .expect("bootstrap");

        let requests = server.join().expect("join");
        assert_eq!(requests.len(), 2);
        assert!(requests[1].starts_with("PUT /collections/docs HTTP/1.1"));
        assert!(requests[1].contains(r#""size":768"#));
    }

    #[cfg_attr(miri, ignore = "Miri does not support TCP socket tests")]
    #[tokio::test]
    async fn bootstrap_collection_if_needed_surfaces_unknown_openai_model_with_config_context() {
        let cfg = RunConfig {
            dir: "/tmp/docs".to_string(),
            embed_backend: EmbedBackend::OpenAi {
                endpoint: "https://api.openai.com/v1/embeddings".to_string(),
                api_key: "test-key".to_string(),
                model: "text-embedding-unknown".to_string(),
            },
            qdrant_url: "http://127.0.0.1:1".to_string(),
            collection: "docs".to_string(),
            state_path: DEFAULT_STATE_PATH.to_string(),
            create_collection_if_missing: true,
            collection_vector_size: None,
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
        };

        let sink = QdrantSink::new(QdrantConfig {
            base_url: cfg.qdrant_url.clone(),
            collection: cfg.collection.clone(),
            timeout: Duration::from_secs(5),
        })
        .expect("sink");

        let err = bootstrap_collection_if_needed(&cfg, &sink)
            .await
            .expect_err("expected config error");

        assert_eq!(err.kind, RagloomErrorKind::Config);
        assert!(
            err.to_string()
                .contains("failed to bootstrap Qdrant collection")
        );
        let source = std::error::Error::source(&err).expect("source");
        assert!(
            source.to_string().contains(
                "unknown OpenAI model for collection vector size: text-embedding-unknown"
            )
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

    #[test]
    fn parse_args_loads_required_values_from_yaml_config() {
        let mut file = NamedTempFile::new().expect("temp file");
        file.write_all(
            br#"
source:
  root: "/tmp/from-config"
embed:
  endpoint: "http://embed-from-config"
sink:
  qdrant_url: "http://qdrant-from-config"
  collection: "from-config"
state:
  path: ".state/from-config.ndjson"
"#,
        )
        .expect("write config");

        let args = vec![
            "ragloom".to_string(),
            "--config".to_string(),
            file.path().to_string_lossy().to_string(),
            "--embed-backend".to_string(),
            "http".to_string(),
        ];

        let cfg = parse_args(&args).expect("config");
        let ParsedCommand::Run(cfg) = cfg else {
            panic!("expected run config");
        };
        assert_eq!(cfg.dir, "/tmp/from-config");
        assert_eq!(cfg.qdrant_url, "http://qdrant-from-config");
        assert_eq!(cfg.collection, "from-config");
        assert_eq!(cfg.state_path, ".state/from-config.ndjson");
        assert_eq!(
            cfg.embed_backend,
            EmbedBackend::Http {
                url: "http://embed-from-config".to_string(),
                model: "default".to_string(),
            }
        );
    }

    #[test]
    fn parse_args_surfaces_yaml_validation_context() {
        let mut file = NamedTempFile::new().expect("temp file");
        file.write_all(
            br#"
source:
  root: ""
embed:
  endpoint: "http://embed"
sink:
  qdrant_url: "http://qdrant"
  collection: "docs"
"#,
        )
        .expect("write config");

        let args = vec![
            "ragloom".to_string(),
            "--config".to_string(),
            file.path().to_string_lossy().to_string(),
            "--embed-backend".to_string(),
            "http".to_string(),
        ];

        let err = parse_args(&args).expect_err("should fail validation");
        assert_eq!(err.kind, RagloomErrorKind::Config);
        assert!(err.to_string().contains("invalid config file"));
    }

    #[test]
    fn parse_args_surfaces_yaml_parse_context() {
        let mut file = NamedTempFile::new().expect("temp file");
        file.write_all(
            br#"
source:
  root: "/tmp/docs"
embed:
  endpoint "missing-colon"
sink:
  qdrant_url: "http://qdrant"
  collection: "docs"
"#,
        )
        .expect("write config");

        let args = vec![
            "ragloom".to_string(),
            "--config".to_string(),
            file.path().to_string_lossy().to_string(),
            "--embed-backend".to_string(),
            "http".to_string(),
        ];

        let err = parse_args(&args).expect_err("should fail parse");
        assert_eq!(err.kind, RagloomErrorKind::Config);
        assert!(err.to_string().contains("failed to parse config file"));
    }
}
