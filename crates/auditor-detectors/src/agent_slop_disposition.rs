#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParityRelation {
    ExactParity,
    PrecisionExtension,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AgentSlopDisposition {
    pub donor_rule: &'static str,
    pub ea_detector: &'static str,
    pub relation: ParityRelation,
}

macro_rules! disposition {
    ($rule:literal, $detector:literal, $relation:ident) => {
        AgentSlopDisposition {
            donor_rule: $rule,
            ea_detector: $detector,
            relation: ParityRelation::$relation,
        }
    };
}

pub(crate) const AGENT_SLOP_DISPOSITIONS: &[AgentSlopDisposition] = &[
    disposition!(
        "structural.local_imports",
        "function_local_import",
        ExactParity
    ),
    disposition!(
        "information.section_comments",
        "section_divider_comment",
        ExactParity
    ),
    disposition!(
        "information.magic_literals",
        "magic_literal_cluster",
        ExactParity
    ),
    disposition!(
        "information.volume",
        "halstead_information_density",
        ExactParity
    ),
    disposition!(
        "information.difficulty",
        "halstead_information_density",
        ExactParity
    ),
    disposition!("lexical.verbosity", "identifier_verbosity", ExactParity),
    disposition!("lexical.tersity", "identifier_tersity", ExactParity),
    disposition!("lexical.stutter", "identifier_stutter", PrecisionExtension),
    disposition!("structural.god_module", "god_module", ExactParity),
    disposition!(
        "structural.redundancy",
        "sibling_call_redundancy",
        PrecisionExtension
    ),
    disposition!(
        "structural.complexity.cognitive",
        "cognitive_complexity",
        ExactParity
    ),
    disposition!(
        "structural.complexity.cyclomatic",
        "function_complexity",
        ExactParity
    ),
    disposition!(
        "structural.complexity.npath",
        "npath_complexity",
        ExactParity
    ),
    disposition!(
        "structural.class.complexity",
        "class_structure_metrics",
        ExactParity
    ),
    disposition!(
        "structural.class.coupling",
        "class_structure_metrics",
        ExactParity
    ),
    disposition!(
        "structural.class.inheritance.depth",
        "class_structure_metrics",
        ExactParity
    ),
    disposition!(
        "structural.class.inheritance.children",
        "class_structure_metrics",
        ExactParity
    ),
    disposition!(
        "structural.types.sentinels",
        "stringly_typed_parameter",
        PrecisionExtension
    ),
    disposition!(
        "structural.types.hidden_mutators",
        "collection_parameter_mutation",
        ExactParity
    ),
    disposition!(
        "structural.types.escape_hatches",
        "escape_hatch_type_density",
        ExactParity
    ),
    disposition!(
        "structural.duplication",
        "source_duplicate_detection",
        ExactParity
    ),
    disposition!("structural.deps", "repo_graph_import_cycle", ExactParity),
    disposition!(
        "structural.packages",
        "package_coupling_metrics",
        ExactParity
    ),
    disposition!(
        "structural.hotspots",
        "churn_complexity_hotspot",
        ExactParity
    ),
];
