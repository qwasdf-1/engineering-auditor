use super::*;

#[path = "structural_mutation_contract.rs"]
mod mutation_contract;
use mutation_contract::mutation_is_contract_visible;

pub(crate) fn stringly_typed_parameter_findings(index: &StructureIndex) -> Vec<Finding> {
    let sentinel_names = [
        "mode", "kind", "type", "status", "state", "action", "variant",
    ];
    let mut findings = Vec::new();
    for file in emittable_files(index) {
        for (owner, function) in function_symbols(file) {
            let parameter = child_symbols(file, owner, SymbolKind::Parameter).find(|parameter| {
                sentinel_names.contains(&parameter.name.to_ascii_lowercase().as_str())
                    && parameter
                        .declaration
                        .as_deref()
                        .is_some_and(declaration_has_string_type)
                    && has_string_sentinel_evidence(file, owner, &parameter.name)
            });
            let Some(parameter) = parameter else {
                continue;
            };
            findings.push(
                auditor_core::finding!(
                    "stringly_typed_parameter",
                    "structural",
                    "Sentinel string parameter should be typed",
                    format!(
                        "Function `{}` accepts sentinel-like string parameter `{}`.",
                        function.name, parameter.name
                    ),
                    Severity::Medium,
                    Confidence::High,
                    &file.path,
                    Some(parameter.line),
                )
                .with_recommendation(
                    "Use an enum, literal union, or dedicated value type for sentinel modes.",
                )
                .with_evidence("function", function.name.clone())
                .with_evidence("parameter", parameter.name.clone())
                .with_identity("parameter", parameter.name.clone()),
            );
        }
    }
    findings
}

fn has_string_sentinel_evidence(file: &StructureFile, owner: u32, parameter: &str) -> bool {
    let comparison_regions = file
        .controls
        .iter()
        .enumerate()
        .filter(|(_, control)| {
            control.owner == Some(owner)
                && matches!(
                    control.kind,
                    auditor_ast::ControlRegionKind::Conditional
                        | auditor_ast::ControlRegionKind::Match
                )
        })
        .filter(|(region, _)| {
            file.references.iter().any(|reference| {
                reference.owner == Some(owner)
                    && reference.name == parameter
                    && reference.control_region == Some(*region as u32)
            })
        })
        .map(|(region, _)| region as u32)
        .collect::<BTreeSet<_>>();
    let sentinels = file
        .literals
        .iter()
        .filter(|literal| {
            literal.owner == Some(owner)
                && literal
                    .control_region
                    .is_some_and(|region| comparison_regions.contains(&region))
                && is_string_literal(&literal.text)
        })
        .map(|literal| literal.text.as_str())
        .collect::<BTreeSet<_>>();
    sentinels.len() >= 2
}

fn is_string_literal(text: &str) -> bool {
    let text = text.trim_start_matches(['b', 'r', '#']);
    text.starts_with(['\'', '"', '`'])
}

pub(crate) fn function_local_import_findings(index: &StructureIndex) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in emittable_files(index) {
        let mut by_owner: BTreeMap<u32, Vec<_>> = BTreeMap::new();
        for import in file.imports.iter().filter(|import| {
            import
                .owner
                .and_then(|owner| file.symbols.get(owner as usize))
                .is_some_and(|symbol| {
                    matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
                })
                && !import.justified
        }) {
            by_owner
                .entry(import.owner.unwrap())
                .or_default()
                .push(import);
        }
        for (owner, imports) in by_owner {
            let Some(function) = file.symbols.get(owner as usize) else {
                continue;
            };
            let lines = imports
                .iter()
                .map(|import| import.line.to_string())
                .collect::<Vec<_>>();
            findings.push(
                auditor_core::finding!(
                    "function_local_import",
                    "structural",
                    "Function contains local imports",
                    format!(
                        "Function `{}` contains {} local import(s).",
                        function.name,
                        imports.len()
                    ),
                    Severity::Medium,
                    Confidence::High,
                    &file.path,
                    imports.first().map(|import| import.line),
                )
                .with_recommendation(
                    "Hoist stable imports to module scope unless lazy loading is required and documented.",
                )
                .with_evidence("function", function.name.clone())
                .with_evidence("count", imports.len())
                .with_evidence("lines", lines),
            );
        }
    }
    findings
}

