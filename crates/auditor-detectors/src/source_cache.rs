use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::{AuditedFile, FunctionSpan};

struct SourceText {
    text: Arc<str>,
    line_ranges: Box<[Range<usize>]>,
    function_spans: OnceLock<Arc<[FunctionSpan]>>,
}

impl SourceText {
    fn new(text: String) -> Self {
        let text: Arc<str> = text.into();
        let mut line_ranges = Vec::new();
        let mut start = 0usize;
        for (index, byte) in text.bytes().enumerate() {
            if byte != b'\n' {
                continue;
            }
            let end = if index > start && text.as_bytes()[index - 1] == b'\r' {
                index - 1
            } else {
                index
            };
            line_ranges.push(start..end);
            start = index + 1;
        }
        if start < text.len() {
            line_ranges.push(start..text.len());
        }
        Self {
            text,
            line_ranges: line_ranges.into_boxed_slice(),
            function_spans: OnceLock::new(),
        }
    }

    fn lines(&self) -> Vec<&str> {
        self.line_ranges
            .iter()
            .map(|range| &self.text[range.clone()])
            .collect()
    }
}

pub(crate) struct FileCache {
    sources: HashMap<PathBuf, SourceText>,
}

impl FileCache {
    pub(crate) fn new(files: &[AuditedFile]) -> Self {
        let mut sources = HashMap::new();
        for file in files {
            if let Ok(text) = std::fs::read_to_string(&file.absolute) {
                sources.insert(file.relative.clone(), SourceText::new(text));
            }
        }
        Self { sources }
    }

    pub(crate) fn text(&self, file: &AuditedFile) -> &str {
        self.sources
            .get(&file.relative)
            .map(|source| source.text.as_ref())
            .unwrap_or_default()
    }

    pub(crate) fn shared_text(&self, file: &AuditedFile) -> Option<Arc<str>> {
        self.sources
            .get(&file.relative)
            .map(|source| Arc::clone(&source.text))
    }

    pub(crate) fn lines(&self, file: &AuditedFile) -> Vec<&str> {
        self.sources
            .get(&file.relative)
            .map(SourceText::lines)
            .unwrap_or_default()
    }

    pub(crate) fn cache_function_spans(&self, file: &AuditedFile, spans: Arc<[FunctionSpan]>) {
        if let Some(source) = self.sources.get(&file.relative) {
            let _ = source.function_spans.set(spans);
        }
    }

    pub(crate) fn function_spans(&self, file: &AuditedFile) -> Option<Arc<[FunctionSpan]>> {
        self.sources
            .get(&file.relative)?
            .function_spans
            .get()
            .cloned()
    }

    pub(crate) fn line_count(&self, file: &AuditedFile) -> usize {
        self.sources.get(&file.relative).map_or(0, |source| {
            source
                .line_ranges
                .iter()
                .filter(|range| !source.text[(**range).clone()].trim().is_empty())
                .count()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_text_exposes_borrowed_lines_with_std_line_semantics() {
        let source = SourceText::new("alpha\r\nbeta\n\n".to_string());
        let lines = source.lines();
        assert_eq!(lines, ["alpha", "beta", ""]);

        let source_start = source.text.as_ptr() as usize;
        let source_end = source_start + source.text.len();
        assert!(lines.iter().all(|line| {
            let line_start = line.as_ptr() as usize;
            source_start <= line_start && line_start <= source_end
        }));
    }

    #[test]
    fn source_text_retains_one_shared_function_span_allocation() {
        let source = SourceText::new("fn main() {}\n".to_string());
        let spans: Arc<[FunctionSpan]> = vec![FunctionSpan {
            name: "main".to_string(),
            start_line: 1,
            end_line: 1,
            start_byte: 0,
            end_byte: 12,
        }]
        .into();
        source.function_spans.set(Arc::clone(&spans)).unwrap();

        let cached = source.function_spans.get().unwrap();
        assert!(Arc::ptr_eq(&spans, cached));
    }
}
