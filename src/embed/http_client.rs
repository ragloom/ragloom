//! HTTP embedding client.
//!
//! # Why
//! Ragloom is a single-binary daemon. Calling an external HTTP embedding service
//! keeps the binary lightweight and operationally simple.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::embed::EmbeddingProvider;
use crate::error::{RagloomError, RagloomErrorKind};

/// HTTP embedding client configuration.
///
/// # Why
/// These knobs define the operational contract with the upstream embedding
/// service (timeouts, model selection).
#[derive(Debug, Clone)]
pub struct HttpEmbeddingConfig {
    pub endpoint: String,
    pub model: String,
    pub timeout: Duration,
}

/// Embedding provider backed by an HTTP API.
///
/// # Why
/// We keep the request/response types explicit so failures can include
/// high-quality context for operators.
#[derive(Debug, Clone)]
pub struct HttpEmbeddingClient {
    config: HttpEmbeddingConfig,
    client: reqwest::Client,
}

impl HttpEmbeddingClient {
    pub fn new(config: HttpEmbeddingConfig) -> Result<Self, RagloomError> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::Embed, e)
                    .with_context("failed to build HTTP client")
            })?;

        Ok(Self { config, client })
    }
}

#[derive(Debug, Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

#[async_trait::async_trait]
impl EmbeddingProvider for HttpEmbeddingClient {
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, RagloomError> {
        let request = EmbedRequest {
            model: &self.config.model,
            input: inputs,
        };

        let response = self
            .client
            .post(&self.config.endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::Embed, e).with_context(format!(
                    "embedding request failed (endpoint={})",
                    self.config.endpoint
                ))
            })?;

        if !response.status().is_success() {
            return Err(
                RagloomError::from_kind(RagloomErrorKind::Embed).with_context(format!(
                    "embedding request returned non-success status (endpoint={}, status={})",
                    self.config.endpoint,
                    response.status()
                )),
            );
        }

        let decoded: EmbedResponse = response.json().await.map_err(|e| {
            RagloomError::new(RagloomErrorKind::Embed, e).with_context(format!(
                "failed to decode embedding response (endpoint={})",
                self.config.endpoint
            ))
        })?;

        Ok(decoded.embeddings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::{Read, Write};
    use std::net::TcpListener;

    use crate::embed::EmbeddingProvider;

    fn spawn_test_server(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);

                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn non_success_status_is_reported_as_error() {
        let endpoint = spawn_test_server(500, r#"{"error":"boom"}"#);

        let client = HttpEmbeddingClient::new(HttpEmbeddingConfig {
            endpoint,
            model: "test".to_string(),
            timeout: Duration::from_secs(1),
        })
        .expect("client");

        let err = client
            .embed(&["hello".to_string()])
            .await
            .expect_err("should fail");

        assert_eq!(err.kind, RagloomErrorKind::Embed);
        assert!(err.to_string().contains("non-success"));
    }

    #[tokio::test]
    async fn decodes_embeddings_from_success_response() {
        let endpoint = spawn_test_server(200, r#"{"embeddings":[[1.0,2.0],[3.0,4.0]]}"#);

        let client = HttpEmbeddingClient::new(HttpEmbeddingConfig {
            endpoint,
            model: "test".to_string(),
            timeout: Duration::from_secs(1),
        })
        .expect("client");

        let vectors = client
            .embed(&["a".to_string(), "b".to_string()])
            .await
            .expect("ok");
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0], vec![1.0, 2.0]);
        assert_eq!(vectors[1], vec![3.0, 4.0]);
    }
}
