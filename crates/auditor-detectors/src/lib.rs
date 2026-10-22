use std::path::Path;

use auditor_core::{
    AnalysisFile, CapabilityExecution, CapabilityExecutionStatus, DetectorContract, Finding,
    FindingProvenance, LanguageCapability, RepositoryMapArtifact, Severity,
};

#[cfg(all(test, feature = "donor-oracle"))]
mod agent_slop_disposition;
mod algorithmic;
mod algorithmic_facts;
mod api_identity;
mod assignment_shapes;
mod ast_scan;
mod async_rules;
mod bloat;
mod block_strings;
mod bound_reads;
mod braced_statement_shapes;
mod c_api_rules;
mod call_graph;
mod call_summaries;
mod churn;
mod cognitive;
pub(crate) mod config;
mod control_flow;
mod correctness;
mod csharp_api_identity;
mod csharp_api_rules;
mod dataflow_sources;
mod dead_code;
mod dead_code_semantic;
mod dead_stores;
mod dependency_manifests;
mod dependency_usage;
mod discarded_results;
mod dispatcher;
mod duplication;
mod errorprone;
mod errorprone_language_rules;
mod finding_anchor;
mod finding_via;
mod go_api_rules;
mod golang;
mod java;
mod java_api_identity;
mod java_api_rules;
mod java_async;
mod java_expression_types;
mod java_syntax;
mod jca_algorithms;
mod js_api_identity;
mod js_api_rules;
mod kotlin;
mod kotlin_api_identity;
mod kotlin_api_rules;
mod language_gate;
mod layering;
mod lexical;
mod magic_literal_code;
mod markdown;
mod naming;
mod network_timeouts;
mod optimization;
mod optimization_facts;
mod python_api_identity;
mod python_api_rules;
mod regex_ambiguity;
mod regex_backtracking;
mod registry;
mod repository_graph;
mod repository_map;
mod rust_api_identity;
mod rust_api_rules;
mod rust_test_modules;
mod scan_filters;
mod scan_indexes;
use scan_indexes::RemainingIndexUsers;
mod eager_log;
mod float_equality;
mod mutable_defaults;
mod pass_through;
mod retry_backoff;
mod security;
mod security_api_rules;
mod security_crypto;
mod security_literal_rules;
mod security_process;
mod security_random_context;
mod security_random_helpers;
mod security_rules;
mod security_sql;
mod security_sql_names;
mod security_sql_templates;
mod single_implementation;
mod source_cache;
mod source_discovery;
mod statement_shapes;
mod structural;
mod structural_semantic;
mod structure_index;
mod systems;
mod systems_binding;
mod systems_effect_classification;
mod systems_effects;
mod systems_facts;
mod systems_inherited;
mod systems_topology;
mod testing_quality;
mod testing_scope;
mod testing_skips;
mod tier_two_sql;
mod tier_two_statement_shapes;
mod unbounded_concurrency;
mod unbounded_queue;
pub(crate) mod util;

pub use config::AuditorConfig;
pub use registry::detector_registry;
pub(crate) use registry::{support_policy_for, SupportPolicy};
pub use repository_graph::{
    parse_co_edit_journal, parse_repo_graph, CoEditJournal, GraphInputs, GraphProvider,
    JsonGraphProvider, NoGraphProvider, RepoGraph, RepoGraphEdge, RepoGraphNode,
};
use scan_filters::{
    analysis_files, check_catalog_contract, finding_matches_changed_scope, invalid_detector_output,
    mark_syntax,
};
pub(crate) use source_discovery::{
    discover_audited_files_with_filter, generated_vs_source_findings, AuditedFile,
};
pub use source_discovery::{
    discover_sources, discover_sources_with_filter, language_for_path, source_role_for_path,
    AnalysisMethod, Language, ScopeFilter, SourceDiscoveryOptions, SourceFile, SourceRole,
    SOURCE_ROLE_VERSION,
};

