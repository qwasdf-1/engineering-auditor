use tree_sitter::Language as TsLanguage;
use tree_sitter_language::LanguageFn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum AstLang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    Kotlin,
    Bash,
    CSharp,
    Julia,
    C,
    Cpp,
    Ruby,
}

pub const SUPPORTED_LANGUAGES: &[AstLang] = &[
    AstLang::Rust,
    AstLang::Python,
    AstLang::JavaScript,
    AstLang::TypeScript,
    AstLang::Go,
    AstLang::Java,
    AstLang::Kotlin,
    AstLang::Bash,
    AstLang::CSharp,
    AstLang::Julia,
    AstLang::C,
    AstLang::Cpp,
    AstLang::Ruby,
];

struct GrammarEntry {
    grammar: LanguageFn,
    function_nodes: &'static [&'static str],
    comment_nodes: &'static [&'static str],
}

const GRAMMARS: &[GrammarEntry] = &[
    GrammarEntry {
        grammar: tree_sitter_rust::LANGUAGE,
        function_nodes: &["function_item"],
        comment_nodes: &["line_comment", "block_comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_python::LANGUAGE,
        function_nodes: &["function_definition"],
        comment_nodes: &["comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_javascript::LANGUAGE,
        function_nodes: &[
            "function_declaration",
            "function_expression",
            "generator_function_declaration",
            "generator_function",
            "method_definition",
            "arrow_function",
        ],
        comment_nodes: &["comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        function_nodes: &[
            "function_declaration",
            "function_expression",
            "generator_function_declaration",
            "generator_function",
            "method_definition",
            "arrow_function",
        ],
        comment_nodes: &["comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_go::LANGUAGE,
        function_nodes: &["function_declaration", "method_declaration"],
        comment_nodes: &["comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_java::LANGUAGE,
        function_nodes: &["method_declaration"],
        comment_nodes: &["line_comment", "block_comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_kotlin_sg::LANGUAGE,
        function_nodes: &["function_declaration"],
        comment_nodes: &["line_comment", "multiline_comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_bash::LANGUAGE,
        function_nodes: &["function_definition"],
        comment_nodes: &["comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_c_sharp::LANGUAGE,
        function_nodes: &["method_declaration", "local_function_statement"],
        comment_nodes: &["comment", "line_comment", "block_comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_julia::LANGUAGE,
        function_nodes: &["function_definition"],
        comment_nodes: &["comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_c::LANGUAGE,
        function_nodes: &["function_definition"],
        comment_nodes: &["comment", "line_comment", "block_comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_cpp::LANGUAGE,
        function_nodes: &["function_definition"],
        comment_nodes: &["comment", "line_comment", "block_comment"],
    },
    GrammarEntry {
        grammar: tree_sitter_ruby::LANGUAGE,
        function_nodes: &["method", "singleton_method"],
        comment_nodes: &["comment"],
    },
];

fn lang_index(lang: &AstLang) -> usize {
    *lang as usize
}

pub fn get_language(lang: &AstLang) -> Result<TsLanguage, String> {
    let idx = lang_index(lang);
    Ok(TsLanguage::from(GRAMMARS[idx].grammar))
}

/// A second grammar for sources of `lang` that its own grammar rejects:
/// TSX for TypeScript, whose JSX the TypeScript grammar cannot read.
pub fn alternate_language(lang: &AstLang) -> Option<TsLanguage> {
    match lang {
        AstLang::TypeScript => Some(TsLanguage::from(tree_sitter_typescript::LANGUAGE_TSX)),
        _ => None,
    }
}

pub fn function_node_kinds() -> &'static [&'static str] {
    static KINDS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    KINDS.get_or_init(|| {
        let mut seen = std::collections::BTreeSet::new();
        GRAMMARS
            .iter()
            .flat_map(|entry| entry.function_nodes.iter().copied())
            .filter(|kind| seen.insert(*kind))
            .collect()
    })
}

pub fn comment_node_kinds(lang: &AstLang) -> &'static [&'static str] {
    let idx = lang_index(lang);
    GRAMMARS[idx].comment_nodes
}
