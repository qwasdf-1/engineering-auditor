//! Executable detector-language support policies.

use crate::{DetectorLanguageSupport, Language, LanguageSupportMode};

#[path = "registry_language_sets.rs"]
mod sets;
use sets::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SupportPolicy {
    AllSource(LanguageSupportMode),
    Documentation(LanguageSupportMode),
    RepositoryMetadata,
    Selected {
        languages: &'static [&'static str],
        mode: LanguageSupportMode,
        other: LanguageSupportMode,
    },
    Unclassified,
}

#[derive(Clone, Copy)]
struct PolicyRule {
    ids: &'static [&'static str],
    policy: SupportPolicy,
}

const fn selected(
    languages: &'static [&'static str],
    mode: LanguageSupportMode,
    other: LanguageSupportMode,
) -> SupportPolicy {
    SupportPolicy::Selected {
        languages,
        mode,
        other,
    }
}

const SUPPORT_RULES: &[PolicyRule] = &[
    PolicyRule {
        ids: &["generated_vs_source"],
        policy: selected(
            MARKDOWN,
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["large_file"],
        policy: SupportPolicy::AllSource(LanguageSupportMode::ExactLexical),
    },
    PolicyRule {
        ids: DOCUMENTATION_IDS,
        policy: SupportPolicy::Documentation(LanguageSupportMode::ExactLexical),
    },
    PolicyRule {
        ids: REPOSITORY_METADATA_IDS,
        policy: SupportPolicy::RepositoryMetadata,
    },
    PolicyRule {
        ids: SEMANTIC_IDS,
        policy: SupportPolicy::AllSource(LanguageSupportMode::Semantic),
    },
    PolicyRule {
        ids: SYSTEMS_EFFECT_IDS,
        policy: selected(
            TIER_ONE_CSHARP,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: SYSTEMS_TOPOLOGY_IDS,
        policy: selected(
            TIER_ONE,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: SYSTEMS_SEMANTIC_IDS,
        policy: selected(
            &[
                "rust",
                "python",
                "javascript",
                "typescript",
                "java",
                "csharp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["npath_complexity"],
        policy: selected(
            NPATH_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["halstead_information_density"],
        policy: selected(
            HALSTEAD_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["excessive_nesting_depth"],
        policy: selected(
            FLOW_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["duplicate_import"],
        policy: selected(
            &[
                "python",
                "javascript",
                "typescript",
                "java",
                "kotlin",
                "csharp",
                "c",
                "cpp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["identifier_stutter"],
        policy: selected(
            &["rust", "go"],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["out_parameter"],
        policy: selected(
            &["java"],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["class_structure_metrics"],
        policy: selected(
            &[
                "python",
                "javascript",
                "typescript",
                "java",
                "kotlin",
                "csharp",
                "cpp",
            ],
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["too_many_parameters"],
        policy: selected(
            FLOW_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["god_module"],
        policy: selected(
            GOD_MODULE_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["too_many_locals"],
        policy: selected(
            LOCAL_METRIC_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["too_many_returns", "too_many_statements"],
        policy: selected(
            STATEMENT_METRIC_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: RUST_PYTHON_PARTIAL_IDS,
        policy: selected(
            RUST_PYTHON,
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["source_duplicate_detection"],
        policy: selected(
            &["rust", "python", "javascript", "typescript", "go", "c"],
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["orphan_symbol"],
        policy: selected(
            RUST,
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: RUST_CORRECTNESS_SEMANTIC_IDS,
        policy: selected(
            RUST,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: PYTHON_JAVA_CORRECTNESS_SEMANTIC_IDS,
        policy: selected(
            &["python", "java", "kotlin", "csharp"],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: API_IDENTITY_SEMANTIC_IDS,
        policy: selected(
            TIERED,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        // Rust releases resources by RAII.
        ids: &["resource_not_closed"],
        policy: selected(
            &[
                "python",
                "go",
                "java",
                "javascript",
                "typescript",
                "kotlin",
                "csharp",
                "c",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: TIER_ONE_CORRECTNESS_SEMANTIC_IDS,
        policy: selected(
            TIERED,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["unsafe_deserialization", "eval_in_production"],
        policy: selected(
            &[
                "python",
                "java",
                "javascript",
                "typescript",
                "kotlin",
                "csharp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["xml_xxe_vulnerable"],
        policy: selected(
            &[
                "python",
                "java",
                "javascript",
                "typescript",
                "kotlin",
                "csharp",
                "c",
                "cpp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: LITERAL_SECURITY_SEMANTIC_IDS,
        policy: selected(
            TIERED,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: PYTHON_JS_CORRECTNESS_SEMANTIC_IDS,
        policy: selected(
            &[
                "python",
                "javascript",
                "typescript",
                "java",
                "kotlin",
                "csharp",
                "cpp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: BRACE_CORRECTNESS_SEMANTIC_IDS,
        policy: selected(
            &[
                "rust",
                "javascript",
                "typescript",
                "java",
                "csharp",
                "c",
                "cpp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["empty_conditional_branch"],
        policy: selected(
            TIERED,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: RUST_PYTHON_JS_CORRECTNESS_SEMANTIC_IDS,
        policy: selected(
            &[
                "rust",
                "python",
                "javascript",
                "typescript",
                "csharp",
                "c",
                "cpp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["sql_injection"],
        policy: selected(
            &[
                "rust",
                "python",
                "javascript",
                "typescript",
                "go",
                "java",
                "kotlin",
                "csharp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["unused_assignment"],
        policy: selected(
            TIERED,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["eager_fallback_computation"],
        policy: selected(
            &["rust", "java", "csharp"],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["immutable_string_concatenation_in_loop"],
        policy: selected(
            &[
                "rust",
                "python",
                "javascript",
                "typescript",
                "go",
                "java",
                "kotlin",
                "csharp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["cache_miss_opportunity", "loop_invariant_computation"],
        policy: selected(
            TIER_ONE_CSHARP,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["loop_invariant_regex_compilation"],
        policy: selected(
            TIER_ONE_CSHARP,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["bulk_collection_extension"],
        policy: selected(
            TIER_ONE_CSHARP,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["repeated_format_allocation"],
        policy: selected(
            TIER_ONE_CSHARP,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["manual_collection_copy_loop"],
        policy: selected(
            TIER_ONE_CSHARP,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["map_entry_opportunity"],
        policy: selected(
            &["rust", "python", "java", "csharp"],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["intermediate_collection_materialization"],
        policy: selected(
            &[
                "rust",
                "python",
                "javascript",
                "typescript",
                "java",
                "kotlin",
                "csharp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["range_for_each_iteration"],
        policy: selected(
            &["rust", "java", "kotlin", "csharp"],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["known_size_preallocation"],
        policy: selected(
            &["rust", "go", "java", "csharp"],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["no_await_in_loop"],
        policy: selected(
            &["rust", "python", "javascript", "typescript", "csharp"],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["require_atomic_updates"],
        policy: selected(
            JAVASCRIPT_TYPESCRIPT,
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["unreachable_code", "constant_condition"],
        policy: selected(
            FLOW_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: PYTHON_IDS,
        policy: selected(
            PYTHON,
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["cpp_raw_ptr_ownership"],
        policy: selected(
            &["cpp"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    },
    PolicyRule {
        ids: &["ruby_double_bang"],
        policy: selected(
            &["ruby"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    },
    PolicyRule {
        ids: &["csharp_linq_enumeration"],
        policy: selected(
            &["csharp"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    },
    PolicyRule {
        ids: KOTLIN_IDS,
        policy: selected(
            &["kotlin"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    },
    PolicyRule {
        ids: FRONT_REMOVAL_IDS,
        policy: selected(
            &[
                "rust",
                "python",
                "javascript",
                "typescript",
                "java",
                "csharp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: REGEX_BACKTRACKING_IDS,
        policy: selected(
            &["python", "javascript", "typescript", "java", "csharp"],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: DISCARDED_RESULT_IDS,
        policy: selected(
            TIER_ONE,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: UNUSED_DEPENDENCY_IDS,
        policy: selected(
            TIER_ONE,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: NETWORK_TIMEOUT_IDS,
        policy: selected(
            TIER_ONE,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: TEST_QUALITY_IDS,
        policy: selected(
            TIER_ONE,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: GO_IDS,
        policy: selected(
            &["go"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    },
    PolicyRule {
        ids: &["java_inefficient_string_buffer"],
        policy: selected(
            &["java"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    },
    PolicyRule {
        ids: RUST_ONLY_IDS,
        policy: selected(
            RUST,
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: ALGORITHMIC_IDS,
        policy: selected(
            TIER_ONE_CSHARP,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["nested_quadratic_iteration"],
        policy: selected(
            MANAGED_LOOP_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["repeated_recursive_subproblem"],
        policy: selected(
            RECURSIVE_LANGUAGES,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["n_plus_one_query_in_loop"],
        policy: selected(
            TIER_ONE_CSHARP,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["loop_carried_copy_accumulation"],
        policy: selected(
            &[
                "rust",
                "python",
                "javascript",
                "typescript",
                "java",
                "csharp",
            ],
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["nested_cross_collection_traversal"],
        policy: selected(
            TIER_ONE_CSHARP,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["invariant_collection_copy_in_loop"],
        policy: selected(
            RUST_PYTHON,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["section_divider_comment"],
        policy: selected(
            TIERED,
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["magic_literal_cluster"],
        policy: selected(
            TIER_ONE,
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["stringly_typed_parameter"],
        policy: selected(
            &["python", "javascript", "typescript", "java"],
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["identifier_tersity", "identifier_verbosity"],
        policy: selected(
            PYTHON,
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["function_local_import"],
        policy: selected(
            PYTHON,
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["sibling_call_redundancy"],
        policy: selected(
            PYTHON,
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["escape_hatch_type_density"],
        policy: selected(
            &["python", "typescript"],
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["collection_parameter_mutation"],
        policy: selected(
            &["python", "typescript", "java", "kotlin", "csharp"],
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
    PolicyRule {
        ids: &["naming_convention"],
        policy: selected(
            &[
                "rust",
                "python",
                "javascript",
                "typescript",
                "go",
                "java",
                "kotlin",
                "csharp",
            ],
            LanguageSupportMode::Partial,
            LanguageSupportMode::Unsupported,
        ),
    },
];

pub(crate) fn support_policy_for(id: &str) -> SupportPolicy {
    SUPPORT_RULES
        .iter()
        .find(|rule| rule.ids.contains(&id))
        .map(|rule| rule.policy)
        .unwrap_or(SupportPolicy::Unclassified)
}

#[cfg(test)]
pub(super) fn support_policy_count(id: &str) -> usize {
    SUPPORT_RULES
        .iter()
        .filter(|rule| rule.ids.contains(&id))
        .count()
}

/// The policy mode of `(detector, language)` before any reviewed disposition.
#[cfg(test)]
pub(super) fn policy_mode(detector_id: &str, language: &str) -> LanguageSupportMode {
    mode_for(language, support_policy_for(detector_id))
}

#[cfg(test)]
pub(super) const fn catalog_languages() -> &'static [&'static str] {
    CATALOG_LANGUAGES
}

pub(crate) fn language_support(
    detector_id: &'static str,
    policy: SupportPolicy,
) -> Vec<DetectorLanguageSupport> {
    CATALOG_LANGUAGES
        .iter()
        .map(|language| claim_for(detector_id, language, policy))
        .collect()
}

fn claim_for(
    detector_id: &'static str,
    language: &'static str,
    policy: SupportPolicy,
) -> DetectorLanguageSupport {
    let mode = mode_for(language, policy);
    if mode == LanguageSupportMode::Unsupported {
        if let Some(disposition) = super::cell_dispositions::cell_disposition(detector_id, language)
        {
            return DetectorLanguageSupport {
                language,
                mode: disposition.mode,
                limitation: Some(disposition.reason),
            };
        }
    }
    DetectorLanguageSupport {
        language,
        mode,
        limitation: limitation_for(detector_id, language, mode),
    }
}

fn mode_for(language: &str, policy: SupportPolicy) -> LanguageSupportMode {
    match policy {
        SupportPolicy::AllSource(mode) if SOURCE_LANGUAGES.contains(&language) => mode,
        SupportPolicy::Documentation(mode) if language == "markdown" => mode,
        SupportPolicy::RepositoryMetadata => LanguageSupportMode::NotApplicable,
        SupportPolicy::Selected {
            languages, mode, ..
        } if languages.contains(&language) => mode,
        SupportPolicy::Selected { other, .. } if SOURCE_LANGUAGES.contains(&language) => other,
        _ => LanguageSupportMode::Unsupported,
    }
}

pub(super) fn supports_language(policy: SupportPolicy, language: &Language) -> bool {
    if policy == SupportPolicy::RepositoryMetadata {
        return true;
    }
    let key = match language {
        Language::Json | Language::Toml | Language::Yaml => "structured",
        other => other.as_str(),
    };
    !matches!(
        mode_for(key, policy),
        LanguageSupportMode::Unsupported | LanguageSupportMode::NotApplicable
    )
}

fn limitation_for(
    detector_id: &str,
    language: &str,
    mode: LanguageSupportMode,
) -> Option<&'static str> {
    match mode {
        LanguageSupportMode::Partial => {
            Some("Heuristic coverage; see the detector capability case for its supported boundary.")
        }
        LanguageSupportMode::Unsupported => Some(unsupported_reason(detector_id, language)),
        LanguageSupportMode::NotApplicable => {
            Some("A language-specific or repository-metadata detector: it reads another language's construct, or the repository as a whole.")
        }
        _ => None,
    }
}

fn unsupported_reason(detector_id: &str, language: &str) -> &'static str {
    match classify_unsupported_reason(detector_id, language) {
        UnsupportedReason::NonSource => "The detector consumes source semantics; documentation and structured data are outside its input contract.",
        UnsupportedReason::Algorithmic => "The frontend lacks a qualified collection, cardinality, mutation, or call-cost proof for this algorithmic capability.",
        UnsupportedReason::Optimization => "The frontend lacks a qualified receiver type, alias, evaluation-order, or failure-behavior proof for this optimization.",
        UnsupportedReason::Systems => "The frontend lacks a qualified effect, lifecycle, or resolved-topology classifier for this systems capability.",
        UnsupportedReason::FlowMetric => "The grammar frontend does not yet normalize the required declaration or statement metric with matching boundaries.",
        UnsupportedReason::Security => "No qualified language-specific API identity and dataflow boundary exists for this security capability.",
        UnsupportedReason::Generic => "No grammar-backed capability and adversarial negative boundary has been qualified for this detector-language pair.",
    }
}

#[derive(Clone, Copy)]
enum UnsupportedReason {
    NonSource,
    Algorithmic,
    Optimization,
    Systems,
    FlowMetric,
    Security,
    Generic,
}

fn classify_unsupported_reason(detector_id: &str, language: &str) -> UnsupportedReason {
    if !SOURCE_LANGUAGES.contains(&language) {
        return UnsupportedReason::NonSource;
    }
    if ALGORITHMIC_REASON_IDS.contains(&detector_id) {
        return UnsupportedReason::Algorithmic;
    }
    if OPTIMIZATION_IDS.contains(&detector_id) {
        return UnsupportedReason::Optimization;
    }
    if [
        SYSTEMS_SEMANTIC_IDS,
        SYSTEMS_EFFECT_IDS,
        SYSTEMS_TOPOLOGY_IDS,
    ]
    .iter()
    .any(|ids| ids.contains(&detector_id))
    {
        return UnsupportedReason::Systems;
    }
    if FLOW_LANGUAGES.contains(&language) && FLOW_METRIC_IDS.contains(&detector_id) {
        return UnsupportedReason::FlowMetric;
    }
    if SECURITY_ID_MARKERS
        .iter()
        .any(|marker| detector_id.contains(marker))
    {
        UnsupportedReason::Security
    } else {
        UnsupportedReason::Generic
    }
}