pub(crate) fn duplicate_import_findings(index: &StructureIndex) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in emittable_files(index) {
        let mut imports: BTreeMap<(&str, ImportForm), Vec<&StructureImport>> = BTreeMap::new();
        for import in file.imports.iter().filter(|import| {
            import.owner.is_none() && !import.dynamic && import.form != ImportForm::Member
        }) {
            let Some(canonical) = import.canonical.as_deref() else {
                continue;
            };
            imports
                .entry((canonical, import.form))
                .or_default()
                .push(import);
        }
        for ((canonical, _), group) in imports {
            // Includes repeat each other only when one's preprocessor branch
            // encloses the other's.
            let overlapping = |left: &StructureImport, right: &StructureImport| {
                left.branches.starts_with(&right.branches)
                    || right.branches.starts_with(&left.branches)
            };
            let lines = group
                .iter()
                .filter(|import| {
                    group
                        .iter()
                        .any(|other| other.line != import.line && overlapping(import, other))
                })
                .map(|import| import.line)
                .collect::<BTreeSet<_>>();
            if lines.len() < 2 {
                continue;
            }
            findings.push(
                auditor_core::finding!(
                    "duplicate_import",
                    "structural",
                    "Module import is duplicated",
                    format!(
                        "Import `{canonical}` is repeated {} times in the same file.",
                        lines.len()
                    ),
                    Severity::Low,
                    Confidence::High,
                    &file.path,
                    lines.iter().nth(1).copied(),
                )
                .with_recommendation("Merge duplicate imports into one declaration.")
                .with_identity("import", canonical)
                .with_evidence("import", canonical)
                .with_evidence("occurrence_count", lines.len())
                .with_evidence(
                    "lines",
                    lines.iter().map(usize::to_string).collect::<Vec<_>>(),
                ),
            );
        }
    }
    findings
}

pub(crate) fn collection_parameter_mutation_findings(index: &StructureIndex) -> Vec<Finding> {
    deduplicate(
        emittable_files(index)
            .flat_map(|file| {
                file.calls
                    .iter()
                    .filter_map(|call| collection_mutation_finding(file, call))
            })
            .collect(),
    )
}

fn collection_mutation_finding(
    file: &StructureFile,
    call: &crate::structure_index::StructureCall,
) -> Option<Finding> {
    let effect = collection_mutation_effect(file, call)?;
    Some(
        parameter_effect_finding(&COLLECTION_MUTATION, file, effect)
            .with_evidence("mutation_line", call.line),
    )
}

fn collection_mutation_effect<'a>(
    file: &'a StructureFile,
    call: &crate::structure_index::StructureCall,
) -> Option<ParameterEffect<'a>> {
    const MUTATORS: &[&str] = &[
        "add", "append", "clear", "discard", "extend", "insert", "pop", "push", "put", "remove",
        "set", "sort", "update",
    ];
    // Python's in-place collection methods return None, so a consumed
    // result (`opts = options.update(...)`) means a copying API instead.
    const PYTHON_NONE_RETURNING: &[&str] = &[
        "add", "append", "clear", "discard", "extend", "insert", "remove", "sort", "update",
    ];
    const CSHARP_MUTATORS: &[&str] = &[
        "Add",
        "AddRange",
        "Clear",
        "Dequeue",
        "Enqueue",
        "Insert",
        "InsertRange",
        "Pop",
        "Push",
        "Remove",
        "RemoveAll",
        "RemoveAt",
        "RemoveRange",
        "Reverse",
        "Sort",
        "TryAdd",
    ];
    let owner = call.owner?;
    let (receiver, method) = call_receiver_and_method(call)?;
    let mutators = if file.language == Language::CSharp {
        CSHARP_MUTATORS
    } else {
        MUTATORS
    };
    if !mutators.contains(&method)
        || file.language == Language::Python
            && call.result_consumed
            && PYTHON_NONE_RETURNING.contains(&method)
    {
        return None;
    }
    let parameter = child_symbols(file, owner, SymbolKind::Parameter).find(|parameter| {
        parameter.name == receiver && !matches!(parameter.name.as_str(), "self" | "cls" | "this")
    })?;
    let function = file.symbols.get(owner as usize).unwrap_or(parameter);
    if matches!(
        file.language,
        Language::Java | Language::TypeScript | Language::Kotlin | Language::CSharp
    ) && !mutation_is_contract_visible(file, owner, function, parameter)
    {
        return None;
    }
    Some(ParameterEffect {
        function,
        parameter,
        line: function.line,
    })
}

pub(crate) fn out_parameter_findings(index: &StructureIndex) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in emittable_files(index) {
        let mut reported = BTreeSet::new();
        for assignment in &file.assignments {
            let Some(effect) = out_parameter_effect(file, assignment) else {
                continue;
            };
            // One finding per function and parameter, at its first write.
            if reported.insert((effect.function.line, effect.parameter.name.as_str())) {
                findings.push(out_parameter_finding(file, assignment, effect));
            }
        }
    }
    findings
}

