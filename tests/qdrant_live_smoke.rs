use std::time::Duration;

use ragloom::sink::qdrant::{QdrantConfig, QdrantSink};
use ragloom::sink::{DocumentIdentity, PointId, Sink, VectorPoint};

type SmokeResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[ignore = "requires RAGLOOM_QDRANT_URL pointing to a live Qdrant instance"]
async fn live_qdrant_bootstrap_upsert_and_delete() {
    let base_url =
        std::env::var("RAGLOOM_QDRANT_URL").expect("RAGLOOM_QDRANT_URL must be configured");
    let collection = format!("ragloom_rc_smoke_{}", std::process::id());
    let point_id = "7c5ac739-ecf5-4f6b-a92f-0f98e8919708";
    let doc_id = "live-smoke-doc";
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .no_proxy()
        .build()
        .expect("build verification client");
    let sink = QdrantSink::new(QdrantConfig {
        base_url: base_url.clone(),
        collection: collection.clone(),
        timeout: Duration::from_secs(10),
    })
    .expect("build Qdrant sink");

    let smoke_result =
        exercise_live_qdrant(&sink, &client, &base_url, &collection, point_id, doc_id).await;
    let cleanup_result = client
        .delete(format!("{base_url}/collections/{collection}"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status);

    if let Err(error) = cleanup_result {
        panic!("live Qdrant smoke cleanup failed after {smoke_result:?}: {error}");
    }
    smoke_result.expect("live Qdrant smoke failed");
}

async fn exercise_live_qdrant(
    sink: &QdrantSink,
    client: &reqwest::Client,
    base_url: &str,
    collection: &str,
    point_id: &str,
    doc_id: &str,
) -> SmokeResult {
    sink.ensure_collection_exists(4).await?;
    sink.upsert_points(vec![VectorPoint {
        id: PointId::parse(point_id)?,
        vector: vec![1.0, 0.0, 0.0, 0.0],
        payload: serde_json::json!({
            "canonical_path": "file:///live-smoke.md",
            "doc_id": doc_id,
            "chunk_index": 0,
            "total_chunks": 1,
            "strategy_fingerprint": "live-smoke-v1"
        }),
    }])
    .await?;

    let point_url =
        format!("{base_url}/collections/{collection}/points/{point_id}?with_payload=true");
    let inserted: serde_json::Value = client
        .get(&point_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if inserted["result"]["payload"]["doc_id"] != doc_id {
        return Err(std::io::Error::other(format!(
            "inserted point has unexpected payload: {inserted}"
        ))
        .into());
    }

    sink.delete_document_points(DocumentIdentity {
        canonical_path: "file:///live-smoke.md".to_string(),
        doc_id: doc_id.to_string(),
    })
    .await?;

    let deleted_response = client.get(&point_url).send().await?;
    if deleted_response.status() != reqwest::StatusCode::NOT_FOUND {
        let deleted: serde_json::Value = deleted_response.error_for_status()?.json().await?;
        if !deleted["result"].is_null() {
            return Err(std::io::Error::other(format!(
                "document deletion did not remove the point: {deleted}"
            ))
            .into());
        }
    }

    Ok(())
}
