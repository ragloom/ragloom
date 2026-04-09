use crate::error::{RagloomError, RagloomErrorKind};
use tracing_subscriber::EnvFilter;

/// Log output format.
///
/// # Why
/// Operators typically want readable logs locally, while production pipelines
/// often require structured JSON for ingestion (e.g. ELK).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum LogFormat {
    Pretty,
    Json,
}

/// Environment inputs (extracted for testability).
#[derive(Debug, Clone, Default)]
pub struct EnvConfig {
    pub log_format: Option<String>,
    pub log_filter: Option<String>,
}

/// Observability configuration.
///
/// # Why
/// This keeps subscriber initialization deterministic and avoids hidden global
/// state. The binary decides *how* logs are emitted; the library only emits
/// events/spans.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ObservabilityConfig {
    pub format: LogFormat,
    pub filter_directives: String,
}

impl ObservabilityConfig {
    pub fn from_env(env: EnvConfig) -> Self {
        Self::try_from_env(env).unwrap_or_else(|_| Self {
            format: LogFormat::Pretty,
            filter_directives: "info".to_string(),
        })
    }

    pub fn try_from_env(env: EnvConfig) -> Result<Self, RagloomError> {
        let format = match env.log_format.as_deref() {
            None => LogFormat::Pretty,
            Some("pretty") => LogFormat::Pretty,
            Some("json") => LogFormat::Json,
            Some(other) => {
                return Err(
                    RagloomError::from_kind(RagloomErrorKind::Config).with_context(format!(
                        "invalid RAGLOOM_LOG_FORMAT: {other} (expected: pretty|json)"
                    )),
                );
            }
        };

        let filter_directives = env
            .log_filter
            .unwrap_or_else(|| "info".to_string())
            .trim()
            .to_string();

        if filter_directives.is_empty() {
            return Err(RagloomError::from_kind(RagloomErrorKind::Config)
                .with_context("RAGLOOM_LOG is empty"));
        }

        Ok(Self {
            format,
            filter_directives,
        })
    }
}

/// Loads observability config from process environment.
///
/// # Why
/// Keeping the parsing logic in one place makes ops behavior predictable.
pub fn load_from_process_env() -> Result<ObservabilityConfig, RagloomError> {
    let env = EnvConfig {
        log_format: std::env::var("RAGLOOM_LOG_FORMAT").ok(),
        log_filter: std::env::var("RAGLOOM_LOG").ok(),
    };
    ObservabilityConfig::try_from_env(env)
}

/// Builds a tracing subscriber for this process.
///
/// # Why
/// The binary should configure formatting and filtering in a single place.
/// Library code must remain pure and only emit `tracing` events.
pub fn init_subscriber(cfg: &ObservabilityConfig) -> Result<tracing::Dispatch, RagloomError> {
    init_subscriber_with_writer(cfg, std::io::stderr)
}

/// Builds a tracing subscriber using a custom writer.
///
/// # Why
/// Tests and embedding environments often need to capture log output without
/// touching global process state.
pub fn init_subscriber_with_writer<W>(
    cfg: &ObservabilityConfig,
    make_writer: W,
) -> Result<tracing::Dispatch, RagloomError>
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    let filter = EnvFilter::try_new(cfg.filter_directives.clone()).map_err(|e| {
        RagloomError::new(RagloomErrorKind::Config, e)
            .with_context("invalid RAGLOOM_LOG filter directives")
    })?;

    let subscriber: tracing::Dispatch = match cfg.format {
        LogFormat::Pretty => tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(make_writer)
                .with_target(true)
                .with_level(true)
                .pretty()
                .finish(),
        ),
        LogFormat::Json => tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(make_writer)
                .with_target(true)
                .with_level(true)
                .json()
                .finish(),
        ),
    };

    Ok(subscriber)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_format_defaults_to_pretty() {
        let env = EnvConfig {
            log_format: None,
            log_filter: None,
        };
        let cfg = ObservabilityConfig::from_env(env);
        assert_eq!(cfg.format, LogFormat::Pretty);
    }

    #[test]
    fn log_filter_defaults_to_info() {
        let env = EnvConfig {
            log_format: None,
            log_filter: None,
        };
        let cfg = ObservabilityConfig::from_env(env);
        assert_eq!(cfg.filter_directives, "info".to_string());
    }

    #[test]
    fn accepts_json_format() {
        let env = EnvConfig {
            log_format: Some("json".to_string()),
            log_filter: None,
        };
        let cfg = ObservabilityConfig::from_env(env);
        assert_eq!(cfg.format, LogFormat::Json);
    }

    #[test]
    fn rejects_unknown_format() {
        let env = EnvConfig {
            log_format: Some("wat".to_string()),
            log_filter: None,
        };
        let err = ObservabilityConfig::try_from_env(env).expect_err("expected error");
        assert_eq!(err.kind, crate::error::RagloomErrorKind::Config);
        assert!(err.to_string().contains("RAGLOOM_LOG_FORMAT"));
    }
}