const LARGE_FILE_ADVISORY_LINES: usize = 1000;
const LARGE_FILE_STRICT_LINES: usize = 1500;
const TOO_MANY_PARAMETERS_THRESHOLD: usize = 5;
const COMPLEXITY_THRESHOLD: usize = 9;
const SECTION_DIVIDER_THRESHOLD: usize = 2;
const MAGIC_LITERAL_THRESHOLD: usize = 3;
const L5_REPEATED_CALL_THRESHOLD: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanProfile {
    Gating,
    Full,
}

impl ScanProfile {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "gating" => Ok(Self::Gating),
            "full" => Ok(Self::Full),
            other => Err(format!("unknown scan profile `{other}`")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gating => "gating",
            Self::Full => "full",
        }
    }

    pub(crate) fn includes(self, minimum: Self) -> bool {
        matches!(
            (self, minimum),
            (Self::Full, _) | (Self::Gating, Self::Gating)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectorDefinition {
    pub id: &'static str,
    pub category: &'static str,
    pub default_severity: Severity,
    pub profile: &'static str,
    pub maturity: &'static str,
    pub layer: &'static str,
    pub emission_roles: &'static [SourceRole],
    pub context_roles: &'static [SourceRole],
    pub language_support: Vec<DetectorLanguageSupport>,
}

impl DetectorDefinition {
    fn into_contract(self) -> DetectorContract {
        DetectorContract {
            id: self.id.to_string(),
            category: self.category.to_string(),
            default_severity: self.default_severity,
            profile: self.profile.to_string(),
            maturity: self.maturity.to_string(),
            layer: self.layer.to_string(),
            emission_roles: self
                .emission_roles
                .iter()
                .map(|role| role.as_str().to_string())
                .collect(),
            context_roles: self
                .context_roles
                .iter()
                .map(|role| role.as_str().to_string())
                .collect(),
            language_support: self
                .language_support
                .into_iter()
                .map(|claim| LanguageCapability {
                    language: claim.language.to_string(),
                    mode: claim.mode.as_str().to_string(),
                    limitation: claim.limitation.map(str::to_string),
                })
                .collect(),
            finding_count: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LanguageSupportMode {
    Semantic,
    ExactLexical,
    Partial,
    Unsupported,
    NotApplicable,
}

impl LanguageSupportMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::ExactLexical => "exact_lexical",
            Self::Partial => "partial",
            Self::Unsupported => "unsupported",
            Self::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectorLanguageSupport {
    pub language: &'static str,
    pub mode: LanguageSupportMode,
    pub limitation: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct RepositoryAnalysis {
    pub files: Vec<AnalysisFile>,
    pub findings: Vec<Finding>,
    pub detectors: Vec<DetectorContract>,
    pub capability_executions: Vec<CapabilityExecution>,
    pub repository_map: Option<RepositoryMapArtifact>,
}

type FunctionSpan = auditor_ast::FunctionSpan;

struct DetectorContext<'a> {
    root: &'a Path,
    /// Files on which this owner may emit findings.
    files: &'a [AuditedFile],
    /// Files the owner may inspect to resolve repository context.
    context_files: &'a [AuditedFile],
    method: AnalysisMethod,
    config: &'a AuditorConfig,
    cache: &'a util::FileCache,
    complexity: Option<cognitive::ComplexityIndex>,
    algorithmic: Option<algorithmic_facts::AlgorithmicIndex>,
    optimization: Option<optimization_facts::OptimizationIndex>,
    systems: Option<systems_facts::SystemsIndex>,
    structure: Option<structure_index::StructureIndex>,
    /// Built with the first index that follows calls; released with
    /// `structure`.
    call_graph: Option<call_graph::CallGraph>,
    repo_ast: auditor_ast::RepoAst,
    #[cfg(feature = "graph-analysis")]
    graph_inputs: &'a GraphInputs,
}

struct RoleViews<'a> {
    all: &'a [AuditedFile],
    production: &'a [AuditedFile],
    code: &'a [AuditedFile],
    documentation: &'a [AuditedFile],
    test_evolution: &'a [AuditedFile],
    test_scope: &'a [AuditedFile],
}

impl<'a> RoleViews<'a> {
    fn new(files: &'a [AuditedFile]) -> Self {
        let production_end = files.partition_point(|file| file.role == SourceRole::Production);
        let code_end = files.partition_point(|file| {
            matches!(file.role, SourceRole::Production | SourceRole::Tooling)
        });
        let test_scope_end = files.partition_point(|file| {
            matches!(
                file.role,
                SourceRole::Production | SourceRole::Tooling | SourceRole::Test
            )
        });
        let test_evolution_end = files.partition_point(|file| {
            !matches!(file.role, SourceRole::Generated | SourceRole::Documentation)
        });
        let documentation_start =
            files.partition_point(|file| file.role != SourceRole::Documentation);
        Self {
            all: files,
            production: &files[..production_end],
            code: &files[..code_end],
            documentation: &files[documentation_start..],
            test_evolution: &files[..test_evolution_end],
            test_scope: &files[..test_scope_end],
        }
    }

    fn files(&self, applicability: registry::RoleApplicability) -> &'a [AuditedFile] {
        match applicability {
            registry::RoleApplicability::All => self.all,
            registry::RoleApplicability::Production => self.production,
            registry::RoleApplicability::Code => self.code,
            registry::RoleApplicability::Documentation => self.documentation,
            registry::RoleApplicability::TestEvolution => self.test_evolution,
            registry::RoleApplicability::TestScope => self.test_scope,
        }
    }
}

const ANALYSIS_ROLE_ORDER: &[SourceRole] = &[
    SourceRole::Production,
    SourceRole::Tooling,
    SourceRole::Test,
    SourceRole::Fixture,
    SourceRole::Benchmark,
    SourceRole::Example,
    SourceRole::Reference,
    SourceRole::Generated,
    SourceRole::Documentation,
];

fn source_role_rank(role: SourceRole) -> usize {
    ANALYSIS_ROLE_ORDER
        .iter()
        .position(|candidate| *candidate == role)
        .expect("every source role has an analysis rank")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedRange {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ScanScope {
    #[default]
    All,
    Changed {
        paths: std::collections::BTreeSet<String>,
        ranges: Vec<ChangedRange>,
    },
}

#[derive(Clone, Debug)]
pub struct AnalysisRequest {
    pub discovery: SourceDiscoveryOptions,
    pub profile: ScanProfile,
    pub scope: ScanScope,
    pub repository_map_output: Option<std::path::PathBuf>,
    pub excluded_paths: std::collections::BTreeSet<std::path::PathBuf>,
}

struct ScanRun<'a> {
    context: DetectorContext<'a>,
    role_views: RoleViews<'a>,
    active_ids: std::collections::HashSet<&'static str>,
    executed_owners: std::collections::HashSet<registry::DetectorOwner>,
    remaining_index_users: RemainingIndexUsers,
    anchors: finding_anchor::AnchorResolver<'a>,
    findings: Vec<Finding>,
    trace_owners: bool,
}

impl<'a> ScanRun<'a> {
    fn new(
        context: DetectorContext<'a>,
        role_views: RoleViews<'a>,
        active_specs: &[&registry::DetectorSpec],
        reserve_repository_map: bool,
    ) -> Self {
        Self {
            anchors: finding_anchor::AnchorResolver::new(role_views.all),
            context,
            role_views,
            active_ids: active_specs.iter().map(|spec| spec.id).collect(),
            executed_owners: std::collections::HashSet::new(),
            remaining_index_users: RemainingIndexUsers::from_specs(
                active_specs,
                reserve_repository_map,
            ),
            findings: Vec::new(),
            trace_owners: std::env::var_os("EA_TRACE_OWNERS").is_some(),
        }
    }

    fn execute(&mut self, active_spec: &registry::DetectorSpec) -> std::io::Result<()> {
        if !self.executed_owners.insert(active_spec.owner) {
            return Ok(());
        }
        self.context.files = self.role_views.files(active_spec.role_applicability);
        self.context.context_files = self.role_views.files(active_spec.context_applicability);
        let started = std::time::Instant::now();
        self.prepare_indexes(active_spec.owner);
        let owner_findings = dispatcher::run_detector_owner(active_spec.owner, &mut self.context);
        if self.trace_owners {
            eprintln!(
                "ea-owner {} findings={} elapsed_ms={}",
                active_spec.id,
                owner_findings.len(),
                started.elapsed().as_millis()
            );
        }
        let mut validated = Vec::with_capacity(owner_findings.len());
        for finding in owner_findings {
            if let Some(finding) = self.validate_finding(active_spec, finding)? {
                validated.push(finding);
            }
        }
        self.findings.extend(self.anchors.anchor_findings(
            validated,
            &mut self.context.repo_ast,
            self.context.cache,
            self.context.method,
        ));
        RemainingIndexUsers::release(
            active_spec.owner,
            &mut self.context,
            &mut self.remaining_index_users,
        );
        Ok(())
    }

    fn validate_finding(
        &self,
        active_spec: &registry::DetectorSpec,
        mut finding: Finding,
    ) -> std::io::Result<Option<Finding>> {
        if !language_gate::finding_matches_catalog(active_spec.id, &finding, self.role_views.all) {
            return Ok(None);
        }
        check_catalog_contract(active_spec, &finding)?;
        if !self.active_ids.contains(finding.detector.as_str())
            || !self.context.config.accepts_finding(&finding)
        {
            return Ok(None);
        }
        finding.severity = self
            .context
            .config
            .severity_for(&finding.detector, finding.severity);
        finding.provenance = self.finding_provenance(&finding);
        Ok(Some(finding))
    }

    fn finding_provenance(&self, finding: &Finding) -> FindingProvenance {
        let file = self
            .role_views
            .all
            .iter()
            .find(|file| file.relative == Path::new(finding.primary_path()));
        FindingProvenance {
            language: file
                .map(|file| file.language.as_str())
                .unwrap_or("repository")
                .to_string(),
            source_role: file
                .map(|file| file.role.as_str())
                .unwrap_or("repository")
                .to_string(),
            analysis_method: self.context.method.as_str().to_string(),
            extractor_version: auditor_core::EXTRACTOR_VERSION,
            origin: "native".to_string(),
            span_precision: if finding.primary_line().is_some() {
                "line"
            } else {
                "repository"
            }
            .to_string(),
        }
    }

    fn verify_indexes_released(&self) -> std::io::Result<()> {
        let all_released = self.context.algorithmic.is_none()
            && self.context.complexity.is_none()
            && self.context.optimization.is_none()
            && self.context.structure.is_none()
            && self.context.systems.is_none();
        if self.remaining_index_users.is_empty() && all_released {
            Ok(())
        } else {
            Err(invalid_detector_output(
                "scan-owned indexes were retained past their final registered owner".to_string(),
            ))
        }
    }

    fn write_repository_map(
        &mut self,
        files: &[AuditedFile],
        report_output: &Path,
        profile: ScanProfile,
    ) -> std::io::Result<RepositoryMapArtifact> {
        self.ensure_structure_index();
        let artifact = repository_map::write_repository_map(
            files,
            self.context
                .structure
                .as_ref()
                .expect("reserved repository map structure index"),
            report_output,
            profile,
            self.context.method,
        )?;
        self.release_reserved_structure();
        Ok(artifact)
    }

    fn into_findings(mut self, scope: &ScanScope) -> Vec<Finding> {
        if let ScanScope::Changed { paths, ranges } = scope {
            self.findings
                .retain(|finding| finding_matches_changed_scope(finding, paths, ranges));
        }
        self.findings.sort_by(|left, right| {
            (
                left.primary_path(),
                left.primary_line().unwrap_or(0),
                left.detector.as_str(),
                left.title.as_str(),
                left.message.as_str(),
            )
                .cmp(&(
                    right.primary_path(),
                    right.primary_line().unwrap_or(0),
                    right.detector.as_str(),
                    right.title.as_str(),
                    right.message.as_str(),
                ))
        });
        self.findings.dedup_by(|left, right| left.id == right.id);
        self.findings
    }
}

pub fn analyze_repository(
    root: &Path,
    options: SourceDiscoveryOptions,
    profile: ScanProfile,
) -> std::io::Result<RepositoryAnalysis> {
    analyze_repository_with_request(
        root,
        AnalysisRequest {
            discovery: options,
            profile,
            scope: ScanScope::All,
            repository_map_output: None,
            excluded_paths: std::collections::BTreeSet::new(),
        },
        &NoGraphProvider,
    )
}

pub fn analyze_repository_with_provider(
    root: &Path,
    options: SourceDiscoveryOptions,
    profile: ScanProfile,
    provider: &dyn GraphProvider,
) -> std::io::Result<RepositoryAnalysis> {
    analyze_repository_with_request(
        root,
        AnalysisRequest {
            discovery: options,
            profile,
            scope: ScanScope::All,
            repository_map_output: None,
            excluded_paths: std::collections::BTreeSet::new(),
        },
        provider,
    )
}

pub fn analyze_repository_with_request(
    root: &Path,
    request: AnalysisRequest,
    provider: &dyn GraphProvider,
) -> std::io::Result<RepositoryAnalysis> {
    let graph_inputs = provider
        .load(root)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    analyze_repository_run(root, request, &graph_inputs)
}

/// Builds a discovery [`ScopeFilter`] from the request's [`ScanScope`].
///
/// `ScanScope::All` produces an empty filter (no pruning). `ScanScope::Changed`
/// produces a filter that retains only the requested paths so repository-wide
/// discovery does not run before the changed-path filter would have culled the
/// result set. Line ranges are honored for emitted findings in
/// `finding_matches_changed_scope`.
fn scope_filter(scope: &ScanScope) -> ScopeFilter {
    match scope {
        ScanScope::All => ScopeFilter::default(),
        ScanScope::Changed { paths, ranges } => ScopeFilter::new(paths.clone(), ranges.clone()),
    }
}

fn analyze_repository_run(
    root: &Path,
    request: AnalysisRequest,
    _graph_inputs: &GraphInputs,
) -> std::io::Result<RepositoryAnalysis> {
    let method = request.discovery.method;
    let profile = request.profile;
    let config = AuditorConfig::from_root(root);
    let mut files =
        discover_audited_files_with_filter(root, request.discovery, &scope_filter(&request.scope))?;
    if !request.excluded_paths.is_empty() {
        let excluded = request
            .excluded_paths
            .iter()
            .filter_map(|path| absolute_existing_path(path).ok())
            .collect::<std::collections::BTreeSet<_>>();
        files.retain(|file| {
            file.absolute
                .canonicalize()
                .map_or(true, |path| !excluded.contains(&path))
        });
    }
    let mut analysis_files = analysis_files(&files, method);
    files.sort_by(|left, right| {
        (source_role_rank(left.role), &left.relative)
            .cmp(&(source_role_rank(right.role), &right.relative))
    });
    let role_views = RoleViews::new(&files);
    let cache = util::FileCache::new(&files);
    let active_specs: Vec<_> = registry::detector_catalog(profile).collect();
    let context = DetectorContext {
        root,
        files: role_views.code,
        context_files: role_views.code,
        method,
        config: &config,
        cache: &cache,
        complexity: None,
        algorithmic: None,
        optimization: None,
        systems: None,
        structure: None,
        call_graph: None,
        repo_ast: auditor_ast::RepoAst::new(),
        #[cfg(feature = "graph-analysis")]
        graph_inputs: _graph_inputs,
    };
    let mut run = ScanRun::new(
        context,
        role_views,
        &active_specs,
        request.repository_map_output.is_some(),
    );
    for active_spec in &active_specs {
        run.execute(active_spec)?;
    }
    let repository_map = request
        .repository_map_output
        .as_deref()
        .map(|output| run.write_repository_map(&files, output, profile))
        .transpose()?;
    run.verify_indexes_released()?;
    mark_syntax(&mut analysis_files, &files, &run.context.repo_ast);
    let mut findings = run.into_findings(&request.scope);
    findings.retain(|finding| {
        registry::detector_spec(&finding.detector).is_none_or(|spec| {
            spec.reports_in(profile, &finding.provenance.language)
                && spec.emits_role(&finding.provenance.source_role)
        })
    });
    let definitions = registry::detector_registry(profile);
    let capability_executions = capability_executions(&definitions, &analysis_files, _graph_inputs);
    let analysis = RepositoryAnalysis {
        files: analysis_files,
        findings,
        detectors: definitions
            .into_iter()
            .map(DetectorDefinition::into_contract)
            .collect(),
        capability_executions,
        repository_map,
    };
    Ok(analysis)
}

fn absolute_existing_path(path: &Path) -> std::io::Result<std::path::PathBuf> {
    if path.is_absolute() {
        path.canonicalize()
    } else {
        std::env::current_dir()?.join(path).canonicalize()
    }
}

fn capability_executions(
    definitions: &[DetectorDefinition],
    files: &[AnalysisFile],
    graph_inputs: &GraphInputs,
) -> Vec<CapabilityExecution> {
    definitions
        .iter()
        .map(|definition| {
            let eligible = files
                .iter()
                .filter(|file| {
                    definition
                        .emission_roles
                        .iter()
                        .any(|role| role.as_str() == file.source_role)
                })
                .collect::<Vec<_>>();
            let policy = support_policy_for(definition.id);
            let mut analyzed_files = 0;
            let mut unsupported_files = 0;
            let mut not_applicable_files = 0;
            for file in &eligible {
                let mode = definition
                    .language_support
                    .iter()
                    .find(|claim| claim.language == file.catalog_language)
                    .map(|claim| claim.mode)
                    .unwrap_or(LanguageSupportMode::Unsupported);
                match mode {
                    LanguageSupportMode::Semantic
                    | LanguageSupportMode::ExactLexical
                    | LanguageSupportMode::Partial => analyzed_files += 1,
                    LanguageSupportMode::Unsupported => unsupported_files += 1,
                    LanguageSupportMode::NotApplicable => not_applicable_files += 1,
                }
            }
            let required_inputs = required_inputs(definition.id);
            let missing_inputs = missing_inputs(&required_inputs, graph_inputs);
            // Repository-metadata detectors operate on the whole repository
            // (no per-language policy) and emit findings when they observe a
            // matching pattern across `emission_roles`. Treating their status
            // as `NotApplicable` here would make the report validator reject
            // any finding they produce, even though they ran. They are
            // considered `Executed` when any eligible file was visible.
            let repository_metadata_executed =
                matches!(policy, SupportPolicy::RepositoryMetadata) && !eligible.is_empty();
            let status = if !missing_inputs.is_empty() {
                CapabilityExecutionStatus::MissingInput
            } else if repository_metadata_executed || analyzed_files > 0 {
                CapabilityExecutionStatus::Executed
            } else if unsupported_files > 0 {
                CapabilityExecutionStatus::Unsupported
            } else {
                CapabilityExecutionStatus::NotApplicable
            };
            CapabilityExecution {
                detector_id: definition.id.to_string(),
                layer: definition.layer.to_string(),
                status,
                eligible_files: eligible.len(),
                analyzed_files,
                unsupported_files,
                not_applicable_files,
                required_inputs,
                missing_inputs,
                limitations: definition
                    .language_support
                    .iter()
                    .filter_map(|claim| claim.limitation.map(str::to_string))
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                failure_reason: None,
            }
        })
        .collect()
}

fn required_inputs(detector_id: &str) -> Vec<String> {
    #[cfg(feature = "graph-analysis")]
    {
        if matches!(
            detector_id,
            "repo_graph_isolated_source_file"
                | "repo_graph_high_fanout_source_file"
                | "repo_graph_import_cycle"
                | "large_file_split_candidate"
                | "small_file_consolidation_candidate"
        ) {
            return vec!["repo_graph".to_string()];
        }
        if detector_id == "shotgun_surgery_cluster" {
            return vec!["co_edit_journal".to_string()];
        }
    }
    let _ = detector_id;
    Vec::new()
}

fn missing_inputs(required: &[String], inputs: &GraphInputs) -> Vec<String> {
    required
        .iter()
        .filter(|input| match input.as_str() {
            "repo_graph" => inputs.repo_graph.is_none(),
            "co_edit_journal" => inputs.co_edit_journal.is_none(),
            _ => true,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod lib_test;

#[cfg(all(test, feature = "donor-oracle"))]
#[path = "donor_parity_tests.rs"]
mod donor_parity;

#[cfg(all(test, feature = "donor-oracle"))]
#[path = "oracle_runner_tests.rs"]
mod oracle_runner;
