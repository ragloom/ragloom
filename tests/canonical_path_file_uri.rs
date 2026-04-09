use ragloom::pipeline::runtime::PipelineExecutor;
use ragloom::sink::VectorPoint;

#[tokio::test]
async fn payload_canonical_path_is_file_uri() {
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

    #[derive(Debug, Clone, Default)]
    struct StubDocumentLoader;

    #[async_trait::async_trait]
    impl ragloom::doc::DocumentLoader for StubDocumentLoader {
        async fn load_utf8(&self, _path: &str) -> Result<String, ragloom::error::RagloomError> {
            Ok("hello".to_string())
        }
    }

    let embedding = std::sync::Arc::new(StubEmbeddingProvider);
    let sink = std::sync::Arc::new(RecordingSink::default());
    let sink_dyn: std::sync::Arc<dyn ragloom::sink::Sink + Send + Sync> = sink.clone();
    let loader = std::sync::Arc::new(StubDocumentLoader);

    let executor = PipelineExecutor::new(embedding, sink_dyn, loader);

    let fp = ragloom::ids::FileFingerprint {
        canonical_path: "D:/code\\repo\\a.txt".to_string(),
        size_bytes: 10,
        mtime_unix_secs: 100,
    };

    let points = executor
        .build_points_from_text(&fp, "hello")
        .await
        .expect("points");

    let payload = points[0].payload.as_object().expect("payload object");
    let got = payload
        .get("canonical_path")
        .and_then(|v| v.as_str())
        .expect("canonical_path string");

    assert!(got.starts_with("file:///"), "expected file URI: {got}");
    assert!(!got.contains('\\'), "uri must not contain \\: {got}");
}
