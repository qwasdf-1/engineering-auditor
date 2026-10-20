use auditor_core::{render_typed_markdown, sha256_digest_bytes};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use auditor_core::{
    MarkdownDocument as ModelDocument, MarkdownField as ModelField,
    MarkdownSection as ModelSection, MarkdownValue as ModelValue,
};

pub const MODEL_MARKDOWN_RENDERER_VERSION: u32 = 2;

pub fn model_value_from_json(value: &Value) -> ModelValue {
    match value {
        Value::Null => ModelValue::list(std::iter::empty::<String>()),
        Value::Bool(value) => ModelValue::text(value.to_string()),
        Value::Number(value) => ModelValue::text(value.to_string()),
        Value::String(value) => ModelValue::text(value),
        Value::Array(values) if values.iter().all(Value::is_string) => ModelValue::list(
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string),
        ),
        Value::Array(values) => {
            ModelValue::Items(values.iter().map(model_value_from_json).collect())
        }
        Value::Object(values) => ModelValue::fields(
            values
                .iter()
                .map(|(key, value)| ModelField::new(key, model_value_from_json(value)))
                .collect(),
        ),
    }
}

pub fn render_structured_model_document(
    title: impl Into<String>,
    value: &Value,
    source_digest: impl Into<String>,
    excluded_field_paths: &[&str],
) -> Result<ModelProjection, std::fmt::Error> {
    let fields = match model_value_from_json(value) {
        ModelValue::Fields(fields) => fields,
        other => vec![ModelField::new("Value", other)],
    };
    render_model_document(
        &ModelDocument::new(title, vec![ModelSection::new("Context", fields)]),
        source_digest,
        excluded_field_paths.iter().copied(),
    )
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProjectionAudit {
    pub renderer_version: u32,
    pub source_digest: String,
    pub markdown_digest: String,
    pub markdown_bytes: usize,
    pub excluded_field_paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelProjection {
    pub markdown: String,
    pub audit: ModelProjectionAudit,
}

pub fn render_model_document<I, S>(
    document: &ModelDocument,
    source_digest: impl Into<String>,
    excluded_field_paths: I,
) -> Result<ModelProjection, std::fmt::Error>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let markdown = render_typed_markdown(document)?;
    let mut excluded_field_paths = excluded_field_paths
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    excluded_field_paths.sort();
    excluded_field_paths.dedup();
    let audit = ModelProjectionAudit {
        renderer_version: MODEL_MARKDOWN_RENDERER_VERSION,
        source_digest: source_digest.into(),
        markdown_digest: sha256_digest_bytes(markdown.as_bytes()),
        markdown_bytes: markdown.len(),
        excluded_field_paths,
    };
    Ok(ModelProjection { markdown, audit })
}
