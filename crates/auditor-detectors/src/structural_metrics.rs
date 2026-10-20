use super::*;

pub(crate) fn too_many_parameters_findings(index: &StructureIndex) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in emittable_files(index) {
        for (owner, function) in function_symbols(file) {
            // An `@overload` stub restates its implementation's signature; a
            // Kotlin `external` or C# `extern` binding mirrors a foreign ABI.
            if function.in_test || function.overload_stub || is_foreign_binding(file, function) {
                continue;
            }
            let parameters = child_symbols(file, owner, SymbolKind::Parameter)
                .filter(|parameter| !is_receiver(file, parameter))
                .collect::<Vec<_>>();
            if parameters.len() <= crate::TOO_MANY_PARAMETERS_THRESHOLD {
                continue;
            }
            findings.push(
                auditor_core::finding!(
                    "too_many_parameters",
                    "structural",
                    "Function parameter list exceeds budget",
                    format!(
                        "Function `{}` takes {} parameters (> {}).",
                        display_name(&function.name),
                        parameters.len(),
                        crate::TOO_MANY_PARAMETERS_THRESHOLD
                    ),
                    Severity::Medium,
                    Confidence::High,
                    &file.path,
                    Some(function.line),
                )
                .with_recommendation(
                    "Group related parameters into a typed params struct or object.",
                )
                .with_evidence("function", function.name.clone())
                .with_evidence("parameter_count", parameters.len())
                .with_evidence("language", file.language.as_str())
                .with_evidence("threshold", crate::TOO_MANY_PARAMETERS_THRESHOLD),
            );
        }
    }
    findings
}

fn is_foreign_binding(file: &StructureFile, function: &StructureSymbol) -> bool {
    let keyword = match file.language {
        Language::Kotlin => "external",
        Language::CSharp => "extern",
        _ => return false,
    };
    function
        .declaration
        .as_deref()
        .is_some_and(|declaration| declaration.split_whitespace().any(|word| word == keyword))
}

/// `self`, `cls`, `this`, or a C# extension method's `this T value`: the
/// value the method acts on, not an argument to it.
fn is_receiver(file: &StructureFile, parameter: &StructureSymbol) -> bool {
    matches!(parameter.name.as_str(), "self" | "cls" | "this")
        || (file.language == Language::CSharp
            && parameter
                .declaration
                .as_deref()
                .is_some_and(|declaration| declaration.split_whitespace().next() == Some("this")))
}

pub(crate) fn sibling_call_redundancy_findings(index: &StructureIndex) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in emittable_files(index) {
        for (shared, owners) in sibling_call_signatures(file) {
            if let Some(finding) = sibling_call_finding(file, shared, &owners) {
                findings.push(finding);
            }
        }
    }
    findings
}

