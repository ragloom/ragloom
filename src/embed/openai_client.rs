//! OpenAI embedding provider.
//!
//! # Why
//! We support a direct OpenAI backend as the default UX path while keeping the
//! pipeline core closed for modification. The provider boundary allows swapping
//! or adding vendors without changing runtime orchestration.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::embed::EmbeddingProvider;
use crate::error::{RagloomError, RagloomErrorKind};

/// OpenAI embedding client configuration.
///
/// # Why
/// Keeping endpoint/model/key explicit makes it easy to run against stubs in tests
/// and enables controlled overrides (e.g. compatible gateways) without touching
/// pipeline code.
#[derive(Debug, Clone)]
pub struct OpenAiEmbeddingConfig {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub timeout: Duration,
}

/// OpenAI embedding provider.
///
/// # Why
/// This type isolates OpenAI request/response formats and authorization details
/// behind `EmbeddingProvider`.
pub struct OpenAiEmbeddingClient {
    config: OpenAiEmbeddingConfig,
    client: reqwest::Client,
}

impl OpenAiEmbeddingClient {
    /// Builds a new OpenAI embedding client.
    ///
    /// # Why
    /// We build and own a `reqwest::Client` to share connection pools and apply
    /// consistent headers/timeouts across requests.
    pub fn new(config: OpenAiEmbeddingConfig) -> Result<Self, RagloomError> {
        let mut headers = HeaderMap::new();

        let auth = format!("Bearer {}", config.api_key);
        let auth_value = HeaderValue::from_str(&auth).map_err(|e| {
            RagloomError::new(RagloomErrorKind::InvalidInput, e)
                .with_context("invalid OpenAI api key for Authorization header")
        })?;

        headers.insert(AUTHORIZATION, auth_value);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let mut builder = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(config.timeout);
        if should_bypass_proxy(&config.endpoint) {
            builder = builder.no_proxy();
        }

        let client = builder.build().map_err(|e| {
            RagloomError::new(RagloomErrorKind::Embed, e)
                .with_context("failed to build OpenAI HTTP client")
        })?;

        Ok(Self { config, client })
    }
}

fn should_bypass_proxy(endpoint: &str) -> bool {
    reqwest::Url::parse(endpoint)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
}

#[derive(Debug, Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Debug, Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbeddingClient {
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, RagloomError> {
        let req = EmbedRequest {
            model: &self.config.model,
            input: inputs,
        };

        let response = self
            .client
            .post(&self.config.endpoint)
            .json(&req)
            .send()
            .await
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::Embed, e).with_context(format!(
                    "openai embedding request failed (model={}, endpoint={})",
                    self.config.model, self.config.endpoint
                ))
            })?;

        if !response.status().is_success() {
            return Err(RagloomError::from_kind(RagloomErrorKind::Embed).with_context(format!(
                "openai embedding request returned non-success status (model={}, endpoint={}, status={})",
                self.config.model,
                self.config.endpoint,
                response.status()
            )));
        }

        let decoded: EmbedResponse = response.json().await.map_err(|e| {
            RagloomError::new(RagloomErrorKind::Embed, e).with_context(format!(
                "failed to decode openai embedding response (model={}, endpoint={})",
                self.config.model, self.config.endpoint
            ))
        })?;

        Ok(decoded.data.into_iter().map(|d| d.embedding).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_test_server(status: u16, body: &'static str) -> String {
        use std::io::{Read, Write};
        use std::net::{Shutdown, TcpListener};

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");

        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);

            let response = format!(
                "HTTP/1.1 {} OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                status,
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
            stream.flush().expect("flush");
            stream.shutdown(Shutdown::Both).expect("shutdown");
        });

        format!("http://{}", addr)
    }

    #[test]
    fn bypasses_proxy_for_loopback_endpoints() {
        assert!(should_bypass_proxy("http://127.0.0.1:6333"));
        assert!(should_bypass_proxy("http://localhost:8080/v1/embeddings"));
        assert!(!should_bypass_proxy("https://api.openai.com/v1/embeddings"));
    }

    #[cfg_attr(miri, ignore = "Miri does not support TCP socket tests")]
    #[tokio::test]
    async fn openai_client_decodes_embeddings_from_data_array() {
        let url = spawn_test_server(200, r#"{ "data": [ { "embedding": [1.0, 2.0] } ] }"#);

        let client = OpenAiEmbeddingClient::new(OpenAiEmbeddingConfig {
            endpoint: url,
            api_key: "test-key".to_string(),
            model: "text-embedding-3-small".to_string(),
            timeout: Duration::from_secs(5),
        })
        .expect("client");

        let out = client.embed(&["hello".to_string()]).await.expect("embed");

        assert_eq!(out, vec![vec![1.0, 2.0]]);
    }
}
