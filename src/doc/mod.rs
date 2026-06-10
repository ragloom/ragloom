//! Document loading abstractions.

use std::path::Path;

use async_trait::async_trait;
use docx_lite::DocxError;
use pdf_extract::{Document as PdfDocument, Error as PdfError, OutputError as PdfOutputError};

use crate::{RagloomError, RagloomErrorKind};

pub mod s3;
pub use s3::S3Utf8Loader;

/// Loads document bytes/text from a backing store.
///
/// # Why
/// Ingestion pipelines should depend on a small abstraction rather than hard-coding
/// filesystem, HTTP, or object-store logic. This trait provides a stable surface
/// for the pipeline while enabling alternate loaders in tests or other runtimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedDocument {
    pub text: String,
}

#[async_trait]
pub trait DocumentLoader: Send + Sync {
    /// Loads a document and returns extracted text.
    async fn load(&self, path: &str) -> Result<LoadedDocument, RagloomError>;
}

pub(crate) fn extract_document_text(path: &str, bytes: Vec<u8>) -> Result<String, RagloomError> {
    if is_pdf_path(path) {
        return extract_pdf_text(&bytes);
    }
    if is_docx_path(path) {
        return extract_docx_text(&bytes);
    }

    String::from_utf8(bytes).map_err(|e| {
        RagloomError::new(RagloomErrorKind::InvalidInput, e)
            .with_context("failed to extract UTF-8 text from document bytes")
    })
}

fn is_pdf_path(path: &str) -> bool {
    has_extension(path, "pdf")
}

fn is_docx_path(path: &str) -> bool {
    has_extension(path, "docx")
}