fn sibling_call_signatures(file: &StructureFile) -> BTreeMap<Vec<String>, Vec<u32>> {
    const TRIVIAL_CALLS: &[&str] = &[
        "boolean", "error", "filter", "find", "map", "split", "reverse", "collect", "iter",
        "clone", "unwrap", "some", "ok", "err", "write", "close", "now",
    ];
    let local_functions = function_symbols(file)
        .map(|(_, function)| function.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut calls_by_owner: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    for call in &file.calls {
        // Test functions repeat setup by design.
        let Some(owner) = call.owner.filter(|owner| {
            file.symbols
                .get(*owner as usize)
                .is_some_and(|function| !function.in_test)
        }) else {
            continue;
        };
        let callee = call
            .callee
            .rsplit(['.', ':'])
            .find(|part| !part.is_empty())
            .unwrap_or(&call.callee)
            .to_ascii_lowercase();
        if !TRIVIAL_CALLS.contains(&callee.as_str()) && local_functions.contains(callee.as_str()) {
            calls_by_owner.entry(owner).or_default().insert(callee);
        }
    }
    let mut signatures: BTreeMap<Vec<String>, Vec<u32>> = BTreeMap::new();
    for (owner, calls) in calls_by_owner
        .into_iter()
        .filter(|(_, calls)| calls.len() >= 3)
    {
        signatures
            .entry(calls.into_iter().collect())
            .or_default()
            .push(owner);
    }
    signatures
}

fn sibling_call_finding(
    file: &StructureFile,
    shared: Vec<String>,
    owners: &[u32],
) -> Option<Finding> {
    if owners.len() < 2 {
        return None;
    }
    let left = file.symbols.get(owners[0] as usize)?;
    let right = file.symbols.get(owners[1] as usize)?;
    Some(
        auditor_core::finding!(
            "sibling_call_redundancy",
            "structural",
            "Sibling functions share redundant call sets",
            format!(
                "Functions `{}` and `{}` share {} non-trivial local callees.",
                left.name,
                right.name,
                shared.len()
            ),
            Severity::Medium,
            Confidence::High,
            &file.path,
            Some(left.line),
        )
        .with_recommendation(
            "Extract a shared helper only if it removes duplicated behavior without increasing indirection.",
        )
        .with_identity("pair", format!("{}+{}", left.name, right.name))
        .with_evidence("left_function", left.name.clone())
        .with_evidence("right_function", right.name.clone())
        .with_evidence("shared_count", shared.len())
        .with_evidence("shared_callees", shared)
        .with_evidence("equivalent_function_count", owners.len()),
    )
}

pub(crate) fn too_many_locals_findings(index: &StructureIndex) -> Vec<Finding> {
    function_metric_findings(
        index,
        MetricRule {
            detector: "too_many_locals",
            title: "Function has too many local variables",
            noun: "local variables",
            evidence_key: "local_count",
            threshold: 15,
            recommendation: "Extract related logic into smaller functions.",
        },
        |metric| metric.locals,
    )
}

pub(crate) fn too_many_returns_findings(index: &StructureIndex) -> Vec<Finding> {
    function_metric_findings(
        index,
        MetricRule {
            detector: "too_many_returns",
            title: "Function has too many return statements",
            noun: "return statements",
            evidence_key: "return_count",
            threshold: 5,
            recommendation:
                "Consider consolidating equivalent exits while retaining useful guard clauses.",
        },
        |metric| metric.returns,
    )
}

pub(crate) fn too_many_statements_findings(index: &StructureIndex) -> Vec<Finding> {
    function_metric_findings(
        index,
        MetricRule {
            detector: "too_many_statements",
            title: "Function body has too many statements",
            noun: "statements",
            evidence_key: "statement_count",
            threshold: 50,
            recommendation: "Break the function into smaller units.",
        },
        |metric| metric.statements,
    )
}

struct MetricRule {
    detector: &'static str,
    title: &'static str,
    noun: &'static str,
    evidence_key: &'static str,
    threshold: usize,
    recommendation: &'static str,
}

/// An anonymous function (`(issue) => { .. }`) has no name to quote.
fn display_name(name: &str) -> &str {
    if name.is_empty() {
        "<anonymous>"
    } else {
        name
    }
}

fn function_metric_findings(
    index: &StructureIndex,
    rule: MetricRule,
    metric: fn(&crate::structure_index::StructureFunctionMetrics) -> usize,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in emittable_files(index) {
        for (owner, function) in function_symbols(file) {
            if function.in_test {
                continue;
            }
            let Some(metrics) = file.function_metrics.get(&owner) else {
                continue;
            };
            let count = metric(metrics);
            if count <= rule.threshold {
                continue;
            }
            findings.push(
                auditor_core::finding!(
                    rule.detector,
                    "structural",
                    rule.title,
                    format!(
                        "Function `{}` has {} {} (> {}).",
                        display_name(&function.name),
                        count,
                        rule.noun,
                        rule.threshold
                    ),
                    Severity::Medium,
                    Confidence::High,
                    &file.path,
                    Some(function.line),
                )
                .with_recommendation(rule.recommendation)
                .with_evidence("function", function.name.clone())
                .with_evidence(rule.evidence_key, count)
                .with_evidence("threshold", rule.threshold),
            );
        }
    }
    findings
}

pub(crate) fn excessive_nesting_depth_findings(index: &StructureIndex) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in emittable_files(index) {
        let mut deepest: BTreeMap<u32, (usize, usize, usize)> = BTreeMap::new();
        for control in &file.controls {
            let Some(owner) = control.owner else {
                continue;
            };
            let entry = deepest
                .entry(owner)
                .or_insert((0, control.line, control.line));
            if control.depth > entry.0 {
                entry.0 = control.depth;
                entry.2 = control.line;
            }
        }
        for (owner, (depth, first_line, deepest_line)) in deepest {
            if depth <= NESTING_DEPTH_THRESHOLD {
                continue;
            }
            let Some(function) = file.symbols.get(owner as usize) else {
                continue;
            };
            findings.push(
                auditor_core::finding!(
                    "excessive_nesting_depth",
                    "structural",
                    "Control flow nesting exceeds budget",
                    format!(
                        "Function `{}` reaches nesting depth {depth} (> {NESTING_DEPTH_THRESHOLD}).",
                        display_name(&function.name)
                    ),
                    Severity::Medium,
                    Confidence::High,
                    &file.path,
                    Some(first_line),
                )
                .with_recommendation(
                    "Flatten the control flow with guard clauses or cohesive helpers when behavior stays clear.",
                )
                .with_evidence("function", function.name.clone())
                .with_evidence("max_depth", depth)
                .with_evidence("deepest_line", deepest_line)
                .with_evidence("threshold", NESTING_DEPTH_THRESHOLD),
            );
        }
    }
    findings
}

pub(crate) fn god_module_findings(index: &StructureIndex) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in emittable_files(index) {
        let count = file
            .symbols
            .iter()
            // Methods belong to their type, however the language attaches them
            // (Rust `impl`, Go receivers, prototype or object-literal members).
            .filter(|symbol| {
                symbol.kind == SymbolKind::Function
                    && symbol.parent.is_none_or(|parent| {
                        file.symbols
                            .get(parent as usize)
                            .is_some_and(|symbol| symbol.kind == SymbolKind::Module)
                    })
                    && !symbol.in_test
            })
            .count();
        if count <= 20 {
            continue;
        }
        findings.push(
            auditor_core::finding!(
                "god_module",
                "structural",
                "Module has too many top-level definitions",
                format!("File has {count} top-level function-like definitions (> 20)."),
                Severity::Medium,
                Confidence::High,
                &file.path,
                None,
            )
            .with_recommendation(
                "Split ownership by cohesive behavior only when coupling remains low.",
            )
            .with_evidence("definition_count", count)
            .with_evidence("threshold", 20usize),
        );
    }
    findings
}
