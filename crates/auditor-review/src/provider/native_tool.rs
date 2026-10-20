//! Provider-native tool declarations shared by qualification and review-v2.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::ProviderProtocol;

pub const TOOL_NOOP: &str = "noop";
pub const PROVIDER_TOOL_TRANSLATION_VERSION: &str = "v2-native-tools-1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Tool {
    NoOp,
}

impl Tool {
    pub fn spec(&self, strict: bool) -> ToolSpec {
        match self {
            Self::NoOp => ToolSpec {
                name: TOOL_NOOP,
                description: "Complete the provider-native tool qualification probe.",
                parameters: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {}
                }),
                strict,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    pub strict: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ToolChoice {
    #[default]
    Auto,
    Required,
    Specific(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderToolDeclaration {
    pub tool_block: Value,
    pub choice_block: Value,
}

pub fn translate(
    specs: &[ToolSpec],
    choice: ToolChoice,
    protocol: ProviderProtocol,
) -> Option<ProviderToolDeclaration> {
    (!specs.is_empty()).then(|| ProviderToolDeclaration {
        tool_block: translated_tools(protocol, specs),
        choice_block: translated_choice(protocol, choice),
    })
}

fn translated_tools(protocol: ProviderProtocol, specs: &[ToolSpec]) -> Value {
    json!(specs
        .iter()
        .map(|spec| match protocol {
            ProviderProtocol::ChatCompletions => json!({
                "type": "function",
                "function": {
                    "name": spec.name,
                    "description": spec.description,
                    "parameters": spec.parameters,
                    "strict": spec.strict
                }
            }),
            ProviderProtocol::Messages => {
                let mut declaration = json!({
                    "name": spec.name,
                    "description": spec.description,
                    "input_schema": spec.parameters
                });
                if spec.strict {
                    declaration["strict"] = json!(true);
                }
                declaration
            }
            ProviderProtocol::Responses => json!({
                "type": "function",
                "name": spec.name,
                "description": spec.description,
                "parameters": spec.parameters,
                "strict": spec.strict
            }),
        })
        .collect::<Vec<_>>())
}

fn translated_choice(protocol: ProviderProtocol, choice: ToolChoice) -> Value {
    match (protocol, choice) {
        (ProviderProtocol::Responses, ToolChoice::Auto) => json!("auto"),
        (ProviderProtocol::Responses, ToolChoice::Required) => json!("required"),
        (ProviderProtocol::Responses, ToolChoice::Specific(name)) => {
            json!({"type": "function", "function_name": name})
        }
        (ProviderProtocol::Messages, ToolChoice::Auto) => json!("auto"),
        (ProviderProtocol::Messages, ToolChoice::Required) => json!({"type": "any"}),
        (ProviderProtocol::Messages, ToolChoice::Specific(name)) => {
            json!({"type": "tool", "name": name})
        }
        (ProviderProtocol::ChatCompletions, ToolChoice::Auto) => json!("auto"),
        (ProviderProtocol::ChatCompletions, ToolChoice::Required) => json!("required"),
        (ProviderProtocol::ChatCompletions, ToolChoice::Specific(name)) => {
            json!({"type": "function", "function": {"name": name}})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tool_sets_have_no_wire_fallback() {
        assert!(translate(&[], ToolChoice::Auto, ProviderProtocol::Responses).is_none());
    }

    #[test]
    fn named_tool_choice_is_native_for_every_protocol() {
        let spec = Tool::NoOp.spec(true);
        for protocol in [
            ProviderProtocol::ChatCompletions,
            ProviderProtocol::Messages,
            ProviderProtocol::Responses,
        ] {
            let translated = translate(
                std::slice::from_ref(&spec),
                ToolChoice::Specific(TOOL_NOOP),
                protocol,
            )
            .expect("non-empty native tool set");
            assert!(translated.tool_block.is_array());
            assert!(!translated.choice_block.is_null());
        }
    }
}
