use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use ragloom::sink::qdrant::{QdrantConfig, QdrantSink};
use ragloom::sink::{PointId, Sink, VectorPoint};

fn spawn_test_server(status: u16, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);

            let response = format!(
                "HTTP/1.1 {status} Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
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
async fn qdrant_non_success_status_includes_response_body_in_error_message() {
    let base_url = spawn_test_server(400, r#"{"status":"error","message":"wrong vector size"}"#);

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

    let msg = err.to_string();
    assert!(msg.contains("status=400"), "expected status: {msg}");
    assert!(msg.contains("wrong vector size"), "expected body: {msg}");
}
