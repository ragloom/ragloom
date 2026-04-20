use ragloom::pipeline::runtime::PipelineExecutor;
use ragloom::sink::VectorPoint;

#[tokio::test]
async fn points_payload_includes_chunk_metadata_and_optional_text() {
    #[derive(Debug, Clone, Default)]
    struct StubDocumentLoader;

    #[async_trait::async_trait]
    impl ragloom::doc::DocumentLoader for StubDocumentLoader {
        async fn load_utf8(&self, _path: &str) -> Result<String, ragloom::error::RagloomError> {
            Ok("hello from stub loader".to_string())
        }
    }

    #[derive(Debug, Clone, Default)]
    struct StubEmbeddingProvider;

    #[async_trait::async_trait]
    impl ragloom::embed::EmbeddingProvider for StubEmbeddingProvider {
        async fn embed(
            &self,
            inputs: &[String],
        ) -> Result<Vec<Vec<f32>>, ragloom::error::RagloomError> {
            Ok(inputs.iter().map(|_| vec![1.0_f32, 2.0_f32]).collect())
        }
    }

    #[derive(Debug, Clone, Default)]
    struct RecordingSink {
        points: std::sync::Arc<tokio::sync::Mutex<Vec<VectorPoint>>>,
    }

    #[async_trait::async_trait]
    impl ragloom::sink::Sink for RecordingSink {
        async fn upsert_points(
            &self,
            points: Vec<VectorPoint>,
        ) -> Result<(), ragloom::error::RagloomError> {
            self.points.lock().await.extend(points);
            Ok(())
        }
    }

    let embedding = std::sync::Arc::new(StubEmbeddingProvider);
    let sink = std::sync::Arc::new(RecordingSink::default());
    let loader = std::sync::Arc::new(StubDocumentLoader);

    let sink_dyn: std::sync::Arc<dyn ragloom::sink::Sink + Send + Sync> = sink.clone();
    let executor = PipelineExecutor::new(embedding, sink_dyn, loader);

    let exec = async {
        let fp = ragloom::ids::FileFingerprint {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
        };

        let points = executor
            .build_points_from_text(&fp, "para1\n\npara2")
            .await
            .expect("points");

        sink.points.lock().await.extend(points);
    };

    exec.await;

    let points = sink.points.lock().await.clone();
    assert!(!points.is_empty(), "expected points");

    // Payload should include file metadata + chunk metadata.
    let payload = points[0].payload.as_object().expect("payload object");
    assert!(payload.contains_key("canonical_path"));
    assert!(payload.contains_key("size_bytes"));
    assert!(payload.contains_key("mtime_unix_secs"));
    assert!(payload.contains_key("chunk_index"));

    assert!(payload.contains_key("chunk_start_byte"));
    assert!(payload.contains_key("chunk_end_byte"));
    assert!(payload.contains_key("chunk_char_len"));
    assert!(payload.contains_key("chunk_text_sha256"));

    assert!(payload.contains_key("chunk_text"));

    // canonical_path is normalized to a file:// URI.
    assert!(
        payload
            .get("canonical_path")
            .and_then(|v| v.as_str())
            .expect("canonical_path")
            .starts_with("file:///"),
        "expected file URI canonical_path"
    );

    // New fields for enterprise-grade filtering and orchestration.
    assert!(payload.contains_key("doc_id"));
    assert!(payload.contains_key("tenant_id"));
    assert!(payload.contains_key("file_extension"));
    assert!(payload.contains_key("total_chunks"));
    assert!(payload.contains_key("previous_chunk_id"));
    assert!(payload.contains_key("next_chunk_id"));

    // Task 9: strategy_fingerprint must be present and match the default pipeline
    // chunker (RecursiveChunker with recursive_config_chars_512()).
    assert!(payload.contains_key("strategy_fingerprint"));
    assert!(payload.get("strategy_fingerprint").is_some());
    assert_eq!(
        payload["strategy_fingerprint"],
        serde_json::Value::String(
            "recursive:v1|metric=chars|tokenizer=chars|max=512|min=0|overlap=0".to_string()
        ),
        "payload must carry the chunker strategy fingerprint"
    );
}
