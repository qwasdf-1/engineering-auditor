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

const fn rule(ids: &'static [&'static str], policy: SupportPolicy) -> PolicyRule {
    PolicyRule { ids, policy }
}

/// Grammar-backed in `languages`; unsupported in the other source languages.
const fn semantic(languages: &'static [&'static str]) -> SupportPolicy {
    selected(
        languages,
        LanguageSupportMode::Semantic,
        LanguageSupportMode::Unsupported,
    )
}

/// Heuristic in `languages`; unsupported in the other source languages.
const fn partial(languages: &'static [&'static str]) -> SupportPolicy {
    selected(
        languages,
        LanguageSupportMode::Partial,
        LanguageSupportMode::Unsupported,
    )
}

const SUPPORT_RULES: &[PolicyRule] = &[
    rule(
        &["generated_vs_source"],
        selected(
            MARKDOWN,
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::Unsupported,
        ),
    ),
    rule(
        &["large_file"],
        SupportPolicy::AllSource(LanguageSupportMode::ExactLexical),
    ),
    rule(
        DOCUMENTATION_IDS,
        SupportPolicy::Documentation(LanguageSupportMode::ExactLexical),
    ),
    rule(REPOSITORY_METADATA_IDS, SupportPolicy::RepositoryMetadata),
    rule(
        SEMANTIC_IDS,
        SupportPolicy::AllSource(LanguageSupportMode::Semantic),
    ),
    rule(SYSTEMS_EFFECT_IDS, semantic(TIER_ONE_CSHARP_CPP_C)),
    rule(SYSTEMS_CACHE_IDS, semantic(TIER_ONE_CSHARP_CPP)),
    rule(SYSTEMS_ARTIFACT_IDS, semantic(TIER_ONE_CSHARP)),
    rule(SYSTEMS_TOPOLOGY_IDS, semantic(TIER_ONE)),
    rule(
        SYSTEMS_SEMANTIC_IDS,
        semantic(&[
            "rust",
            "python",
            "javascript",
            "typescript",
            "java",
            "csharp",
        ]),
    ),
    rule(&["npath_complexity"], semantic(NPATH_LANGUAGES)),
    rule(
        &["halstead_information_density"],
        semantic(HALSTEAD_LANGUAGES),
    ),
    rule(&["excessive_nesting_depth"], semantic(FLOW_LANGUAGES)),
    rule(
        &["duplicate_import"],
        semantic(&[
            "python",
            "javascript",
            "typescript",
            "java",
            "kotlin",
            "csharp",
            "c",
            "cpp",
        ]),
    ),
    rule(&["identifier_stutter"], semantic(&["rust", "go"])),
    rule(&["out_parameter"], semantic(&["java"])),
    rule(
        &["class_structure_metrics"],
        partial(&[
            "python",
            "javascript",
            "typescript",
            "java",
            "kotlin",
            "csharp",
            "cpp",
        ]),
    ),
    rule(&["too_many_parameters"], semantic(FLOW_LANGUAGES)),
    rule(&["god_module"], semantic(GOD_MODULE_LANGUAGES)),
    rule(&["too_many_locals"], semantic(LOCAL_METRIC_LANGUAGES)),
    rule(
        &["too_many_returns", "too_many_statements"],
        semantic(STATEMENT_METRIC_LANGUAGES),
    ),
    rule(RUST_PYTHON_PARTIAL_IDS, partial(RUST_PYTHON)),
    rule(
        &["source_duplicate_detection"],
        partial(&["rust", "python", "javascript", "typescript", "go", "c"]),
    ),
    rule(&["orphan_symbol"], partial(RUST)),
    rule(RUST_CORRECTNESS_SEMANTIC_IDS, semantic(RUST)),
    rule(
        PYTHON_JAVA_CORRECTNESS_SEMANTIC_IDS,
        semantic(&["python", "java", "kotlin", "csharp"]),
    ),
    rule(API_IDENTITY_SEMANTIC_IDS, semantic(TIERED)),
    // Rust releases resources by RAII.
    rule(
        &["resource_not_closed"],
        semantic(&[
            "python",
            "go",
            "java",
            "javascript",
            "typescript",
            "kotlin",
            "csharp",
            "c",
        ]),
    ),
    rule(TIER_ONE_CORRECTNESS_SEMANTIC_IDS, semantic(TIERED)),
    rule(
        &["unsafe_deserialization", "eval_in_production"],
        semantic(&[
            "python",
            "java",
            "javascript",
            "typescript",
            "kotlin",
            "csharp",
        ]),
    ),
    rule(
        &["xml_xxe_vulnerable"],
        semantic(&[
            "python",
            "java",
            "javascript",
            "typescript",
            "kotlin",
            "csharp",
            "c",
            "cpp",
        ]),
    ),
    rule(LITERAL_SECURITY_SEMANTIC_IDS, semantic(TIERED)),
    rule(
        PYTHON_JS_CORRECTNESS_SEMANTIC_IDS,
        semantic(&[
            "python",
            "javascript",
            "typescript",
            "java",
            "kotlin",
            "csharp",
            "cpp",
        ]),
    ),
    rule(
        BRACE_CORRECTNESS_SEMANTIC_IDS,
        semantic(&[
            "rust",
            "javascript",
            "typescript",
            "java",
            "csharp",
            "c",
            "cpp",
        ]),
    ),
    rule(&["empty_conditional_branch"], semantic(TIERED)),
    rule(
        RUST_PYTHON_JS_CORRECTNESS_SEMANTIC_IDS,
        semantic(&[
            "rust",
            "python",
            "javascript",
            "typescript",
            "csharp",
            "c",
            "cpp",
        ]),
    ),
    rule(
        &["sql_injection"],
        semantic(&[
            "rust",
            "python",
            "javascript",
            "typescript",
            "go",
            "java",
            "kotlin",
            "csharp",
        ]),
    ),
    rule(&["unused_assignment"], semantic(TIERED)),
    rule(
        &["eager_fallback_computation"],
        semantic(&["rust", "java", "csharp", "cpp"]),
    ),
    rule(
        &["immutable_string_concatenation_in_loop"],
        semantic(&[
            "rust",
            "python",
            "javascript",
            "typescript",
            "go",
            "java",
            "kotlin",
            "csharp",
            "cpp",
        ]),
    ),
    rule(
        &["cache_miss_opportunity", "loop_invariant_computation"],
        semantic(TIER_ONE_CSHARP_CPP_C),
    ),
    rule(
        &["loop_invariant_regex_compilation"],
        semantic(TIER_ONE_CSHARP_CPP_C),
    ),
    rule(
        &["bulk_collection_extension"],
        semantic(TIER_ONE_CSHARP_CPP),
    ),
    rule(
        &["repeated_format_allocation"],
        semantic(TIER_ONE_CSHARP_CPP),
    ),
    rule(
        &["manual_collection_copy_loop"],
        semantic(TIER_ONE_CSHARP_CPP),
    ),
    rule(
        &["map_entry_opportunity"],
        semantic(&["rust", "python", "java", "csharp", "cpp"]),
    ),
    rule(
        &["intermediate_collection_materialization"],
        semantic(&[
            "rust",
            "python",
            "javascript",
            "typescript",
            "java",
            "kotlin",
            "csharp",
        ]),
    ),
    rule(
        &["range_for_each_iteration"],
        semantic(&["rust", "java", "kotlin", "csharp"]),
    ),
    rule(
        &["known_size_preallocation"],
        semantic(&["rust", "go", "java", "csharp"]),
    ),
    rule(
        &["no_await_in_loop"],
        semantic(&["rust", "python", "javascript", "typescript", "csharp"]),
    ),
    rule(
        &["require_atomic_updates"],
        selected(
            JAVASCRIPT_TYPESCRIPT,
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::Unsupported,
        ),
    ),
    rule(
        &["unreachable_code", "constant_condition"],
        semantic(FLOW_LANGUAGES),
    ),
    rule(
        PYTHON_ONLY_SEMANTIC_IDS,
        selected(
            PYTHON,
            LanguageSupportMode::Semantic,
            LanguageSupportMode::NotApplicable,
        ),
    ),
    rule(
        PYTHON_IDS,
        selected(
            PYTHON,
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::Unsupported,
        ),
    ),
    rule(
        &["cpp_raw_ptr_ownership"],
        selected(
            &["cpp"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    ),
    rule(
        &["ruby_double_bang"],
        selected(
            &["ruby"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    ),
    rule(
        &["csharp_linq_enumeration"],
        selected(
            &["csharp"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    ),
    rule(
        KOTLIN_IDS,
        selected(
            &["kotlin"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    ),
    rule(
        FRONT_REMOVAL_IDS,
        semantic(&[
            "rust",
            "python",
            "javascript",
            "typescript",
            "java",
            "csharp",
            "cpp",
        ]),
    ),
    rule(
        REGEX_BACKTRACKING_IDS,
        semantic(&[
            "python",
            "javascript",
            "typescript",
            "java",
            "csharp",
            "cpp",
        ]),
    ),
    rule(FLOAT_EQUALITY_IDS, semantic(TIERED)),
    rule(DISCARDED_RESULT_IDS, semantic(TIER_ONE)),
    rule(UNUSED_DEPENDENCY_IDS, semantic(TIER_ONE)),
    rule(NETWORK_TIMEOUT_IDS, semantic(TIER_ONE)),
    rule(RETRY_BACKOFF_IDS, semantic(TIER_ONE)),
    rule(UNBOUNDED_CONCURRENCY_IDS, semantic(TIER_ONE)),
    rule(UNBOUNDED_QUEUE_IDS, semantic(&["rust", "python", "java"])),
    rule(
        EAGER_LOG_IDS,
        semantic(&["python", "javascript", "typescript", "go", "java"]),
    ),
    rule(TEST_QUALITY_IDS, semantic(TIER_ONE)),
    rule(SKIPPED_TEST_IDS, semantic(TIERED)),
    rule(
        GO_IDS,
        selected(
            &["go"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    ),
    rule(
        &["java_inefficient_string_buffer"],
        selected(
            &["java"],
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::NotApplicable,
        ),
    ),
    rule(
        RUST_ONLY_IDS,
        selected(
            RUST,
            LanguageSupportMode::ExactLexical,
            LanguageSupportMode::Unsupported,
        ),
    ),
    rule(SORT_IDS, semantic(TIER_ONE_CSHARP_CPP_C)),
    rule(ALGORITHMIC_IDS, semantic(TIER_ONE_CSHARP_CPP)),
    rule(&["full_sort_for_top_k"], semantic(TIER_ONE_CSHARP)),
    rule(
        &["nested_quadratic_iteration"],
        semantic(MANAGED_LOOP_LANGUAGES),
    ),
    rule(
        &["repeated_recursive_subproblem"],
        semantic(RECURSIVE_LANGUAGES),
    ),
    rule(&["n_plus_one_query_in_loop"], semantic(TIER_ONE_CSHARP_CPP)),
    rule(
        &["loop_carried_copy_accumulation"],
        semantic(&[
            "rust",
            "python",
            "javascript",
            "typescript",
            "java",
            "csharp",
        ]),
    ),
    rule(
        &["nested_cross_collection_traversal"],
        semantic(TIER_ONE_CSHARP_CPP),
    ),
    rule(
        &["invariant_collection_copy_in_loop"],
        semantic(RUST_PYTHON),
    ),
    rule(&["section_divider_comment"], partial(TIERED)),
    rule(&["magic_literal_cluster"], partial(TIER_ONE)),
    rule(
        &["stringly_typed_parameter"],
        partial(&["python", "javascript", "typescript", "java"]),
    ),
    rule(
        &["identifier_tersity", "identifier_verbosity"],
        partial(PYTHON),
    ),
    rule(&["function_local_import"], partial(PYTHON)),
    rule(&["sibling_call_redundancy"], partial(PYTHON)),
    rule(
        &["escape_hatch_type_density"],
        partial(&["python", "typescript"]),
    ),
    rule(
        &["collection_parameter_mutation"],
        partial(&["python", "typescript", "java", "kotlin", "csharp"]),
    ),
    rule(
        &["naming_convention"],
        partial(&[
            "rust",
            "python",
            "javascript",
            "typescript",
            "go",
            "java",
            "kotlin",
            "csharp",
        ]),
    ),
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
        SYSTEMS_CACHE_IDS,
        SYSTEMS_ARTIFACT_IDS,
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
