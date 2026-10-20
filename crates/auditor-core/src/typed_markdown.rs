use std::fmt::Write;

pub const TYPED_MARKDOWN_RENDERER_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownDocument {
    pub title: String,
    pub sections: Vec<MarkdownSection>,
}

impl MarkdownDocument {
    pub fn new(title: impl Into<String>, sections: Vec<MarkdownSection>) -> Self {
        Self {
            title: title.into(),
            sections,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownSection {
    pub heading: String,
    pub fields: Vec<MarkdownField>,
}

impl MarkdownSection {
    pub fn new(heading: impl Into<String>, fields: Vec<MarkdownField>) -> Self {
        Self {
            heading: heading.into(),
            fields,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownField {
    pub label: String,
    pub value: MarkdownValue,
}

impl MarkdownField {
    pub fn new(label: impl Into<String>, value: MarkdownValue) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkdownValue {
    Text(String),
    List(Vec<String>),
    Fields(Vec<MarkdownField>),
    Items(Vec<MarkdownValue>),
    Sections(Vec<MarkdownSection>),
    RepositorySource { language: String, content: String },
}

impl MarkdownValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    pub fn list<I, S>(values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::List(values.into_iter().map(Into::into).collect())
    }

    pub fn fields(fields: Vec<MarkdownField>) -> Self {
        Self::Fields(fields)
    }

    pub fn sections(sections: Vec<MarkdownSection>) -> Self {
        Self::Sections(sections)
    }

    pub fn repository_source(language: impl Into<String>, content: impl Into<String>) -> Self {
        Self::RepositorySource {
            language: language.into(),
            content: content.into(),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Text(value) => value.trim().is_empty(),
            Self::List(values) => values.is_empty(),
            Self::Fields(fields) => fields.iter().all(|field| field.value.is_empty()),
            Self::Items(values) => values.iter().all(Self::is_empty),
            Self::Sections(sections) => sections
                .iter()
                .all(|section| section.fields.iter().all(|field| field.value.is_empty())),
            Self::RepositorySource { .. } => false,
        }
    }
}

pub fn render_typed_markdown(document: &MarkdownDocument) -> Result<String, std::fmt::Error> {
    let mut markdown = String::new();
    writeln!(markdown, "# {}\n", document.title.trim())?;
    render_sections(&mut markdown, &document.sections, 2)?;
    Ok(markdown)
}

fn render_sections(
    output: &mut String,
    sections: &[MarkdownSection],
    heading_depth: usize,
) -> Result<(), std::fmt::Error> {
    for section in sections {
        if section.fields.iter().all(|field| field.value.is_empty()) {
            continue;
        }
        writeln!(
            output,
            "{} {}\n",
            "#".repeat(heading_depth),
            section.heading.trim()
        )?;
        render_fields(output, &section.fields, 0)?;
        writeln!(output)?;
    }
    Ok(())
}

fn render_fields(
    output: &mut String,
    fields: &[MarkdownField],
    indent: usize,
) -> Result<(), std::fmt::Error> {
    for field in fields {
        if field.value.is_empty() {
            continue;
        }
        render_field(output, field, indent)?;
    }
    Ok(())
}

fn render_field(
    output: &mut String,
    field: &MarkdownField,
    indent: usize,
) -> Result<(), std::fmt::Error> {
    let prefix = "  ".repeat(indent);
    let label = field.label.trim();
    match &field.value {
        MarkdownValue::Text(value) => {
            render_text_field(output, &prefix, label, value)?;
        }
        MarkdownValue::List(values) => {
            writeln!(output, "{prefix}- **{label}:**")?;
            for value in values {
                writeln!(output, "{prefix}  - {}", value.replace('\r', ""))?;
            }
        }
        MarkdownValue::Fields(values) => {
            writeln!(output, "{prefix}- **{label}:**")?;
            render_fields(output, values, indent + 1)?;
        }
        MarkdownValue::Items(values) => {
            render_items_field(output, &prefix, label, values, indent)?;
        }
        MarkdownValue::Sections(values) => {
            writeln!(output, "{prefix}- **{label}:**")?;
            render_sections(output, values, 3 + indent)?;
        }
        MarkdownValue::RepositorySource { language, content } => {
            render_repository_source(output, &prefix, label, language, content)?;
        }
    }
    Ok(())
}

fn render_text_field(
    output: &mut String,
    prefix: &str,
    label: &str,
    value: &str,
) -> Result<(), std::fmt::Error> {
    let normalized = value.replace('\r', "");
    if !normalized.contains('\n') {
        return writeln!(output, "{prefix}- **{label}:** {normalized}");
    }
    writeln!(output, "{prefix}- **{label}:**")?;
    for line in normalized.lines() {
        writeln!(output, "{prefix}  > {line}")?;
    }
    Ok(())
}

fn render_items_field(
    output: &mut String,
    prefix: &str,
    label: &str,
    values: &[MarkdownValue],
    indent: usize,
) -> Result<(), std::fmt::Error> {
    writeln!(output, "{prefix}- **{label}:**")?;
    for (index, value) in values.iter().enumerate() {
        render_fields(
            output,
            &[MarkdownField::new(
                format!("Item {}", index + 1),
                value.clone(),
            )],
            indent + 1,
        )?;
    }
    Ok(())
}

fn render_repository_source(
    output: &mut String,
    prefix: &str,
    label: &str,
    language: &str,
    content: &str,
) -> Result<(), std::fmt::Error> {
    writeln!(output, "{prefix}- **{label}:**")?;
    writeln!(output, "```{}", language.trim())?;
    write!(output, "{}", content.replace('\r', ""))?;
    if !content.ends_with('\n') {
        writeln!(output)?;
    }
    writeln!(output, "```")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_is_deterministic_and_omits_empty_values() {
        let document = MarkdownDocument::new(
            "Audit",
            vec![MarkdownSection::new(
                "Finding",
                vec![
                    MarkdownField::new("Title", MarkdownValue::text("Repeated work")),
                    MarkdownField::new("Empty", MarkdownValue::list(Vec::<String>::new())),
                ],
            )],
        );
        let first = render_typed_markdown(&document).unwrap();
        assert_eq!(first, render_typed_markdown(&document).unwrap());
        assert!(first.contains("**Title:** Repeated work"));
        assert!(!first.contains("Empty"));
    }
}
