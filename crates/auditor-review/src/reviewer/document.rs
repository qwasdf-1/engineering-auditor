//! A model-visible document that fits a byte budget: its row lists shrink
//! until the rendered Markdown fits, and only the rows it shows issue their
//! evidence IDs.

use std::collections::BTreeSet;

use crate::{
    render_model_document, ModelDocument, ModelField, ModelProjectionAudit, ModelSection,
    ModelValue,
};

/// One rendered fact and the evidence IDs it summarizes.
pub(super) struct Row {
    pub(super) text: String,
    pub(super) ids: Vec<String>,
}

impl Row {
    pub(super) fn new<I, S>(text: impl Into<String>, ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            text: text.into(),
            ids: ids.into_iter().map(Into::into).collect(),
        }
    }

    fn render(&self) -> String {
        format!("{} [{}]", self.text, self.ids.join(", "))
    }
}

pub(super) enum Item {
    /// A summary line that cites nothing.
    Text { label: String, value: String },
    /// Notes that cite nothing and are never cut.
    Notes { label: String, values: Vec<String> },
    /// Rows ranked most relevant first; `total` counts the rows the builder
    /// left out before rendering.
    Rows {
        label: String,
        rows: Vec<Row>,
        total: usize,
        shown: usize,
    },
    /// Repository source, which is shown whole or not at all.
    Source {
        label: String,
        language: String,
        content: String,
        ids: Vec<String>,
    },
}

impl Item {
    pub(super) fn text(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Text {
            label: label.into(),
            value: value.into(),
        }
    }

    pub(super) fn notes(label: impl Into<String>, values: Vec<String>) -> Self {
        Self::Notes {
            label: label.into(),
            values,
        }
    }

    /// The first `cap` of `rows`, recording how many there were.
    pub(super) fn rows(
        label: impl Into<String>,
        rows: impl IntoIterator<Item = Row>,
        cap: usize,
    ) -> Self {
        let mut rows = rows.into_iter();
        let kept = rows.by_ref().take(cap).collect::<Vec<_>>();
        let total = kept.len() + rows.count();
        let rows = kept;
        Self::Rows {
            label: label.into(),
            shown: rows.len(),
            rows,
            total,
        }
    }

    fn shrink(&mut self) -> bool {
        match self {
            Self::Rows { shown, .. } if *shown > 1 => {
                *shown = (*shown * 3 / 4).max(1);
                true
            }
            _ => false,
        }
    }
}

pub(super) struct Section {
    heading: String,
    items: Vec<Item>,
}

impl Section {
    pub(super) fn new(heading: impl Into<String>, items: Vec<Item>) -> Self {
        Self {
            heading: heading.into(),
            items,
        }
    }
}

pub(super) struct BoundedDocument {
    title: String,
    sections: Vec<Section>,
}

pub(super) struct Rendered {
    pub(super) markdown: String,
    pub(super) audit: ModelProjectionAudit,
    pub(super) evidence_ids: BTreeSet<String>,
    /// Whether the budget cut any list below the rows its builder kept.
    pub(super) truncated: bool,
}

impl BoundedDocument {
    pub(super) fn new(title: impl Into<String>, sections: Vec<Section>) -> Self {
        Self {
            title: title.into(),
            sections,
        }
    }

    /// Renders within `max_bytes`, cutting every row list by a quarter at a
    /// time; fails when the lists are down to one row and it still does not
    /// fit.
    pub(super) fn render(
        mut self,
        max_bytes: usize,
        source_digest: &str,
    ) -> Result<Rendered, String> {
        loop {
            let (document, evidence_ids, truncated) = self.model_document();
            let projection = render_model_document(&document, source_digest, Vec::<String>::new())
                .map_err(|error| error.to_string())?;
            if projection.markdown.len() <= max_bytes {
                return Ok(Rendered {
                    markdown: projection.markdown,
                    audit: projection.audit,
                    evidence_ids,
                    truncated,
                });
            }
            let mut shrunk = false;
            for item in self
                .sections
                .iter_mut()
                .flat_map(|section| &mut section.items)
            {
                shrunk |= item.shrink();
            }
            if !shrunk {
                return Err(format!(
                    "the document needs {} bytes at its smallest; the budget is {max_bytes}",
                    projection.markdown.len()
                ));
            }
        }
    }

    fn model_document(&self) -> (ModelDocument, BTreeSet<String>, bool) {
        let mut evidence_ids = BTreeSet::new();
        let mut truncated = false;
        let sections = self
            .sections
            .iter()
            .map(|section| {
                let fields = section
                    .items
                    .iter()
                    .map(|item| match item {
                        Item::Text { label, value } => {
                            ModelField::new(label, ModelValue::text(value))
                        }
                        Item::Notes { label, values } => {
                            ModelField::new(label, ModelValue::list(values.iter().cloned()))
                        }
                        Item::Rows {
                            label,
                            rows,
                            total,
                            shown,
                        } => {
                            let visible = &rows[..(*shown).min(rows.len())];
                            truncated |= visible.len() < rows.len();
                            evidence_ids
                                .extend(visible.iter().flat_map(|row| row.ids.iter().cloned()));
                            let label = if visible.len() < *total {
                                format!("{label} (top {} of {total})", visible.len())
                            } else {
                                label.clone()
                            };
                            ModelField::new(
                                label,
                                ModelValue::list(visible.iter().map(Row::render)),
                            )
                        }
                        Item::Source {
                            label,
                            language,
                            content,
                            ids,
                        } => {
                            evidence_ids.extend(ids.iter().cloned());
                            ModelField::new(
                                format!("{label} [{}]", ids.join(", ")),
                                ModelValue::repository_source(language, content),
                            )
                        }
                    })
                    .collect();
                ModelSection::new(&section.heading, fields)
            })
            .collect();
        (
            ModelDocument::new(&self.title, sections),
            evidence_ids,
            truncated,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(rows: usize) -> BoundedDocument {
        BoundedDocument::new(
            "Test",
            vec![Section::new(
                "Rows",
                vec![
                    Item::text("Summary", format!("{rows} rows")),
                    Item::rows(
                        "Files",
                        (0..rows).map(|index| {
                            Row::new(
                                format!("row {index:03} with some padding text"),
                                [format!("file:{index}")],
                            )
                        }),
                        rows,
                    ),
                ],
            )],
        )
    }

    #[test]
    fn rows_shrink_to_the_budget_and_only_shown_rows_issue_ids() {
        let full = document(40).render(64 * 1024, "sha256:test").unwrap();
        assert!(!full.truncated);
        assert_eq!(full.evidence_ids.len(), 40);
        let small = document(40).render(1024, "sha256:test").unwrap();
        assert!(small.truncated);
        assert!(small.markdown.len() <= 1024);
        assert!(small.markdown.contains("(top "));
        assert!(small.evidence_ids.len() < 40);
        assert!(small
            .evidence_ids
            .iter()
            .all(|id| small.markdown.contains(id.as_str())));
    }

    #[test]
    fn a_document_that_cannot_fit_is_an_error() {
        assert!(document(4).render(16, "sha256:test").is_err());
    }

    #[test]
    fn the_builder_cap_counts_every_row() {
        let Item::Rows { total, shown, .. } =
            Item::rows("R", (0..9).map(|i| Row::new("r", [i.to_string()])), 3)
        else {
            panic!("rows");
        };
        assert_eq!((total, shown), (9, 3));
    }
}
