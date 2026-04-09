use ragloom::observability::{EnvConfig, ObservabilityConfig, init_subscriber_with_writer};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct BufWriter(Arc<Mutex<Vec<u8>>>);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufWriter {
    type Writer = BufGuard;

    fn make_writer(&'a self) -> Self::Writer {
        BufGuard {
            inner: Arc::clone(&self.0),
        }
    }
}

struct BufGuard {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl std::io::Write for BufGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.lock().expect("lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn subscriber_writes_pretty_or_json_output() {
    let writer = BufWriter::default();

    let pretty_cfg = ObservabilityConfig::from_env(EnvConfig {
        log_format: Some("pretty".to_string()),
        log_filter: Some("info".to_string()),
    });

    let dispatch = init_subscriber_with_writer(&pretty_cfg, writer.clone()).expect("init");
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!(event.name = "ragloom.test", "hello");
    });

    let pretty_out = String::from_utf8(writer.0.lock().expect("lock").clone()).expect("utf8");
    assert!(pretty_out.contains("hello"));

    let writer = BufWriter::default();
    let json_cfg = ObservabilityConfig::from_env(EnvConfig {
        log_format: Some("json".to_string()),
        log_filter: Some("info".to_string()),
    });

    let dispatch = init_subscriber_with_writer(&json_cfg, writer.clone()).expect("init");
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!(event.name = "ragloom.test", "hello");
    });

    let json_out = String::from_utf8(writer.0.lock().expect("lock").clone()).expect("utf8");
    assert!(json_out.contains("\"hello\""));
}
