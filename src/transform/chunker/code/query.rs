//! Declaration-level node-type constants per supported language.
//!
//! # Why
//! Semantic chunking wants to cut at outer boundaries of function / class /
//! module declarations. Each grammar names nodes differently; this table is
//! the single source of truth.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    Bash,
}

impl Lang {
    pub fn as_fingerprint(&self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::Go => "go",
            Lang::Java => "java",
            Lang::C => "c",
            Lang::Cpp => "cpp",
            Lang::Ruby => "ruby",
            Lang::Bash => "bash",
        }
    }

    /// Grammar fingerprint encoded into the chunker's StrategyFingerprint.
    /// MUST reflect the crate version pinned in Cargo.toml.
    pub fn grammar_fingerprint(&self) -> &'static str {
        match self {
            Lang::Rust => "tree-sitter-rust@0.24",
            Lang::Python => "tree-sitter-python@0.25",
            Lang::JavaScript => "tree-sitter-javascript@0.25",
            Lang::TypeScript => "tree-sitter-typescript@0.23",
            Lang::Tsx => "tree-sitter-typescript@0.23-tsx",
            Lang::Go => "tree-sitter-go@0.25",
            Lang::Java => "tree-sitter-java@0.23",
            Lang::C => "tree-sitter-c@0.24",
            Lang::Cpp => "tree-sitter-cpp@0.23",
            Lang::Ruby => "tree-sitter-ruby@0.23",
            Lang::Bash => "tree-sitter-bash@0.25",
        }
    }

    pub fn declaration_node_types(&self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "function_item",
                "impl_item",
                "struct_item",
                "enum_item",
                "trait_item",
                "mod_item",
                "union_item",
                "type_item",
                "const_item",
                "static_item",
                "use_declaration",
                "extern_crate_declaration",
                "macro_definition",
                "foreign_mod_item",
            ],
            Lang::Python => &[
                "function_definition",
                "class_definition",
                "decorated_definition",
            ],
            Lang::JavaScript | Lang::Tsx => &[
                "function_declaration",
                "method_definition",
                "class_declaration",
                "export_statement",
                "lexical_declaration",
                "variable_declaration",
            ],
            Lang::TypeScript => &[
                "function_declaration",
                "method_definition",
                "class_declaration",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "export_statement",
                "lexical_declaration",
            ],
            Lang::Go => &[
                "function_declaration",
                "method_declaration",
                "type_declaration",
                "var_declaration",
                "const_declaration",
            ],
            Lang::Java => &[
                "method_declaration",
                "class_declaration",
                "interface_declaration",
                "constructor_declaration",
                "enum_declaration",
            ],
            Lang::C => &[
                "function_definition",
                "struct_specifier",
                "union_specifier",
                "enum_specifier",
                "type_definition",
            ],
            Lang::Cpp => &[
                "function_definition",
                "class_specifier",
                "struct_specifier",
                "union_specifier",
                "enum_specifier",
                "namespace_definition",
                "template_declaration",
                "type_definition",
            ],
            Lang::Ruby => &["method", "class", "module", "singleton_method"],
            Lang::Bash => &["function_definition"],
        }
    }
}
