use std::io::{Read, Write};
use std::time::Duration;

use ragloom::sink::qdrant::{QdrantConfig, QdrantSink};
use ragloom::sink::{PointId, Sink, VectorPoint};

#[tokio::test]
async fn qdrant_sink_sends_uuid_point_ids() {
    // Arrange: a stub server that validates the outgoing JSON contains UUID point ids.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");

    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");

        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).expect("read");
        let req = String::from_utf8_lossy(&buf[..n]);

        // The request is raw HTTP; just assert the JSON body contains a canonical UUID.
        assert!(
            req.contains("\"id\":\"550e8400-e29b-41d4-a716-446655440000\""),
            "expected uuid id in request: {req}"
        );

        let body = r#"{"status":"ok"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write");
    });

    let sink = QdrantSink::new(QdrantConfig {
        base_url: format!("http://{addr}"),
        collection: "docs".to_string(),
        timeout: Duration::from_secs(1),
    })
    .expect("sink");

    let points = vec![VectorPoint {
        id: PointId::parse("550e8400-e29b-41d4-a716-446655440000").expect("id"),
        vector: vec![1.0, 2.0, 3.0],
        payload: serde_json::json!({"k":"v"}),
    }];

    // Act
    sink.upsert_points(points).await.expect("ok");

    handle.join().expect("server");
}