fn has_extension(path: &str, extension: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

fn extract_pdf_text(bytes: &[u8]) -> Result<String, RagloomError> {
    let document = PdfDocument::load_mem(bytes).map_err(map_pdf_parse_error)?;
    if document.is_encrypted() {
        return Err(RagloomError::from_kind(RagloomErrorKind::InvalidInput)
            .with_context("encrypted PDFs are not supported for text extraction"));
    }

    pdf_extract::extract_text_from_mem(bytes).map_err(map_pdf_extract_error)
}

fn extract_docx_text(bytes: &[u8]) -> Result<String, RagloomError> {
    docx_lite::extract_text_from_bytes(bytes).map_err(map_docx_extract_error)
}

fn map_pdf_parse_error(error: PdfError) -> RagloomError {
    RagloomError::new(RagloomErrorKind::InvalidInput, error)
        .with_context("failed to parse PDF document bytes")
}

fn map_pdf_extract_error(error: PdfOutputError) -> RagloomError {
    let context = match &error {
        PdfOutputError::PdfError(PdfError::UnsupportedSecurityHandler(_)) => {
            "unsupported PDF security handler prevents text extraction"
        }
        _ => "failed to extract PDF text from document bytes",
    };

    RagloomError::new(RagloomErrorKind::InvalidInput, error).with_context(context)
}

fn map_docx_extract_error(error: DocxError) -> RagloomError {
    let context = match &error {
        DocxError::FileNotFound(path) => format!("DOCX is missing required OOXML part {path}"),
        DocxError::UnsupportedFormat(message) => {
            format!("unsupported DOCX format prevents text extraction: {message}")
        }
        DocxError::Structure(message) => format!("invalid DOCX structure: {message}"),
        _ => "failed to extract DOCX text from document bytes".to_string(),
    };

    RagloomError::new(RagloomErrorKind::InvalidInput, error).with_context(context)
}

/// Loads UTF-8 documents from the local filesystem.
///
/// # Why
/// The MVP ingest path often starts from local files. This loader keeps the
/// filesystem concerns encapsulated and returns crate-level errors with context.
#[derive(Debug, Default, Clone, Copy)]
pub struct FsUtf8Loader;

#[async_trait]
impl DocumentLoader for FsUtf8Loader {
    async fn load(&self, path: &str) -> Result<LoadedDocument, RagloomError> {
        let bytes = tokio::fs::read(path).await.map_err(|e| {
            RagloomError::new(RagloomErrorKind::Io, e).with_context("failed to load document bytes")
        })?;

        let text = extract_document_text(path, bytes)?;

        Ok(LoadedDocument { text })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use zip::write::SimpleFileOptions as FileOptions;

    fn write_test_file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).expect("write test file");
        path
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

    fn docx_without_document_xml_bytes() -> Vec<u8> {
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
        zip.finish().expect("finish docx").into_inner()
    }

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

    fn encrypted_pdf_bytes() -> Vec<u8> {
        let mut document = PdfDocument::load_mem(&minimal_pdf_bytes(
            "BT\n/F1 18 Tf\n50 100 Td\n(Secret PDF) Tj\nET\n",
        ))
        .expect("parse plaintext pdf");
        document.trailer.set(
            "ID",
            pdf_extract::Object::Array(vec![
                pdf_extract::Object::string_literal(b"ABC"),
                pdf_extract::Object::string_literal(b"DEF"),
            ]),
        );

        let version = pdf_extract::EncryptionVersion::V2 {
            document: &document,
            owner_password: "owner",
            user_password: "user",
            key_length: 40,
            permissions: pdf_extract::Permissions::all(),
        };

        let state = pdf_extract::EncryptionState::try_from(version).expect("build encryption");
        document.encrypt(&state).expect("encrypt pdf");

        let mut encrypted = Vec::new();
        document.save_to(&mut encrypted).expect("serialize pdf");
        encrypted
    }

    #[tokio::test]
    async fn fs_utf8_loader_reads_text_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(&dir, "hello.txt", b"hello\nworld");

        let loader = FsUtf8Loader;
        let loaded = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect("load file");

        assert_eq!(loaded.text, "hello\nworld");
    }

    #[tokio::test]
    async fn fs_utf8_loader_returns_error_with_context_on_missing_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("missing.txt");

        let loader = FsUtf8Loader;
        let err = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect_err("expected error");

        assert_eq!(err.kind, RagloomErrorKind::Io);
        assert!(err.to_string().contains("failed to load document bytes"));
    }

    #[tokio::test]
    async fn fs_utf8_loader_surfaces_utf8_extraction_context() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(&dir, "invalid.bin", &[0xff, 0xfe, 0xfd]);

        let loader = FsUtf8Loader;
        let err = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect_err("expected invalid utf-8");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("failed to extract UTF-8 text from document bytes")
        );
    }

    #[tokio::test]
    async fn fs_utf8_loader_extracts_text_from_pdf() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(
            &dir,
            "hello.pdf",
            &minimal_pdf_bytes("BT\n/F1 18 Tf\n50 100 Td\n(Hello PDF) Tj\nET\n"),
        );

        let loader = FsUtf8Loader;
        let loaded = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect("load pdf");

        assert_eq!(loaded.text, "\n\nHello PDF");
    }

    #[tokio::test]
    async fn fs_utf8_loader_returns_empty_string_for_pdf_without_extractable_text() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(&dir, "empty.pdf", &minimal_pdf_bytes(""));

        let loader = FsUtf8Loader;
        let loaded = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect("load empty pdf");

        assert_eq!(loaded.text, "");
    }

    #[tokio::test]
    async fn fs_utf8_loader_rejects_malformed_pdf_with_context() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(&dir, "broken.pdf", b"%PDF-1.4\nnot a real pdf\n");

        let loader = FsUtf8Loader;
        let err = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect_err("expected malformed pdf to fail");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("failed to parse PDF document bytes")
        );
    }

    #[tokio::test]
    async fn fs_utf8_loader_rejects_encrypted_pdf_with_clear_error() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(&dir, "secret.pdf", &encrypted_pdf_bytes());

        let loader = FsUtf8Loader;
        let err = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect_err("expected encrypted pdf to fail");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("encrypted PDFs are not supported for text extraction")
        );
    }

    #[tokio::test]
    async fn fs_utf8_loader_extracts_text_from_docx() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(
            &dir,
            "hello.docx",
            &minimal_docx_bytes(&["Hello DOCX", "Second paragraph"]),
        );

        let loader = FsUtf8Loader;
        let loaded = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect("load docx");

        assert_eq!(loaded.text, "Hello DOCX\nSecond paragraph\n");
    }

    #[tokio::test]
    async fn fs_utf8_loader_returns_empty_string_for_docx_without_extractable_text() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(&dir, "empty.docx", &minimal_docx_bytes(&[]));

        let loader = FsUtf8Loader;
        let loaded = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect("load empty docx");

        assert_eq!(loaded.text, "");
    }

    #[tokio::test]
    async fn fs_utf8_loader_extracts_docx_text_deterministically() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(&dir, "stable.docx", &minimal_docx_bytes(&["Stable text"]));

        let loader = FsUtf8Loader;
        let first = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect("first load");
        let second = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect("second load");

        assert_eq!(first.text, second.text);
    }

    #[tokio::test]
    async fn fs_utf8_loader_rejects_malformed_docx_with_context() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(&dir, "broken.docx", b"not a zip archive");

        let loader = FsUtf8Loader;
        let err = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect_err("expected malformed docx to fail");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("failed to extract DOCX text from document bytes")
        );
    }

    #[tokio::test]
    async fn fs_utf8_loader_rejects_docx_missing_document_xml_with_clear_error() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_test_file(&dir, "missing.xml.docx", &docx_without_document_xml_bytes());

        let loader = FsUtf8Loader;
        let err = loader
            .load(path.to_str().expect("utf-8 path"))
            .await
            .expect_err("expected missing document xml to fail");

        assert_eq!(err.kind, RagloomErrorKind::InvalidInput);
        assert!(
            err.to_string()
                .contains("DOCX is missing required OOXML part word/document.xml")
        );
    }
}
