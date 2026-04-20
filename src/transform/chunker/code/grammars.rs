//! Language handle for each supported tree-sitter grammar.
//!
//! # Why
//! Isolate the `LanguageFn -> Language` conversion and grammar-specific API
//! shape differences here so the chunker body stays clean.

use tree_sitter::Language;

use super::query::Lang;

pub fn language_for(lang: Lang) -> Language {
    match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        Lang::Python => tree_sitter_python::LANGUAGE.into(),
        Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::Go => tree_sitter_go::LANGUAGE.into(),
        Lang::Java => tree_sitter_java::LANGUAGE.into(),
        Lang::C => tree_sitter_c::LANGUAGE.into(),
        Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Lang::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        Lang::Bash => tree_sitter_bash::LANGUAGE.into(),
    }
}
