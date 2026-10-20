//! Binding of a called function's parameters to a call's arguments, for
//! effects inherited through calls. Findings-free.
//!
//! A value or artifact is expressed relative to one function: a parameter
//! by its position, receivers excluded, and anything else as a local name
//! qualified by the function. Crossing a call rebinds parameters to the
//! arguments at their positions. A named argument leaves the binding
//! unknown.

use std::collections::{BTreeMap, HashMap};

use auditor_ast::SymbolKind;

use crate::structure_index::{StructureCall, StructureFile, StructureIndex};

use super::systems_effects::{
    assignment_aliases, call_arguments, normalize_subject, simple_subject,
};

/// A value relative to one function.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Value {
    /// The function's parameter at this position.
    Param(usize),
    /// A value local to a function on the call path, qualified by it.
    Local(String),
}

impl Value {
    /// The value's name at a call site, in the caller's terms.
    pub(super) fn bind(&self, site: &Site) -> Option<String> {
        match self {
            Self::Local(token) => Some(token.clone()),
            Self::Param(position) => site.subject(*position),
        }
    }

    /// The value relative to the calling function.
    pub(super) fn through(&self, site: &Site, caller: &Scope) -> Option<Self> {
        match self {
            Self::Local(token) => Some(Self::Local(token.clone())),
            Self::Param(position) => Some(caller.value(&site.subject(*position)?)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Artifact {
    text: String,
    /// Positions of the function's parameters `text` names.
    params: Vec<usize>,
    /// `text` is exactly the parameter `params[0]`.
    is_param: bool,
    /// The artifact varies with a loop or argument on the call path.
    variant: bool,
}

/// An artifact bound to a call site.
pub(super) struct BoundArtifact<'a> {
    pub(super) text: String,
    /// The arguments the artifact depends on.
    pub(super) inputs: Vec<&'a str>,
    /// The artifact varies regardless of the caller's loop.
    pub(super) variant: bool,
}

impl Artifact {
    pub(super) fn new(text: String, scope: &Scope, variant: bool) -> Self {
        let params = parameter_positions(&text, scope.parameters);
        Self {
            is_param: params.len() == 1 && scope.parameters[params[0]] == text.trim(),
            text,
            params,
            variant,
        }
    }

    pub(super) fn bind<'a>(&self, site: &Site<'a>) -> BoundArtifact<'a> {
        let mut variant = self.variant;
        let mut inputs = Vec::new();
        for position in &self.params {
            match site.argument(*position) {
                Some(argument) => inputs.push(argument),
                None => variant = true,
            }
        }
        let text = match (self.is_param, inputs.first()) {
            (true, Some(argument)) => (*argument).to_string(),
            _ => self.text.clone(),
        };
        BoundArtifact {
            text,
            inputs,
            variant,
        }
    }

    /// The artifact relative to the calling function.
    pub(super) fn through(&self, site: &Site, caller: &Scope) -> Self {
        let bound = self.bind(site);
        let params = parameter_positions(&bound.inputs.join(", "), caller.parameters);
        Self {
            is_param: params.len() == 1 && caller.parameters[params[0]] == bound.text.trim(),
            text: bound.text,
            params,
            variant: bound.variant,
        }
    }
}

/// Per-file facts the binding needs: assignment aliases and each
/// function's parameter names.
pub(super) struct FileScopes {
    aliases: BTreeMap<String, String>,
    parameters: HashMap<u32, Vec<String>>,
}

pub(super) fn file_scopes(structure: &StructureIndex) -> Vec<FileScopes> {
    structure
        .files()
        .iter()
        .map(|file| FileScopes {
            aliases: assignment_aliases(file),
            parameters: parameters(file),
        })
        .collect()
}

/// The calling side of one call: its arguments and the caller's names.
pub(super) struct Site<'a> {
    arguments: Vec<&'a str>,
    aliases: &'a BTreeMap<String, String>,
}

impl<'a> Site<'a> {
    pub(super) fn new(call: &'a StructureCall, scopes: &'a FileScopes) -> Self {
        Self {
            arguments: call_arguments(call),
            aliases: &scopes.aliases,
        }
    }

    /// The argument bound to the parameter at `position`, unless a named
    /// argument makes the positional binding unknown.
    pub(super) fn argument(&self, position: usize) -> Option<&'a str> {
        let argument = self.arguments.get(position)?.trim();
        (!is_named(argument)).then_some(argument)
    }

    /// The caller's variable passed as the parameter at `position`.
    pub(super) fn subject(&self, position: usize) -> Option<String> {
        let argument = self.argument(position)?;
        let plain = argument
            .trim_start_matches(['&', '*'])
            .trim_start_matches("mut ")
            .trim();
        plain
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
            .then(|| simple_subject(plain))
            .flatten()
            .map(|subject| normalize_subject(&subject, self.aliases))
    }
}

/// The function a value is expressed relative to.
pub(super) struct Scope<'a> {
    parameters: &'a [String],
    name: String,
}

impl<'a> Scope<'a> {
    /// `function` is its symbol index in the file `scopes` describes.
    pub(super) fn new(scopes: &'a FileScopes, function: usize, name: String) -> Self {
        Self {
            parameters: scopes
                .parameters
                .get(&(function as u32))
                .map_or(&[], Vec::as_slice),
            name,
        }
    }

    /// Positions of the parameters `text` names.
    pub(super) fn positions(&self, text: &str) -> Vec<usize> {
        parameter_positions(text, self.parameters)
    }

    pub(super) fn value(&self, subject: &str) -> Value {
        self.parameters
            .iter()
            .position(|parameter| parameter == subject)
            .map_or_else(
                || Value::Local(format!("{}::{subject}", self.name)),
                Value::Param,
            )
    }
}

/// Each function's parameter names in order, without receivers.
fn parameters(file: &StructureFile) -> HashMap<u32, Vec<String>> {
    let mut parameters: HashMap<u32, Vec<String>> = HashMap::new();
    for symbol in &file.symbols {
        if symbol.kind != SymbolKind::Parameter
            || matches!(symbol.name.as_str(), "self" | "cls" | "this")
        {
            continue;
        }
        if let Some(parent) = symbol.parent {
            parameters
                .entry(parent)
                .or_default()
                .push(symbol.name.clone());
        }
    }
    parameters
}

fn parameter_positions(text: &str, parameters: &[String]) -> Vec<usize> {
    let identifiers = crate::util::identifiers(text);
    parameters
        .iter()
        .enumerate()
        .filter(|(_, parameter)| identifiers.contains(parameter))
        .map(|(position, _)| position)
        .collect()
}

/// A keyword or named argument (`value=raw`, `value: raw`).
fn is_named(argument: &str) -> bool {
    let bytes = argument.as_bytes();
    argument
        .char_indices()
        .any(|(index, character)| match character {
            '=' => {
                bytes.get(index + 1) != Some(&b'=')
                    && index > 0
                    && !matches!(bytes[index - 1], b'=' | b'!' | b'<' | b'>')
            }
            ':' => bytes.get(index + 1) != Some(&b':') && (index == 0 || bytes[index - 1] != b':'),
            _ => false,
        })
}
