use std::sync::Arc;

use async_trait::async_trait;

use crate::doc::{DocumentLoader, LoadedDocument, extract_document_text};
use crate::s3::{S3Client, parse_s3_uri};
use crate::{RagloomError, RagloomErrorKind};

#[derive(Debug, Clone)]
pub struct S3Utf8Loader {
    client: Arc<dyn S3Client>,
}

impl S3Utf8Loader {
    pub fn new(client: Arc<dyn S3Client>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl DocumentLoader for S3Utf8Loader {
    async fn load(&self, path: &str) -> Result<LoadedDocument, RagloomError> {
        let location = parse_s3_uri(path)?;
        if location.bucket != self.client.bucket_name() {
            return Err(
                RagloomError::from_kind(RagloomErrorKind::InvalidInput).with_context(format!(
                    "S3 canonical path bucket {} does not match configured bucket {}",
                    location.bucket,
                    self.client.bucket_name()
                )),
            );
        }
        let client = Arc::clone(&self.client);
        let key = location.key.to_string();
        let bytes = tokio::task::spawn_blocking(move || client.get_object(&key))
            .await
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::Internal, e)
                    .with_context("failed to join S3 object read task")
            })?
            .map_err(|e| {
                RagloomError::new(e.kind, e).with_context("failed to load document bytes")
            })?;

        let text = extract_document_text(path, bytes)?;

        Ok(LoadedDocument { text })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    use crate::s3::S3ObjectMeta;
    use zip::write::SimpleFileOptions as FileOptions;

    fn minimal_pdf_bytes(stream: &str) -> Vec<u8> {
        let objects = [
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_string(),
            "2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n".to_string(),
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n".to_string(),
            format!(
                "4 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n",
                stream.len()
            ),
            "5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n"
                .to_string(),
        ];

        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = vec![0usize];
        for object in &objects {
            offsets.push(pdf.len());
            pdf.push_str(object);
        }

        let xref_offset = pdf.len();
        pdf.push_str("xref\n0 6\n");
        pdf.push_str("0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str("trailer\n<< /Root 1 0 R /Size 6 >>\n");
        pdf.push_str(&format!("startxref\n{xref_offset}\n%%EOF\n"));
        pdf.into_bytes()
    }

    fn minimal_docx_bytes(paragraphs: &[&str]) -> Vec<u8> {
        let body = paragraphs
            .iter()
            .map(|paragraph| format!("<w:p><w:r><w:t>{paragraph}</w:t></w:r></w:p>"))
            .collect::<String>();
        let document_xml = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
                r#"<w:body>{}</w:body>"#,
                r#"</w:document>"#
            ),
            format!("{body}<w:sectPr/>")
        );

        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = FileOptions::default();
        zip.start_file("[Content_Types].xml", options)
            .expect("start content types");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
        )
        .expect("write content types");
        zip.add_directory("_rels/", options)
            .expect("add rels directory");
        zip.start_file("_rels/.rels", options).expect("start rels");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
        )
        .expect("write rels");
        zip.add_directory("word/", options)
            .expect("add word directory");
        zip.start_file("word/document.xml", options)
            .expect("start document xml");
        zip.write_all(document_xml.as_bytes())
            .expect("write document xml");
        zip.finish().expect("finish docx").into_inner()
    }

    #[derive(Debug)]
    struct FakeS3Client {
        bytes: Vec<u8>,
        fail_get_object: bool,
    }

    impl S3Client for FakeS3Client {
        fn bucket_name(&self) -> &str {
            "docs-bucket"
        }

        fn list_objects(&self, _prefix: Option<&str>) -> Result<Vec<S3ObjectMeta>, RagloomError> {
            unreachable!("source only")
        }

        fn get_object(&self, _key: &str) -> Result<Vec<u8>, RagloomError> {
            if self.fail_get_object {
                return Err(RagloomError::from_kind(RagloomErrorKind::Io)
                    .with_context("failed to get S3 object s3://docs-bucket/kb/hello.txt"));
            }

            Ok(self.bytes.clone())
        }
    }

    #[tokio::test]
    async fn rejects_non_s3_canonical_path() {
        let loader = S3Utf8Loader::new(Arc::new(FakeS3Client {
            bytes: b"hello".to_vec(),
            fail_get_object: false,
        }));

        let err = loader
            .load("/tmp/hello.txt")
            .await
            .expect_err("expected invalid input");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn reads_utf8_text_from_s3() {
        let loader = S3Utf8Loader::new(Arc::new(FakeS3Client {
            bytes: b"hello from s3".to_vec(),
            fail_get_object: false,
        }));

        let text = loader
            .load("s3://docs-bucket/kb/hello.txt")
            .await
            .expect("load text");

        assert_eq!(text.text, "hello from s3");
    }

    #[tokio::test]
    async fn rejects_bucket_mismatch() {
        let loader = S3Utf8Loader::new(Arc::new(FakeS3Client {
            bytes: b"hello from s3".to_vec(),
            fail_get_object: false,
        }));

        let err = loader
            .load("s3://other-bucket/kb/hello.txt")
            .await
            .expect_err("expected invalid input");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(err.to_string().contains("does not match configured bucket"));
    }

    #[tokio::test]
    async fn surfaces_utf8_extraction_context_for_s3_bytes() {
        let loader = S3Utf8Loader::new(Arc::new(FakeS3Client {
            bytes: vec![0xff, 0xfe, 0xfd],
            fail_get_object: false,
        }));

        let err = loader
            .load("s3://docs-bucket/kb/hello.txt")
            .await
            .expect_err("expected invalid utf-8");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("failed to extract UTF-8 text from document bytes")
        );
    }

    #[tokio::test]
    async fn surfaces_load_context_for_s3_read_failures() {
        let loader = S3Utf8Loader::new(Arc::new(FakeS3Client {
            bytes: Vec::new(),
            fail_get_object: true,
        }));

        let err = loader
            .load("s3://docs-bucket/kb/hello.txt")
            .await
            .expect_err("expected read failure");

        assert_eq!(err.kind, RagloomErrorKind::Io);
        assert!(err.to_string().contains("failed to load document bytes"));
    }

    #[tokio::test]
    async fn extracts_pdf_text_from_s3_bytes() {
        let loader = S3Utf8Loader::new(Arc::new(FakeS3Client {
            bytes: minimal_pdf_bytes("BT\n/F1 18 Tf\n50 100 Td\n(Hello from S3 PDF) Tj\nET\n"),
            fail_get_object: false,
        }));

        let loaded = loader
            .load("s3://docs-bucket/kb/hello.pdf")
            .await
            .expect("load pdf");

        assert_eq!(loaded.text, "\n\nHello from S3 PDF");
    }

    #[tokio::test]
    async fn extracts_docx_text_from_s3_bytes() {
        let loader = S3Utf8Loader::new(Arc::new(FakeS3Client {
            bytes: minimal_docx_bytes(&["Hello from S3 DOCX"]),
            fail_get_object: false,
        }));

        let loaded = loader
            .load("s3://docs-bucket/kb/hello.docx")
            .await
            .expect("load docx");

        assert_eq!(loaded.text, "Hello from S3 DOCX\n");
    }

    #[tokio::test]
    async fn rejects_malformed_docx_from_s3_with_context() {
        let loader = S3Utf8Loader::new(Arc::new(FakeS3Client {
            bytes: b"not a zip archive".to_vec(),
            fail_get_object: false,
        }));

        let err = loader
            .load("s3://docs-bucket/kb/broken.docx")
            .await
            .expect_err("expected malformed docx");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("failed to extract DOCX text from document bytes")
        );
    }
}
