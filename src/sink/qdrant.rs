//! Qdrant sink implementation.
//!
//! # Why
//! Qdrant supports idempotent upserts keyed by point id. Ragloom leverages this
//! to achieve near exactly-once effects even with at-least-once execution.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{RagloomError, RagloomErrorKind};
use crate::sink::{Sink, VectorPoint};

/// Qdrant HTTP client configuration.
///
/// # Why
/// Configuration is explicit so operators can tune batching and timeouts without
/// code changes.
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    pub base_url: String,
    pub collection: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct QdrantSink {
    config: QdrantConfig,
    client: reqwest::Client,
}

impl QdrantSink {
    pub fn new(config: QdrantConfig) -> Result<Self, RagloomError> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::Sink, e)
                    .with_context("failed to build Qdrant HTTP client")
            })?;

        Ok(Self { config, client })
    }

    fn upsert_url(&self) -> String {
        format!(
            "{}/collections/{}/points?wait=true",
            self.config.base_url.trim_end_matches('/'),
            self.config.collection
        )
    }
}

#[async_trait::async_trait]
impl Sink for QdrantSink {
    async fn upsert_points(&self, points: Vec<VectorPoint>) -> Result<(), RagloomError> {
        let request = UpsertRequest {
            points: points
                .into_iter()
                .map(|p| QdrantPoint {
                    id: p.id.into_string(),
                    vector: p.vector,
                    payload: p.payload,
                })
                .collect(),
        };

        let response = self
            .client
            .put(self.upsert_url())
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::Sink, e).with_context(format!(
                    "qdrant upsert request failed (url={})",
                    self.upsert_url()
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());
            return Err(RagloomError::from_kind(RagloomErrorKind::Sink).with_context(format!(
                "qdrant upsert returned non-success status (url={}, status={}, body={})",
                self.upsert_url(),
                status,
                body
            )));
        }

        let decoded: QdrantResponse = response.json().await.map_err(|e| {
            RagloomError::new(RagloomErrorKind::Sink, e).with_context(format!(
                "failed to decode qdrant response (url={})",
                self.upsert_url()
            ))
        })?;

        if decoded.status != "ok" {
            return Err(
                RagloomError::from_kind(RagloomErrorKind::Sink).with_context(format!(
                    "qdrant returned non-ok status in body (url={}, status={})",
                    self.upsert_url(),
                    decoded.status
                )),
            );
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct UpsertRequest {
    points: Vec<QdrantPoint>,
}

#[derive(Debug, Serialize)]
struct QdrantPoint {
    id: String,
    vector: Vec<f32>,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct QdrantResponse {
    status: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::{Read, Write};
    use std::net::TcpListener;

    use crate::sink::{PointId, VectorPoint};

    fn spawn_test_server(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
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

    fn test_point() -> VectorPoint {
        VectorPoint {
            id: PointId::parse("deadbeef").expect("valid id"),
            vector: vec![1.0, 2.0, 3.0],
            payload: serde_json::json!({"k":"v"}),
        }
    }

    #[tokio::test]
    async fn non_success_status_is_reported_as_error() {
        let base_url = spawn_test_server(500, r#"{"status":"error"}"#);

        let sink = QdrantSink::new(QdrantConfig {
            base_url,
            collection: "docs".to_string(),
            timeout: Duration::from_secs(1),
        })
        .expect("sink");

        let err = sink
            .upsert_points(vec![test_point()])
            .await
            .expect_err("should fail");

        assert_eq!(err.kind, RagloomErrorKind::Sink);
        assert!(err.to_string().contains("non-success"));
    }

    #[tokio::test]
    async fn ok_body_is_accepted() {
        let base_url = spawn_test_server(200, r#"{"status":"ok"}"#);

        let sink = QdrantSink::new(QdrantConfig {
            base_url,
            collection: "docs".to_string(),
            timeout: Duration::from_secs(1),
        })
        .expect("sink");

        sink.upsert_points(vec![test_point()]).await.expect("ok");
    }

    #[tokio::test]
    async fn non_ok_body_is_reported_as_error() {
        let base_url = spawn_test_server(200, r#"{"status":"error"}"#);

        let sink = QdrantSink::new(QdrantConfig {
            base_url,
            collection: "docs".to_string(),
            timeout: Duration::from_secs(1),
        })
        .expect("sink");

        let err = sink
            .upsert_points(vec![test_point()])
            .await
            .expect_err("should fail");

        assert_eq!(err.kind, RagloomErrorKind::Sink);
        assert!(err.to_string().contains("non-ok"));
    }
}
