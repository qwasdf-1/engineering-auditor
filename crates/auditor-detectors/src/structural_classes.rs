use super::*;

pub(crate) fn class_structure_metrics_findings(index: &StructureIndex) -> Vec<Finding> {
    let mut findings = Vec::new();
    let classes = index
        .files()
        .iter()
        .enumerate()
        .flat_map(|(file_index, file)| {
            file.symbols
                .iter()
                .enumerate()
                .filter(|(_, symbol)| matches!(symbol.kind, SymbolKind::Class | SymbolKind::Object))
                .map(move |(symbol_index, symbol)| (file_index, symbol_index, symbol))
        })
        .collect::<Vec<_>>();
    let class_names = classes
        .iter()
        .map(|(_, _, symbol)| symbol.name.as_str())
        .collect::<BTreeSet<_>>();
    for (file_index, symbol_index, class) in &classes {
        let file = &index.files()[*file_index];
        // A Kotlin interface or `expect` class declares an API surface that
        // its implementations carry.
        let declares_only = file.language == Language::Kotlin
            && class.declaration.as_deref().is_some_and(|header| {
                header
                    .split_whitespace()
                    .any(|word| matches!(word, "interface" | "expect"))
            });
        if !file.emittable || class.in_test || declares_only {
            continue;
        }
        let class_owner = *symbol_index as u32;
        let methods = file
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| {
                matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
                    && symbol.parent == Some(class_owner)
            })
            .collect::<Vec<_>>();
        let method_owners = methods
            .iter()
            .map(|(index, _)| *index as u32)
            .collect::<BTreeSet<_>>();
        let wmc = methods.len()
            + file
                .controls
                .iter()
                .filter(|control| {
                    control
                        .owner
                        .is_some_and(|owner| method_owners.contains(&owner))
                })
                .count();
        let cbo = class_names
            .iter()
            .filter(|name| **name != class.name)
            .filter(|name| {
                file.references.iter().any(|reference| {
                    reference.name == **name
                        && reference.kind != auditor_ast::ReferenceKind::Import
                        && reference.owner.is_some_and(|owner| {
                            owner == class_owner || method_owners.contains(&owner)
                        })
                        && class.span.start_byte <= reference.span.start_byte
                        && reference.span.end_byte <= class.span.end_byte
                })
            })
            .count();
        let dit = inheritance_depth(*file_index, class, &classes);
        let noc = classes
            .iter()
            .filter(|(candidate_file, _, candidate)| {
                candidate.base.as_deref().is_some_and(|base| {
                    resolve_class(*candidate_file, base, &classes).is_some_and(
                        |(base_file, base_symbol, _)| {
                            (*base_file, *base_symbol) == (*file_index, *symbol_index)
                        },
                    )
                })
            })
            .count();
        // A method-less leaf deep in a hierarchy (an exception taxonomy) adds
        // no behaviour, so its depth alone is not a design smell; a base built
        // for extension has many children by design.
        if wmc <= CLASS_WMC_THRESHOLD
            && cbo <= CLASS_CBO_THRESHOLD
            && (dit <= CLASS_DIT_THRESHOLD || wmc == 0)
            && (noc <= CLASS_NOC_THRESHOLD || is_extension_point(class))
        {
            continue;
        }
        let message = format!(
            "Class `{}` exceeds a CK/WMC budget (WMC={wmc}, CBO={cbo}, DIT={dit}, NOC={noc}).",
            class.name
        );
        findings.push(
            structural_finding(&CLASS_STRUCTURE, message, file, class.line)
                .with_evidence("class", class.name.clone())
                .with_evidence("wmc", wmc)
                .with_evidence("cbo", cbo)
                .with_evidence("dit", dit)
                .with_evidence("noc", noc),
        );
    }
    findings
}

/// A class declared for subclassing: abstract (`abstract class`, an `ABC`
/// or `Protocol` base, an `ABCMeta` metaclass) or a mixin. Chidamber and
/// Kemerer read a high NOC on such a base as reuse, not as a defect.
fn is_extension_point(class: &StructureSymbol) -> bool {
    let header = class.declaration.as_deref().unwrap_or_default();
    class.name.ends_with("Mixin")
        || crate::util::identifiers(header)
            .iter()
            .any(|word| matches!(word.as_str(), "abstract" | "ABC" | "ABCMeta" | "Protocol"))
}

pub(crate) fn orphan_symbol_findings(index: &StructureIndex) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (file_index, file) in index.files().iter().enumerate() {
        if !file.emittable || file.language != Language::Rust {
            continue;
        }
        for (symbol_index, symbol) in file.symbols.iter().enumerate() {
            if symbol.parent.is_some()
                || symbol.is_public
                || symbol.in_test
                || symbol.implicitly_used
                || symbol.name.len() < 4
                || !matches!(
                    symbol.kind,
                    SymbolKind::Constant
                        | SymbolKind::Static
                        | SymbolKind::TypeAlias
                        | SymbolKind::Union
                )
                || index.reference_resolution(file_index, symbol_index)
                    != ReferenceResolution::Unreferenced
            {
                continue;
            }
            findings.push(
                auditor_core::finding!(
                    "orphan_symbol",
                    "structural",
                    "Unreferenced symbol",
                    format!(
                        "Symbol `{}` defined at line {} has zero resolved source references beyond its definition",
                        symbol.name, symbol.line
                    ),
                    Severity::Medium,
                    Confidence::High,
                    &file.path,
                    Some(symbol.line),
                )
                .with_recommendation(format!(
                    "Verify that `{}` is intended for external use or remove it if orphaned.",
                    symbol.name
                ))
                .with_evidence("symbol", symbol.name.clone())
                .with_evidence("symbol_kind", symbol_kind_name(symbol.kind))
                .with_evidence("resolution", "unreferenced"),
            );
        }
    }
    findings
}

const CLASS_STRUCTURE: FindingText = FindingText {
    detector: "class_structure_metrics",
    title: "Class structure exceeds metric budget",
    recommendation: "Reduce class responsibility, inheritance depth, or coupling when the class owns unrelated behavior.",
};
