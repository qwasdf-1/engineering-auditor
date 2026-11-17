//! The source a question's findings sit in, attached to the question so the
//! reviewer has read the code a recommendation would change. Each finding
//! brings its enclosing function or type, or the lines around it outside
//! one or inside one too long to attach whole; source past the question's
//! budget is named so the reviewer can read the part it needs.

use std::collections::hash_map::{Entry, HashMap};

use super::document::{BoundedDocument, Item, Rendered, Row, Section};
use super::questions::{Question, QuestionKind};
use super::retrieval::RetrievalHost;
use super::view::{severity_rank, ReportFinding};

/// Source one span may add; a longer symbol gives the lines around each
/// finding instead.
const SYMBOL_BYTES: usize = 4 * 1024;
/// Source one question may carry, each span counted with its label and
/// IDs.
const QUESTION_SOURCE_BYTES: usize = 16 * 1024;
/// Room for the headings and the spans named instead of attached.
const FRAME_BYTES: usize = 4 * 1024;
/// Lines either side of a finding outside any function or type, or inside
/// one too long to attach.
const CONTEXT_LINES: usize = 10;

/// One range of source, its text, and the findings in it.
struct Span {
    file: usize,
    start: usize,
    end: usize,
    symbol: Option<usize>,
    /// The symbol's lines when the span is only part of it.
    part_of: Option<(usize, usize)>,
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
        self.findings_source(&question.finding_ids, "Source of the cited findings")
    }

    /// The source of `finding_ids`, most severe first, under `title`; none
    /// when no finding has a line.
    pub(super) fn findings_source(
        &self,
        finding_ids: &[String],
        title: &str,
    ) -> Result<Option<Rendered>, String> {
        let spans = self.cited_spans(finding_ids)?;
        if spans.is_empty() {
            return Ok(None);
        }
        let mut items = Vec::new();
        let mut named = Vec::new();
        let mut budget = QUESTION_SOURCE_BYTES;
        for span in spans {
            if span.content.len() > SYMBOL_BYTES {
                named.push(self.named_row(&span, "too long to attach"));
                continue;
            }
            let item = self.source_item(&span);
            match item.source_bytes().filter(|bytes| *bytes <= budget) {
                Some(bytes) => {
                    budget -= bytes;
                    items.push(item);
                }
                None => named.push(self.named_row(&span, "past this question's source budget")),
            }
        }
        if !named.is_empty() {
            items.push(Item::rows(
                "Not attached; read the lines you need",
                named,
                usize::MAX,
            ));
        }
        BoundedDocument::new(title, vec![Section::new("Source", items)])
            .render(
                QUESTION_SOURCE_BYTES + FRAME_BYTES,
                &self.view.source_digest,
            )
            .map(Some)
    }

    /// One span per finding with a line, most severe first; a finding
    /// inside an earlier span joins it, and overlapping parts of one symbol
    /// merge.
    fn cited_spans(&self, finding_ids: &[String]) -> Result<Vec<Span>, String> {
        let mut findings = finding_ids
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
            match spans
                .iter_mut()
                .find(|known| known.file == file && joins(known, &span, &lines))
            {
                Some(known) => {
                    known.start = known.start.min(span.start);
                    known.end = known.end.max(span.end);
                    known.content = lines[known.start - 1..known.end].concat();
                    known.findings.extend(span.findings);
                }
                None => spans.push(span),
            }
        }
        Ok(spans)
    }

    /// The finding's enclosing symbol, or the lines around it outside one
    /// or inside one too long to attach, within the file's `lines`; none
    /// for an empty file.
    fn span(&self, finding: &ReportFinding, file: usize, lines: &[&str]) -> Option<Span> {
        let count = lines.len();
        if count == 0 {
            return None;
        }
        let line = finding.line.expect("filtered").clamp(1, count);
        let symbol = finding.symbol.map(|symbol| {
            let span = self.view.symbol(symbol).span;
            let end = span.end_line.clamp(1, count);
            (span.start_line.clamp(1, end), end)
        });
        let whole =
            symbol.filter(|(start, end)| lines[start - 1..*end].concat().len() <= SYMBOL_BYTES);
        let (low, high) = symbol.unwrap_or((1, count));
        let (start, end) = whole.unwrap_or((
            line.saturating_sub(CONTEXT_LINES).max(low),
            (line + CONTEXT_LINES).min(high),
        ));
        Some(Span {
            file,
            start,
            end,
            symbol: finding.symbol,
            part_of: symbol.filter(|_| whole.is_none()),
            findings: vec![finding.evidence_id()],
            content: lines[start - 1..end].concat(),
        })
    }

    fn source_item(&self, span: &Span) -> Item {
        let mapped = self.view.file(span.file);
        let label = format!("{}, {}", self.subject(span), lines_of(span, &mapped.path));
        let mut ids = vec![format!(
            "source-span:{}#L{}-L{}",
            mapped.path, span.start, span.end
        )];
        ids.extend(span.findings.iter().cloned());
        Item::Source {
            label,
            language: mapped.language.clone(),
            content: span.content.clone(),
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
        match (span.symbol, span.part_of) {
            (Some(symbol), Some((start, end))) => format!(
                "Part of {} (lines {start}-{end})",
                self.view.qualified_name(symbol)
            ),
            (Some(symbol), None) => self.view.qualified_name(symbol),
            (None, _) => "Around the finding".to_string(),
        }
    }
}

/// Whether `span` lies inside `known`, or both are parts of one symbol
/// that touch or overlap and together still fit one span.
fn joins(known: &Span, span: &Span, lines: &[&str]) -> bool {
    let inside = known.start <= span.start && span.end <= known.end;
    let touching = known.part_of.is_some()
        && known.part_of == span.part_of
        && span.start <= known.end + 1
        && known.start <= span.end + 1;
    let (start, end) = (known.start.min(span.start), known.end.max(span.end));
    inside || (touching && lines[start - 1..end].concat().len() <= SYMBOL_BYTES)
}

fn lines_of(span: &Span, path: &str) -> String {
    format!("lines {}-{} of {path}", span.start, span.end)
}