fn out_parameter_finding(
    file: &StructureFile,
    assignment: &crate::structure_index::StructureAssignment,
    effect: ParameterEffect<'_>,
) -> Finding {
    parameter_effect_finding(&OUT_PARAMETER, file, effect)
        .with_evidence("write_target", assignment.target.clone())
}

fn out_parameter_effect<'a>(
    file: &'a StructureFile,
    assignment: &crate::structure_index::StructureAssignment,
) -> Option<ParameterEffect<'a>> {
    let owner = assignment.owner?;
    let parameter = child_symbols(file, owner, SymbolKind::Parameter).find(|parameter| {
        assignment_targets_parameter(&file.language, &assignment.target, &parameter.name)
    })?;
    let function = file.symbols.get(owner as usize).unwrap_or(parameter);
    if acts_as_owner(file, owner, parameter) || implements_inherited_contract(function) {
        return None;
    }
    Some(ParameterEffect {
        function,
        parameter,
        line: assignment.line,
    })
}

/// Writes that Clean Code's "change the state of the owning object" allows:
/// the parameter's type is declared in the same file, so this code owns its
/// invariants (a map rotating its own tree nodes, a node detaching a child),
/// or the function is a method of an enum, a GoF State handler acting on the
/// context it is a state of.
fn acts_as_owner(file: &StructureFile, owner: u32, parameter: &StructureSymbol) -> bool {
    let mut cursor = Some(owner);
    while let Some(index) = cursor {
        let Some(symbol) = file.symbols.get(index as usize) else {
            break;
        };
        if symbol.kind == SymbolKind::Enum {
            return true;
        }
        cursor = symbol.parent;
    }
    let Some(type_name) = parameter
        .declaration
        .as_deref()
        .and_then(|declaration| parameter_type_name(declaration, &parameter.name))
    else {
        return false;
    };
    file.symbols.iter().any(|symbol| {
        matches!(
            symbol.kind,
            SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Enum
        ) && symbol.name == type_name
    })
}

/// A Java `@Override`: the supertype's contract fixes the signature, so an
/// output buffer it defines (`Reader.read(char[] cbuf, ..)`) is not this
/// method's choice.
fn implements_inherited_contract(function: &StructureSymbol) -> bool {
    function
        .declaration
        .as_deref()
        .is_some_and(|header| header.split_whitespace().any(|word| word == "@Override"))
}

/// The simple type name in a typed parameter declaration:
/// `final Map.Entry<K, V>[] entry` -> `Entry`.
fn parameter_type_name<'a>(declaration: &'a str, name: &str) -> Option<&'a str> {
    let declared = declaration.trim().strip_suffix(name)?.trim_end();
    let declared = declared.split(['<', '[']).next()?.trim_end();
    let simple = declared.rsplit([' ', '.']).next()?.trim();
    (!simple.is_empty()).then_some(simple)
}

#[derive(Clone, Copy)]
struct ParameterEffect<'a> {
    function: &'a StructureSymbol,
    parameter: &'a StructureSymbol,
    line: usize,
}

/// What a detector of effects through a parameter reports: its text and the
/// verb phrase of its message.
struct ParameterFinding {
    text: FindingText,
    action: &'static str,
}

const COLLECTION_MUTATION: ParameterFinding = ParameterFinding {
    text: FindingText {
        detector: "collection_parameter_mutation",
        title: "Collection parameter is mutated in place",
        recommendation: "Return a new collection or make mutation explicit in the API contract.",
    },
    action: "mutates collection parameter",
};

const OUT_PARAMETER: ParameterFinding = ParameterFinding {
    text: FindingText {
        detector: "out_parameter",
        title: "Function writes through an input parameter",
        recommendation:
            "Return the computed value or make the output contract explicit in the type.",
    },
    action: "writes through parameter",
};

fn parameter_effect_finding(
    spec: &ParameterFinding,
    file: &StructureFile,
    effect: ParameterEffect<'_>,
) -> Finding {
    let message = format!(
        "Function `{}` {} `{}`.",
        effect.function.name, spec.action, effect.parameter.name
    );
    structural_finding(&spec.text, message, file, effect.line)
        .with_evidence("function", effect.function.name.clone())
        .with_evidence("parameter", effect.parameter.name.clone())
        .with_identity("parameter", effect.parameter.name.clone())
}
