//! The source a question's findings sit in, attached to the question so the
//! reviewer has read the code a recommendation would change. Each finding
//! brings its enclosing function or type, or the lines around it outside
//! one; a symbol too long to attach, or past the question's budget, is
//! named so the reviewer can read the part it needs.

use std::collections::hash_map::{Entry, HashMap};

use super::document::{BoundedDocument, Item, Rendered, Row, Section};
use super::questions::{Question, QuestionKind};
use super::retrieval::RetrievalHost;
use super::view::{severity_rank, ReportFinding};

/// Source one symbol may add; a longer one is named instead.
const SYMBOL_BYTES: usize = 4 * 1024;
/// Source one question may carry.
const QUESTION_SOURCE_BYTES: usize = 16 * 1024;
/// Room for the headings, labels, and names around the source.
const FRAME_BYTES: usize = 4 * 1024;
/// Lines either side of a finding outside any function or type.
const CONTEXT_LINES: usize = 10;

/// One range of source, its text, and the findings in it.
struct Span {
    file: usize,
    start: usize,
    end: usize,
    symbol: Option<usize>,
    findings: Vec<String>,
    content: String,
}

impl RetrievalHost<'_, '_> {
    /// The source of `question`'s findings, most severe first; none for
    /// the plan, which builds on the other answers, or for a question whose
    /// findings have no line.
    pub(super) fn cited_source(&self, question: &Question) -> Result<Option<Rendered>, String> {
        if question.kind == QuestionKind::RemediationPlan {
            return Ok(None);
        }
        let spans = self.cited_spans(question)?;
        if spans.is_empty() {
            return Ok(None);
        }
        let mut items = Vec::new();
        let mut named = Vec::new();
        let mut budget = QUESTION_SOURCE_BYTES;
        for span in spans {
            let bytes = span.content.len();
            if bytes <= SYMBOL_BYTES.min(budget) {
                budget -= bytes;
                items.push(self.source_item(span));
            } else if bytes > SYMBOL_BYTES {
                named.push(self.named_row(&span, "too long to attach"));
            } else {
                named.push(self.named_row(&span, "past this question's source budget"));
            }
        }
        if !named.is_empty() {
            items.push(Item::rows(
                "Not attached; read the lines you need",
                named,
                usize::MAX,
            ));
        }
        BoundedDocument::new(
            "Source of the cited findings",
            vec![Section::new("Source", items)],
        )
        .render(
            QUESTION_SOURCE_BYTES + FRAME_BYTES,
            &self.view.source_digest,
        )
        .map(Some)
    }

    /// One span per finding with a line, most severe first; a finding
    /// inside an earlier span joins it.
    fn cited_spans(&self, question: &Question) -> Result<Vec<Span>, String> {
        let mut findings = question
            .finding_ids
            .iter()
            .filter_map(|id| self.view.finding_by_evidence_id(id))
            .filter(|finding| finding.file.is_some() && finding.line.is_some())
            .collect::<Vec<_>>();
        findings.sort_by_key(|finding| severity_rank(&finding.severity));
        let mut texts = HashMap::new();
        let mut spans: Vec<Span> = Vec::new();
        for finding in findings {
            let file = finding.file.expect("filtered");
            let text = match texts.entry(file) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(self.source_text(file)?),
            };
            let lines = text.split_inclusive('\n').collect::<Vec<_>>();
            let Some(span) = self.span(finding, file, &lines) else {
                continue;
            };
            match spans.iter_mut().find(|known| {
                known.file == file && known.start <= span.start && span.end <= known.end
            }) {
                Some(known) => known.findings.extend(span.findings),
                None => spans.push(span),
            }
        }
        Ok(spans)
    }

    /// The finding's enclosing symbol, or the lines around it, within the
    /// file's `lines`; none for an empty file.
    fn span(&self, finding: &ReportFinding, file: usize, lines: &[&str]) -> Option<Span> {
        let count = lines.len();
        let line = finding.line.expect("filtered").min(count);
        let (start, end) = match finding.symbol {
            Some(symbol) => {
                let span = self.view.symbol(symbol).span;
                (span.start_line, span.end_line)
            }
            None => (line.saturating_sub(CONTEXT_LINES), line + CONTEXT_LINES),
        };
        let end = end.min(count);
        let start = start.clamp(1, end.max(1));
        (count > 0).then(|| Span {
            file,
            start,
            end,
            symbol: finding.symbol,
            findings: vec![finding.evidence_id()],
            content: lines[start - 1..end].concat(),
        })
    }

    fn source_item(&self, span: Span) -> Item {
        let mapped = self.view.file(span.file);
        let label = format!("{}, {}", self.subject(&span), lines_of(&span, &mapped.path));
        let mut ids = vec![format!(
            "source-span:{}#L{}-L{}",
            mapped.path, span.start, span.end
        )];
        ids.extend(span.findings);
        Item::Source {
            label,
            language: mapped.language.clone(),
            content: span.content,
            ids,
        }
    }

    fn named_row(&self, span: &Span, why: &str) -> Row {
        let mapped = self.view.file(span.file);
        let mut ids = span.symbol.map_or_else(
            || vec![mapped.id.clone()],
            |symbol| vec![self.view.symbol(symbol).id.clone()],
        );
        ids.extend(span.findings.iter().cloned());
        Row::new(
            format!(
                "{}, {} ({} bytes), {why}",
                self.subject(span),
                lines_of(span, &mapped.path),
                span.content.len()
            ),
            ids,
        )
    }

    fn subject(&self, span: &Span) -> String {
        match span.symbol {
            Some(symbol) => self.view.qualified_name(symbol),
            None => "Around the finding".to_string(),
        }
    }
}

fn lines_of(span: &Span, path: &str) -> String {
    format!("lines {}-{} of {path}", span.start, span.end)
}
