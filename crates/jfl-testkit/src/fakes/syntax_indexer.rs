//! [`CannedSyntaxIndexer`]: a `SyntaxIndexer` that answers with the golden corpus's summaries.
//!
//! It applies the port's own source checks first (`syntax_indexer::source_text`: the byte-order
//! mark, the size, UTF-8 and nesting bounds), exactly as the tree-sitter adapter does, then looks
//! the text up among the summaries it was given. A text it was not given is refused as
//! [`SyntaxIndexError::TooMuchWork`]: a canned fake cannot summarise what it has not seen, and a
//! refusal is the one answer that can never pass for a wrong summary.
//!
//! Pinned by the `syntax_indexer` contract suite (`contracts::syntax_indexer::tests`).

use jfl_domain::Language;
use std::collections::BTreeMap;
use std::sync::Arc;

use jfl_application::syntax_indexer::{
    GrammarId, Indexed, SyntaxIndexError, SyntaxIndexer, source_text,
};

use crate::golden;

/// Canned summaries by text.
#[derive(Debug, Clone)]
pub struct CannedSyntaxIndexer {
    grammar: GrammarId,
    summaries: Arc<BTreeMap<String, Indexed>>,
}

impl CannedSyntaxIndexer {
    /// The goldens of `dir` (`orchard`, `upstream`) for every corpus file under `fixtures/java/<files>`.
    pub fn from_goldens(grammar: GrammarId, dir: &str, files: &str) -> Result<Self, String> {
        let mut summaries = BTreeMap::new();
        for file in golden::corpus(files)? {
            let path = golden::golden_path(dir, &file.path);
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let indexed =
                golden::parse(&text, &file.bytes).map_err(|e| format!("{}: {e}", file.path))?;
            let key = source_text(&file.bytes).map_err(|e| e.to_string())?;
            summaries.insert(key.to_owned(), indexed);
        }
        Ok(Self {
            grammar,
            summaries: Arc::new(summaries),
        })
    }

    /// The canned answer for `bytes`.
    pub fn answer(&self, bytes: &[u8]) -> Result<Indexed, SyntaxIndexError> {
        let text = source_text(bytes)?;
        self.summaries
            .get(text)
            .cloned()
            .ok_or(SyntaxIndexError::TooMuchWork)
    }
}

impl SyntaxIndexer for CannedSyntaxIndexer {
    fn language(&self) -> Language {
        Language::Java
    }

    fn grammar(&self) -> GrammarId {
        self.grammar
    }

    fn summariser_version(&self) -> u32 {
        1
    }

    fn index(
        &self,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<Indexed, SyntaxIndexError>> + Send {
        core::future::ready(self.answer(&bytes))
    }
}
