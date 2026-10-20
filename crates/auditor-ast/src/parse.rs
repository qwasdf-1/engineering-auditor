//! Parsing a source into an [`AstTree`]: the language's grammar, then an
//! alternate grammar, then recovery by quarantine for sources with syntax
//! errors.

use std::sync::{Arc, OnceLock};

use tree_sitter::{Parser, Tree};

use crate::{languages, macro_blanking, quarantine, AstLang, AstParser, AstTree};

impl AstParser {
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        let lang = languages::get_language(&AstLang::Rust)?;
        parser
            .set_language(&lang)
            .map_err(|e| format!("failed to set language: {e}"))?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, lang: &AstLang, source: &str) -> Result<AstTree, String> {
        self.parse_shared(lang, Arc::<str>::from(source))
    }

    pub fn parse_shared(&mut self, lang: &AstLang, source: Arc<str>) -> Result<AstTree, String> {
        let language = languages::get_language(lang)?;
        self.parser
            .set_language(&language)
            .map_err(|e| format!("failed to set language: {e}"))?;
        let parsed = if matches!(lang, AstLang::C | AstLang::Cpp) {
            macro_blanking::blank_macros(&source, *lang == AstLang::Cpp)
        } else {
            std::borrow::Cow::Borrowed(source.as_ref())
        };
        let mut tree = self
            .parser
            .parse(parsed.as_ref(), None)
            .ok_or_else(|| "failed to parse source".to_string())?;
        let mut recovered = None;
        if tree.root_node().has_error() {
            if let Some(alternate) = self.parse_alternate(lang, parsed.as_ref())? {
                tree = alternate;
            } else if let Some(recovery) = self.recover(lang, &language, &tree, parsed.as_ref())? {
                recovered = Some(Box::new(AstTree {
                    tree: recovery.tree,
                    source: Arc::from(quarantine::blanked(&source, &recovery.removals)),
                    language: *lang,
                    quarantined: quarantine::records(&source, &recovery.removals),
                    recovered: None,
                    function_spans: OnceLock::new(),
                    semantic_facts: OnceLock::new(),
                }));
            }
        }
        drop(parsed);
        Ok(AstTree {
            tree,
            source,
            language: *lang,
            quarantined: Vec::new(),
            recovered,
            function_spans: OnceLock::new(),
            semantic_facts: OnceLock::new(),
        })
    }
}

impl AstParser {
    /// `parsed` without the declarations holding its syntax errors, under
    /// `language`, which `tree` was parsed with.
    fn recover(
        &mut self,
        lang: &AstLang,
        language: &tree_sitter::Language,
        tree: &Tree,
        parsed: &str,
    ) -> Result<Option<quarantine::Recovered>, String> {
        self.parser
            .set_language(language)
            .map_err(|e| format!("failed to set language: {e}"))?;
        Ok(quarantine::recover(&mut self.parser, lang, tree, parsed))
    }

    /// The tree of `lang`'s alternate grammar, when it parses `source`
    /// without error: TypeScript sources are often TSX.
    fn parse_alternate(
        &mut self,
        lang: &AstLang,
        source: &str,
    ) -> Result<Option<tree_sitter::Tree>, String> {
        let Some(language) = languages::alternate_language(lang) else {
            return Ok(None);
        };
        self.parser
            .set_language(&language)
            .map_err(|e| format!("failed to set language: {e}"))?;
        Ok(self
            .parser
            .parse(source, None)
            .filter(|tree| !tree.root_node().has_error()))
    }
}
