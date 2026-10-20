use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use auditor_detectors::{
    AnalysisMethod, AnalysisRequest, NoGraphProvider, ScanProfile, ScanScope,
    SourceDiscoveryOptions,
};
use auditor_report::{ColorMode, ExternalInput, ExternalInputMetadata, NoReviewArtifactLoader};
use auditor_review::{
    AgendaPlanner, AgendaPolicy, AgendaPolicyDecision, BenefitEvidenceMode, CandidateSelection,
    CandidateSelector, CanonicalIdentity, CaseClosure, CaseClosureReason, CaseEvidenceRequest,
    CaseExecutionState, CaseFailure, CaseInstantiationError, CaseInstantiationResult, CaseKind,
    CasePosition, CaseProviderTurn, CaseStateLedger, CaseTemplateBundle, CaseTemplateRegistry,
    ClaimConfidence, ClaimDraft, ClaimStatus, ContextualRecommendation, ContractKind, DecisionKind,
    DecisionMagnitude, DeliberationMode, DeliberationRouter, DetectorAdviceRelationship,
    DeterministicDisposition, EvidenceAttachmentAdapter, EvidenceAttachmentKind, EvidenceCompiler,
    EvidenceError, EvidenceItemKind, EvidenceLookup, EvidenceLookupRequest,
    EvidenceLookupRequestKind, EvidencePacketBudget, ExpectedBenefit, InputContract, InputError,
    InputVerificationRequest, InputVerifier, Materiality, ModeAvailability, OptionalProbeRequest,
    PlanningPriority, PolicySuppressionReason, ProbeAnswer, ProbeDecisionTarget, ProbeDefinition,
    ProbeKind, ProbeLookupPlanner, ProbePlanner, ProbeResolution, ProbeResolutionError,
    ProbeResolver, ProbeResolverRegistry, RecommendationAlternative, RecommendationDraft,
    RecommendationFamily, ResolverKind, ResolverRegistration, ReviewBrief, ReviewCandidate,
    ReviewerAdmissionContext, ReviewerCaseReview, ReviewerCaseReviewPayload, RoutePredicate,
    RoutePredicateEvidence, RouteTarget, RoutingFacts, RoutingInput, SelectionReason,
    SelectorRegistry, SingleReviewerError, SingleReviewerSession, SingleReviewerTurn, SkippedStage,
    StageSkipReason, SuppressionReason, SuppressionSignal, TrustLabel, VerificationStep,
};
use auditor_scan::{run_scan, ScanFormat, ScanRequest};
use serde_json::{json, Value};

#[derive(Clone)]
struct TypedContract {
    kind: ContractKind,
    schema_version: u32,
    supported_schema_version: u32,
    name: &'static str,
}

impl CanonicalIdentity for TypedContract {
    fn identity_fields(&self) -> Value {
        json!({"name": self.name})
    }
}

impl InputContract for TypedContract {
    fn contract_kind(&self) -> ContractKind {
        self.kind
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn supported_schema_version(&self) -> u32 {
        self.supported_schema_version
    }

    fn validate_contract(&self) -> Result<(), String> {
        (!self.name.is_empty())
            .then_some(())
            .ok_or_else(|| "typed contract name is empty".to_string())
    }
}

#[derive(Clone)]
struct RuntimeMeasurement {
    metric: &'static str,
    value: u64,
}

impl CanonicalIdentity for RuntimeMeasurement {
    fn identity_fields(&self) -> Value {
        json!({"metric": self.metric, "value": self.value})
    }
}

impl EvidenceAttachmentAdapter for RuntimeMeasurement {
    fn attachment_kind(&self) -> EvidenceAttachmentKind {
        EvidenceAttachmentKind::RuntimeMeasurement
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn supported_schema_version(&self) -> u32 {
        1
    }

    fn validate_attachment(&self) -> Result<(), String> {
        (!self.metric.is_empty())
            .then_some(())
            .ok_or_else(|| "measurement metric is empty".to_string())
    }
}

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    report: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let source = format!(
            "{}use std::process::Command;\n\
             pub fn run(a: bool, b: bool, c: bool, d: bool, e: bool, f: bool, g: bool, value: &str) -> i32 {{\n\
                 if a {{ if b {{ if c {{ if d {{ return 1; }} }} }} }}\n\
                 if e {{ if f {{ if g {{ return 2; }} }} }}\n\
                 let _ = Command::new(\"sh\").arg(\"-c\").arg(value).status();\n\
                 0\n\
             }}\n\
             pub fn repeated_sort(values: &mut Vec<i32>) {{\n\
                 for _ in 0..values.len() {{ values.sort(); }}\n\
             }}\n",
            "// deterministic large-file fixture\n".repeat(1_005)
        );
        Self::from_source(source)
    }

    fn lifecycle_only() -> Self {
        Self::from_source(
            "pub fn repeated_sort(values: &mut Vec<i32>) {\n\
                 for _ in 0..values.len() { values.sort(); }\n\
             }\n"
            .to_string(),
        )
    }

    fn from_source(source: String) -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let id = ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("ea-case-review-input-{epoch}-{id}"));
        let repo = root.join("repo");
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(repo.join("src/lib.rs"), source).unwrap();
        let report = artifacts.join("audit.json");
        let input = "case-review-input-foundation";
        run_scan(
            ScanRequest {
                repo_root: repo.clone(),
                analysis: AnalysisRequest {
                    discovery: SourceDiscoveryOptions {
                        include_references: false,
                        method: AnalysisMethod::Auto,
                    },
                    profile: ScanProfile::Full,
                    scope: ScanScope::All,
                    repository_map_output: Some(report.clone()),
                    excluded_paths: BTreeSet::from([report.clone()]),
                },
                external_inputs: vec![ExternalInput::from_text(
                    ExternalInputMetadata::analysis("case_review_input", "spec-02", 1, "complete"),
                    input,
                )],
                baseline: None,
                format: ScanFormat::Json,
                output: Some(report.clone()),
                color: ColorMode::Plain,
                summary_only: false,
                review_loader: Box::new(NoReviewArtifactLoader),
            },
            &NoGraphProvider,
        )
        .unwrap();
        Self { root, repo, report }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self
            .root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("ea-case-review-input-"))
        {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn contracts() -> (CaseTemplateBundle, TypedContract, TypedContract) {
    (
        CaseTemplateBundle::checked_in().unwrap(),
        TypedContract {
            kind: ContractKind::PromptBundle,
            schema_version: 2,
            supported_schema_version: 2,
            name: "prompts-v2",
        },
        TypedContract {
            kind: ContractKind::RouteConfiguration,
            schema_version: 2,
            supported_schema_version: 2,
            name: "routes-v2",
        },
    )
}

fn verify<'a>(
    fixture: &'a Fixture,
    templates: Option<&'a dyn InputContract>,
    prompts: Option<&'a dyn InputContract>,
    routes: Option<&'a dyn InputContract>,
    attachments: &'a [&'a dyn EvidenceAttachmentAdapter],
    required: &'a [EvidenceAttachmentKind],
    expected: Option<&'a auditor_review::InputBinding>,
) -> Result<auditor_review::ReviewInput, InputError> {
    InputVerifier.verify(InputVerificationRequest {
        report_path: &fixture.report,
        repo_root: &fixture.repo,
        operator_brief: Some(&ReviewBrief::general()),
        case_template_bundle: templates,
        prompt_bundle: prompts,
        route_configuration: routes,
        attachment_adapters: attachments,
        required_attachment_kinds: required,
        expected_binding: expected,
    })
}

#[test]
fn real_full_scan_builds_deterministic_review_input_without_provider_interface() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let measurement = RuntimeMeasurement {
        metric: "wall_time_ms",
        value: 42,
    };
    let adapters: [&dyn EvidenceAttachmentAdapter; 1] = [&measurement];
    let required = [EvidenceAttachmentKind::RuntimeMeasurement];
    let first = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &adapters,
        &required,
        None,
    )
    .unwrap();
    let second = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &adapters,
        &required,
        Some(first.binding()),
    )
    .unwrap();
    assert_eq!(first.id(), second.id());
    assert_eq!(first.binding(), second.binding());
    assert_eq!(first.schema_version(), 1);
    assert!(!first.repository_map().files.is_empty());
    assert_eq!(first.evidence_attachments().len(), 1);
    let finding_id = first.report()["findings"][0]["id"].as_str().unwrap();
    assert!(first.finding(finding_id).is_some());
}

#[test]
fn verifier_rejects_changed_finding_stale_source_duplicate_and_missing_adapter() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let measurement = RuntimeMeasurement {
        metric: "wall_time_ms",
        value: 42,
    };
    let adapters: [&dyn EvidenceAttachmentAdapter; 1] = [&measurement];
    let original = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &adapters,
        &[],
        None,
    )
    .unwrap();

    let mut report: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.report).unwrap()).unwrap();
    report["findings"][0]["recommendation"] = json!("mutated recommendation");
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        verify(
            &fixture,
            Some(&templates),
            Some(&prompts),
            Some(&routes),
            &adapters,
            &[],
            Some(original.binding()),
        ),
        Err(InputError::BindingMismatch {
            contract: "report-v5",
            ..
        })
    ));

    let fresh = Fixture::new();
    let duplicates: [&dyn EvidenceAttachmentAdapter; 2] = [&measurement, &measurement];
    assert!(matches!(
        verify(
            &fresh,
            Some(&templates),
            Some(&prompts),
            Some(&routes),
            &duplicates,
            &[],
            None,
        ),
        Err(InputError::DuplicateAttachment { .. })
    ));
    assert!(matches!(
        verify(
            &fresh,
            Some(&templates),
            Some(&prompts),
            Some(&routes),
            &[],
            &[EvidenceAttachmentKind::Benchmark],
            None,
        ),
        Err(InputError::MissingAttachmentAdapter {
            kind: EvidenceAttachmentKind::Benchmark
        })
    ));

    std::fs::write(fresh.repo.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    assert!(matches!(
        verify(
            &fresh,
            Some(&templates),
            Some(&prompts),
            Some(&routes),
            &adapters,
            &[],
            None,
        ),
        Err(InputError::StaleInput { .. })
    ));
}

#[test]
fn verifier_rejects_missing_wrong_version_and_unsafe_map_path() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    assert!(matches!(
        verify(
            &fixture,
            Some(&templates),
            None,
            Some(&routes),
            &[],
            &[],
            None,
        ),
        Err(InputError::MissingInput {
            contract: "prompt-bundle"
        })
    ));

    let unsupported = TypedContract {
        schema_version: 3,
        ..prompts.clone()
    };
    assert!(matches!(
        verify(
            &fixture,
            Some(&templates),
            Some(&unsupported),
            Some(&routes),
            &[],
            &[],
            None,
        ),
        Err(InputError::UnsupportedVersion {
            contract: "prompt-bundle",
            ..
        })
    ));

    let mut report: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.report).unwrap()).unwrap();
    report["repository_map"]["path"] = json!("../escaped.json");
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        verify(
            &fixture,
            Some(&templates),
            Some(&prompts),
            Some(&routes),
            &[],
            &[],
            None,
        ),
        Err(InputError::UnsafePath { .. })
    ));
}

#[test]
fn verifier_rejects_repository_map_artifact_digest_mismatch() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.report).unwrap()).unwrap();
    let map_name = report["repository_map"]["path"].as_str().unwrap();
    let map_path = fixture.report.parent().unwrap().join(map_name);
    let mut map = std::fs::read_to_string(&map_path).unwrap();
    map.push('\n');
    std::fs::write(map_path, map).unwrap();

    assert!(matches!(
        verify(
            &fixture,
            Some(&templates),
            Some(&prompts),
            Some(&routes),
            &[],
            &[],
            None,
        ),
        Err(InputError::BindingMismatch {
            contract: "repository-map-v1",
            ..
        })
    ));
}

struct SignalPolicy {
    signals: Vec<PolicySuppressionReason>,
    max_planned_cases: Option<usize>,
}

impl AgendaPolicy for SignalPolicy {
    fn evaluate(
        &self,
        candidate: &ReviewCandidate,
        _input: &auditor_review::ReviewInput,
    ) -> Result<AgendaPolicyDecision, String> {
        Ok(AgendaPolicyDecision {
            priority: PlanningPriority::Normal,
            suppression_signals: self
                .signals
                .iter()
                .copied()
                .map(|reason| SuppressionSignal {
                    reason,
                    supporting_evidence_refs: candidate.selection_evidence_refs.clone(),
                })
                .collect(),
        })
    }

    fn max_planned_cases(&self) -> Option<usize> {
        self.max_planned_cases
    }
}

struct DanglingSelector;

impl CandidateSelector for DanglingSelector {
    fn id(&self) -> &'static str {
        "dangling_selector"
    }

    fn version(&self) -> u32 {
        1
    }

    fn select(
        &self,
        _input: &auditor_review::ReviewInput,
    ) -> Result<Vec<ReviewCandidate>, auditor_review::CandidateSelectionError> {
        Ok(vec![ReviewCandidate::from_selection(CandidateSelection {
            selector_id: self.id(),
            selector_version: self.version(),
            decision_kind: DecisionKind::EvaluateSharedRootCause,
            subject_ids: vec!["symbol:absent".to_string()],
            finding_ids: Vec::new(),
            selection_evidence_refs: vec!["measurement:absent".to_string()],
            eligible_case_kinds: vec![CaseKind::SharedRootCause],
            selection_reasons: vec![SelectionReason::SameSubjectConcentration],
        })
        .unwrap()])
    }
}

struct MultiCaseSelector;

impl CandidateSelector for MultiCaseSelector {
    fn id(&self) -> &'static str {
        "multi_case_selector"
    }

    fn version(&self) -> u32 {
        1
    }

    fn select(
        &self,
        input: &auditor_review::ReviewInput,
    ) -> Result<Vec<ReviewCandidate>, auditor_review::CandidateSelectionError> {
        let finding_id = input.report()["findings"][0]["id"].as_str().unwrap();
        let graph_id = format!("finding:{finding_id}");
        let edges = input.report()["evidence_graph"]["edges"]
            .as_array()
            .unwrap();
        let evidence_ref = edges
            .iter()
            .find(|edge| edge["kind"] == "supports" && edge["to"] == graph_id)
            .and_then(|edge| edge["from"].as_str())
            .unwrap();
        let subject_id = edges
            .iter()
            .find(|edge| edge["kind"] == "derived_from" && edge["from"] == evidence_ref)
            .and_then(|edge| edge["to"].as_str())
            .unwrap();
        Ok(vec![ReviewCandidate::from_selection(CandidateSelection {
            selector_id: self.id(),
            selector_version: self.version(),
            decision_kind: DecisionKind::EvaluateSharedRootCause,
            subject_ids: vec![subject_id.to_string()],
            finding_ids: vec![finding_id.to_string()],
            selection_evidence_refs: vec![evidence_ref.to_string()],
            eligible_case_kinds: vec![
                CaseKind::SharedRootCause,
                CaseKind::RecommendationConsolidation,
            ],
            selection_reasons: vec![SelectionReason::SameSubjectConcentration],
        })
        .unwrap()])
    }
}

#[test]
fn real_full_scan_builds_byte_identical_agenda_without_provider_interface() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let planner = AgendaPlanner;
    let first = planner.plan_with_foundation_policy(&input).unwrap();
    let second = planner.plan_with_foundation_policy(&input).unwrap();
    assert_eq!(first.to_canonical_json(), second.to_canonical_json());
    assert!(!first.candidates.is_empty());
    assert_eq!(
        first.coverage.planned_case_count,
        first
            .case_planning_records
            .iter()
            .filter(|record| record.status == auditor_review::CasePlanningStatus::Planned)
            .count()
    );
    assert_eq!(
        auditor_review::ReviewAgenda::from_json(&first.to_canonical_json(), &input)
            .unwrap()
            .agenda_id,
        first.agenda_id
    );

    let empty = planner
        .plan(
            &input,
            &SelectorRegistry::new(Vec::new()).unwrap(),
            &SignalPolicy {
                signals: Vec::new(),
                max_planned_cases: None,
            },
        )
        .unwrap();
    assert!(empty.candidates.is_empty());
    assert_eq!(
        empty.coverage.not_selected_finding_count,
        input.report()["findings"].as_array().unwrap().len()
    );
}

#[test]
fn agenda_suppression_precedence_budget_and_transactional_failure_are_explicit() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let planner = AgendaPlanner;
    let all_semantic_reasons = vec![
        PolicySuppressionReason::InsufficientMateriality,
        PolicySuppressionReason::NoAdditionalContextCanChangeAction,
        PolicySuppressionReason::DeterministicActionAlreadyComplete,
        PolicySuppressionReason::PrivacyPolicyExcludesRequiredContext,
        PolicySuppressionReason::OutsideOperatorScope,
        PolicySuppressionReason::UnsupportedEvidenceNeighborhood,
    ];
    let suppressed = planner
        .plan(
            &input,
            &SelectorRegistry::built_in(),
            &SignalPolicy {
                signals: all_semantic_reasons,
                max_planned_cases: Some(1),
            },
        )
        .unwrap();
    assert!(suppressed
        .suppression_records
        .iter()
        .any(|record| { record.reason == SuppressionReason::UnsupportedEvidenceNeighborhood }));
    assert!(!suppressed
        .suppression_records
        .iter()
        .any(|record| record.reason == SuppressionReason::ReviewBudgetDeferred));

    let budgeted = planner
        .plan(
            &input,
            &SelectorRegistry::new(vec![Box::new(MultiCaseSelector)]).unwrap(),
            &SignalPolicy {
                signals: Vec::new(),
                max_planned_cases: Some(1),
            },
        )
        .unwrap();
    assert!(budgeted
        .suppression_records
        .iter()
        .any(|record| record.reason == SuppressionReason::ReviewBudgetDeferred));

    let error = planner
        .plan(
            &input,
            &SelectorRegistry::new(vec![Box::new(DanglingSelector)]).unwrap(),
            &SignalPolicy {
                signals: Vec::new(),
                max_planned_cases: None,
            },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        auditor_review::CandidateSelectionError::DanglingReference { .. }
    ));
}

#[test]
fn real_agenda_builds_stable_production_and_shadow_cases() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let agenda = AgendaPlanner.plan_with_foundation_policy(&input).unwrap();
    let registry = CaseTemplateRegistry::from_input(&input).unwrap();
    let first = registry.instantiate_agenda(&agenda, &input).unwrap();
    let second = registry.instantiate_agenda(&agenda, &input).unwrap();
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    let lifecycle = first
        .iter()
        .find_map(|result| match result {
            CaseInstantiationResult::Instantiated(case)
                if case.case_kind == CaseKind::LifecycleMismatch =>
            {
                Some(case)
            }
            _ => None,
        })
        .expect("real full scan should produce a lifecycle case");
    assert_eq!(lifecycle.mandatory_probes.len(), 8);
    assert_eq!(lifecycle.finding_ids.len(), 1);
    assert!(lifecycle.allows_optional_probe(
        registry.bundle().entries[0].template.as_ref().unwrap(),
        ProbeKind::Counterexample,
    ));
    assert!(!lifecycle.allows_optional_probe(
        registry.bundle().entries[0].template.as_ref().unwrap(),
        ProbeKind::Mutation,
    ));
    assert!(auditor_review::EngineeringCase::from_json(
        &lifecycle.to_canonical_json(),
        registry.bundle().entries[0].template.as_ref().unwrap(),
        agenda
            .candidates
            .iter()
            .find(|candidate| candidate.candidate_id == lifecycle.candidate_id)
            .unwrap(),
        &input,
    )
    .is_ok());

    assert!(first
        .iter()
        .filter_map(|result| match result {
            CaseInstantiationResult::Instantiated(case) => Some(case),
            CaseInstantiationResult::TemplateNotEnabled(_) => None,
        })
        .any(|case| case.template_status == auditor_review::TemplateStatus::Shadow));
    assert!(!first
        .iter()
        .any(|result| matches!(result, CaseInstantiationResult::TemplateNotEnabled(_))));

    let mut edited_templates = templates.clone();
    edited_templates.entries[0]
        .template
        .as_mut()
        .unwrap()
        .evidence_contract
        .required[0]
        .description
        .push_str(" Release update.");
    let edited_input = verify(
        &fixture,
        Some(&edited_templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    assert!(matches!(
        CaseTemplateRegistry::from_input(&edited_input),
        Err(CaseInstantiationError::BundleMismatch)
    ));
    let edited_case_id = auditor_review::EngineeringCase::derive_id(
        lifecycle.case_kind,
        lifecycle.template_version,
        &lifecycle.candidate_id,
        &edited_input.binding().stable_id(),
        &lifecycle.operator_constraints,
    );
    assert_ne!(lifecycle.case_id, edited_case_id);
}

#[test]
fn lifecycle_instantiation_rejects_disconnected_subjects() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let agenda = AgendaPlanner.plan_with_foundation_policy(&input).unwrap();
    let lifecycle = agenda
        .candidates
        .iter()
        .find(|candidate| {
            candidate
                .eligible_case_kinds
                .contains(&CaseKind::LifecycleMismatch)
        })
        .unwrap();
    let second_subject = input.report()["evidence_graph"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["kind"] == "symbol")
        .filter_map(|node| node["id"].as_str())
        .find(|id| !lifecycle.subject_ids.contains(&id.to_string()))
        .unwrap();
    let candidate = ReviewCandidate::from_selection(CandidateSelection {
        selector_id: "disconnected_lifecycle_test",
        selector_version: 1,
        decision_kind: DecisionKind::EvaluateLifecycleOwnership,
        subject_ids: vec![lifecycle.subject_ids[0].clone(), second_subject.to_string()],
        finding_ids: lifecycle.finding_ids.clone(),
        selection_evidence_refs: lifecycle.selection_evidence_refs.clone(),
        eligible_case_kinds: vec![CaseKind::LifecycleMismatch],
        selection_reasons: vec![SelectionReason::RepeatedWorkLifecycleIntersection],
    })
    .unwrap();
    let registry = CaseTemplateRegistry::from_input(&input).unwrap();
    assert!(matches!(
        registry.instantiate(&candidate, CaseKind::LifecycleMismatch, &input),
        Err(CaseInstantiationError::DisconnectedSubjects)
    ));
}

fn instantiated_lifecycle_case(
    input: &auditor_review::ReviewInput,
) -> Box<auditor_review::EngineeringCase> {
    let agenda = AgendaPlanner.plan_with_foundation_policy(input).unwrap();
    let registry = CaseTemplateRegistry::from_input(input).unwrap();
    registry
        .instantiate_agenda(&agenda, input)
        .unwrap()
        .into_iter()
        .find_map(|result| match result {
            CaseInstantiationResult::Instantiated(case)
                if case.case_kind == CaseKind::LifecycleMismatch =>
            {
                Some(case)
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn evidence_packet_is_bounded_typed_case_scoped_and_schema_closed() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let case = instantiated_lifecycle_case(&input);
    let packet = EvidenceCompiler.compile(&case, &input).unwrap();

    packet.validate().unwrap();
    assert!(packet.byte_count <= packet.budget.hard_packet_bytes);
    assert_eq!(packet.case_id, case.case_id);
    assert_eq!(packet.input_binding_id, case.input_binding_id);
    assert!(packet.indexes.required.values().all(|ids| !ids.is_empty()));
    assert!(packet
        .indexes
        .unavailable
        .contains_key("runtime_measurement"));
    assert!(packet.items.iter().all(|item| {
        item.trust != TrustLabel::UntrustedRepositoryDocumentation
            && item.trust != TrustLabel::UntrustedProviderOutput
    }));
    let projected_findings = packet
        .items
        .iter()
        .filter(|item| item.kind == EvidenceItemKind::Finding)
        .map(|item| item.id.strip_prefix("finding:").unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        projected_findings,
        case.finding_ids.iter().map(String::as_str).collect()
    );
    assert!(input.report()["findings"].as_array().unwrap().len() >= projected_findings.len());

    let schema: Value = serde_json::from_str(include_str!(
        "../../../contracts/evidence-packet-v1.schema.json"
    ))
    .unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["schema_version"]["const"], 1);
}

#[test]
fn evidence_lookup_is_case_scoped_deduplicated_and_digest_order_independent() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let case = instantiated_lifecycle_case(&input);
    let packet = EvidenceCompiler.compile(&case, &input).unwrap();
    let probe_id = case.mandatory_probes[0].probe_id.clone();
    let finding_request = EvidenceLookupRequest {
        case_id: case.case_id.clone(),
        probe_id: probe_id.clone(),
        kind: EvidenceLookupRequestKind::FindingSource,
        target_id: format!("finding:{}", case.finding_ids[0]),
        decision_relevance: "inspect the selected repetition evidence".to_string(),
        hops: 0,
    };
    let mut lookup = EvidenceLookup::new(
        &fixture.repo,
        &case,
        &input,
        EvidencePacketBudget::default(),
    )
    .unwrap();
    let finding_result = lookup.execute(&finding_request).unwrap();
    assert!(matches!(
        lookup.execute(&finding_request),
        Err(EvidenceError::InvalidLookup(detail)) if detail.contains("duplicate")
    ));

    let symbol_id = packet
        .allowed_neighborhood
        .map_symbol_ids
        .first()
        .expect("lifecycle case has a mapped symbol")
        .clone();
    let symbol_request = EvidenceLookupRequest {
        case_id: case.case_id.clone(),
        probe_id,
        kind: EvidenceLookupRequestKind::SymbolNeighborhood,
        target_id: symbol_id,
        decision_relevance: "inspect the potential wider owner".to_string(),
        hops: 1,
    };
    let symbol_result = lookup.execute(&symbol_request).unwrap();
    let mut forward = finding_result.items.clone();
    forward.extend(symbol_result.items.clone());
    let mut reverse = symbol_result.items;
    reverse.extend(finding_result.items);
    reverse.reverse();
    let first = EvidenceCompiler
        .compile_with_lookup_items(&case, &input, &forward)
        .unwrap();
    let second = EvidenceCompiler
        .compile_with_lookup_items(&case, &input, &reverse)
        .unwrap();
    assert_eq!(first.packet_digest, second.packet_digest);
    assert_eq!(first.packet_id, second.packet_id);

    let path_request = EvidenceLookupRequest {
        case_id: case.case_id.clone(),
        probe_id: case.mandatory_probes[0].probe_id.clone(),
        kind: EvidenceLookupRequestKind::SymbolNeighborhood,
        target_id: "../src/lib.rs".to_string(),
        decision_relevance: "attempt a path escape".to_string(),
        hops: 0,
    };
    assert!(matches!(
        lookup.execute(&path_request),
        Err(EvidenceError::InvalidLookup(detail)) if detail.contains("raw paths")
    ));
    let excess_hops = EvidenceLookupRequest {
        target_id: packet.allowed_neighborhood.map_symbol_ids[0].clone(),
        hops: 3,
        ..path_request
    };
    assert!(matches!(
        lookup.execute(&excess_hops),
        Err(EvidenceError::InvalidLookup(detail)) if detail.contains("hop")
    ));
}

#[test]
fn evidence_lookup_rejects_stale_source_and_required_items_survive_pressure() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let large_measurement = RuntimeMeasurement {
        metric: "wall_time_ms",
        value: 42,
    };
    let adapters: [&dyn EvidenceAttachmentAdapter; 1] = [&large_measurement];
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &adapters,
        &[],
        None,
    )
    .unwrap();
    let case = instantiated_lifecycle_case(&input);
    let pressured = EvidencePacketBudget {
        target_packet_bytes: 4 * 1024,
        hard_packet_bytes: 32 * 1024,
        ..EvidencePacketBudget::default()
    };
    let packet = EvidenceCompiler
        .compile_with_budget(&case, &input, pressured)
        .unwrap();
    assert!(packet
        .items
        .iter()
        .any(|item| item.id == format!("finding:{}", case.finding_ids[0])));
    assert!(packet
        .omissions
        .iter()
        .any(|omission| omission.reason == auditor_review::EvidenceOmissionReason::TargetBudget));

    let mut lookup = EvidenceLookup::new(
        &fixture.repo,
        &case,
        &input,
        EvidencePacketBudget::default(),
    )
    .unwrap();
    std::fs::write(
        fixture.repo.join("src/lib.rs"),
        "pub fn stale_after_verification() {}\n",
    )
    .unwrap();
    let request = EvidenceLookupRequest {
        case_id: case.case_id.clone(),
        probe_id: case.mandatory_probes[0].probe_id.clone(),
        kind: EvidenceLookupRequestKind::FindingSource,
        target_id: format!("finding:{}", case.finding_ids[0]),
        decision_relevance: "verify the source snapshot before relying on it".to_string(),
        hops: 0,
    };
    assert!(matches!(
        lookup.execute(&request),
        Err(EvidenceError::RepositoryMap(
            auditor_review::RepositoryMapError::StaleSourceSnapshot(_)
        ))
    ));
}

#[test]
fn evidence_packet_reports_required_over_budget_and_rejects_mutated_case_contract() {
    let fixture = Fixture::new();
    let mut report: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.report).unwrap()).unwrap();
    for finding in report["findings"].as_array_mut().unwrap() {
        finding["message"] = json!("x".repeat(40 * 1024));
    }
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let case = instantiated_lifecycle_case(&input);
    let constrained = EvidencePacketBudget {
        target_packet_bytes: 16 * 1024,
        hard_packet_bytes: 32 * 1024,
        ..EvidencePacketBudget::default()
    };
    let packet = EvidenceCompiler
        .compile_with_budget(&case, &input, constrained)
        .unwrap();
    assert_eq!(
        packet.closure.state,
        auditor_review::ClosureState::OverBudget
    );
    assert!(packet.omissions.iter().any(|omission| {
        omission.requirement_id == "finding_records"
            && omission.reason == auditor_review::EvidenceOmissionReason::HardBudget
    }));

    let mut mutated_case = (*case).clone();
    mutated_case.evidence_contract.required.clear();
    assert!(matches!(
        EvidenceCompiler.compile(&mutated_case, &input),
        Err(EvidenceError::InvalidPacket(detail)) if detail.contains("bound template")
    ));
}

struct StaticProbeResolver {
    id: &'static str,
    answer: ProbeAnswer,
    evidence_id: Option<String>,
    model: bool,
}

impl ProbeResolver for StaticProbeResolver {
    fn id(&self) -> &'static str {
        self.id
    }

    fn version(&self) -> u32 {
        1
    }

    fn supports(&self, _probe: &ProbeDefinition) -> bool {
        true
    }

    fn resolve(
        &self,
        _case: &auditor_review::EngineeringCase,
        _probe: &ProbeDefinition,
        _packet: &auditor_review::EvidencePacket,
    ) -> Result<ProbeResolution, ProbeResolutionError> {
        let evidence = self.evidence_id.iter().cloned().collect();
        if self.model {
            let reviewer_run_id = auditor_review::StableId::from_identity(
                "reviewer-run",
                1,
                &json!({"resolver": self.id}),
            )
            .unwrap();
            Ok(ProbeResolution::model(
                self.answer,
                evidence,
                format!("{} model answer", self.id),
                Vec::new(),
                reviewer_run_id,
            ))
        } else {
            Ok(ProbeResolution::new(
                self.answer,
                evidence,
                format!("{} deterministic answer", self.id),
                Vec::new(),
            ))
        }
    }
}

#[test]
fn probe_planner_builds_all_mandatory_paths_and_bounds_optional_requests() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let case = instantiated_lifecycle_case(&input);
    let packet = EvidenceCompiler.compile(&case, &input).unwrap();
    let template = templates.entries[0].template.as_ref().unwrap();
    let mandatory = ProbePlanner.plan(&case, template, &packet, &[]).unwrap();
    assert_eq!(mandatory.len(), 8);
    assert!(mandatory
        .iter()
        .all(|definition| definition.origin == auditor_review::ProbeOrigin::Mandatory));
    let registry = ProbeResolverRegistry::built_in().unwrap();
    let results = mandatory
        .iter()
        .map(|definition| registry.resolve(&case, definition, &packet).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 8);
    assert_eq!(
        results
            .iter()
            .map(|result| result.probe_id.clone())
            .collect::<BTreeSet<_>>()
            .len(),
        8
    );
    assert!(results
        .iter()
        .all(|result| result.resolver.kind != ResolverKind::ModelInterpretation));
    assert!(results
        .iter()
        .all(|result| result.answer == ProbeAnswer::Unknown));
    let lookup_plan = ProbeLookupPlanner.plan(&case, &mandatory, &packet).unwrap();
    assert!(!lookup_plan.requests.is_empty());
    assert!(lookup_plan.requests.iter().all(|request| {
        request.case_id == case.case_id
            && mandatory
                .iter()
                .any(|definition| definition.probe_id == request.probe_id)
    }));
    assert!(
        lookup_plan.requests.len() + lookup_plan.unscheduled_probe_ids.len() <= mandatory.len()
    );

    let request = OptionalProbeRequest {
        slot: "lifecycle_counterexample".to_string(),
        kind: ProbeKind::Counterexample,
        question: "Is there a case-bounded counterexample?".to_string(),
        required_evidence_classes: vec![auditor_review::EvidenceClass::Finding],
        positive_boundary: "A selected finding supplies a concrete counterexample.".to_string(),
        negative_boundary: "No selected evidence contradicts the candidate.".to_string(),
        blocked_claim_slots: vec![auditor_review::ClaimSlot::LifecycleConclusion],
        target_ids: vec![format!("finding:{}", case.finding_ids[0])],
        decision_target: ProbeDecisionTarget::Recommendation(
            RecommendationFamily::LifecycleReownership,
        ),
        decision_relevance: "could block lifecycle reownership".to_string(),
    };
    let with_optional = ProbePlanner
        .plan(&case, template, &packet, std::slice::from_ref(&request))
        .unwrap();
    assert_eq!(with_optional.len(), 9);
    assert_eq!(
        with_optional.last().unwrap().origin,
        auditor_review::ProbeOrigin::Optional
    );

    let mut escaped = request.clone();
    escaped.target_ids = vec!["symbol:outside-case".to_string()];
    assert!(matches!(
        ProbePlanner.plan(&case, template, &packet, &[escaped]),
        Err(auditor_review::ProbeError::InvalidOptionalProbe(_))
    ));
    let mut unsupported = request;
    unsupported.kind = ProbeKind::Mutation;
    assert!(matches!(
        ProbePlanner.plan(&case, template, &packet, &[unsupported]),
        Err(auditor_review::ProbeError::InvalidOptionalProbe(_))
    ));

    let definition_schema: Value = serde_json::from_str(include_str!(
        "../../../contracts/probe-definition-v1.schema.json"
    ))
    .unwrap();
    let result_schema: Value = serde_json::from_str(include_str!(
        "../../../contracts/probe-result-v1.schema.json"
    ))
    .unwrap();
    assert_eq!(definition_schema["additionalProperties"], false);
    assert_eq!(result_schema["additionalProperties"], false);
}

#[test]
fn probe_registry_preserves_precedence_states_conflict_and_evidence_integrity() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let case = instantiated_lifecycle_case(&input);
    let packet = EvidenceCompiler.compile(&case, &input).unwrap();
    let definition = ProbePlanner
        .plan(
            &case,
            templates.entries[0].template.as_ref().unwrap(),
            &packet,
            &[],
        )
        .unwrap()
        .remove(0);
    let evidence_id = packet.indexes.required["finding_records"][0].clone();

    for answer in [
        ProbeAnswer::Yes,
        ProbeAnswer::No,
        ProbeAnswer::NotApplicable,
        ProbeAnswer::ConflictingEvidence,
    ] {
        let registry = ProbeResolverRegistry::new(vec![ResolverRegistration::new(
            ResolverKind::ExistingDeterministicFact,
            Box::new(StaticProbeResolver {
                id: "state-resolver",
                answer,
                evidence_id: Some(evidence_id.clone()),
                model: false,
            }),
        )])
        .unwrap();
        assert_eq!(
            registry
                .resolve(&case, &definition, &packet)
                .unwrap()
                .answer,
            answer
        );
    }
    let unknown = ProbeResolverRegistry::new(Vec::new())
        .unwrap()
        .resolve(&case, &definition, &packet)
        .unwrap();
    assert_eq!(unknown.answer, ProbeAnswer::Unknown);
    assert_eq!(unknown.resolver.kind, ResolverKind::UnknownFallback);

    let precedence = ProbeResolverRegistry::new(vec![
        ResolverRegistration::new(
            ResolverKind::ModelInterpretation,
            Box::new(StaticProbeResolver {
                id: "model-no",
                answer: ProbeAnswer::No,
                evidence_id: Some(evidence_id.clone()),
                model: true,
            }),
        ),
        ResolverRegistration::new(
            ResolverKind::ExistingDeterministicFact,
            Box::new(StaticProbeResolver {
                id: "fact-yes",
                answer: ProbeAnswer::Yes,
                evidence_id: Some(evidence_id.clone()),
                model: false,
            }),
        ),
    ])
    .unwrap()
    .resolve(&case, &definition, &packet)
    .unwrap();
    assert_eq!(precedence.answer, ProbeAnswer::Yes);
    assert_eq!(
        precedence.resolver.kind,
        ResolverKind::ExistingDeterministicFact
    );

    let conflict_registrations = |reversed: bool| {
        let yes = ResolverRegistration::new(
            ResolverKind::ExistingDeterministicFact,
            Box::new(StaticProbeResolver {
                id: "a-yes",
                answer: ProbeAnswer::Yes,
                evidence_id: Some(evidence_id.clone()),
                model: false,
            }),
        );
        let no = ResolverRegistration::new(
            ResolverKind::ExistingDeterministicFact,
            Box::new(StaticProbeResolver {
                id: "b-no",
                answer: ProbeAnswer::No,
                evidence_id: Some(evidence_id.clone()),
                model: false,
            }),
        );
        if reversed {
            vec![no, yes]
        } else {
            vec![yes, no]
        }
    };
    let first = ProbeResolverRegistry::new(conflict_registrations(false))
        .unwrap()
        .resolve(&case, &definition, &packet)
        .unwrap();
    let second = ProbeResolverRegistry::new(conflict_registrations(true))
        .unwrap()
        .resolve(&case, &definition, &packet)
        .unwrap();
    assert_eq!(first.answer, ProbeAnswer::ConflictingEvidence);
    assert_eq!(first, second);

    let invalid = ProbeResolverRegistry::new(vec![ResolverRegistration::new(
        ResolverKind::ExistingDeterministicFact,
        Box::new(StaticProbeResolver {
            id: "dangling",
            answer: ProbeAnswer::Yes,
            evidence_id: Some("finding:missing".to_string()),
            model: false,
        }),
    )])
    .unwrap();
    assert!(matches!(
        invalid.resolve(&case, &definition, &packet),
        Err(auditor_review::ProbeError::InvalidResult(_))
    ));

    let mut incompatible_template = templates.entries[0].template.clone().unwrap();
    incompatible_template.mandatory_probes[0].required_evidence_classes =
        vec![auditor_review::EvidenceClass::RetentionConstraint];
    assert!(incompatible_template.validate().is_err());
}

fn routing_facts(disposition: DeterministicDisposition) -> RoutingFacts {
    RoutingFacts {
        deterministic_disposition: disposition,
        predicate_evidence: Vec::new(),
        operator_intent_known: true,
        local_and_reversible: true,
        credible_action_families: 1,
        higher_order_inference_required: false,
        consequence: DecisionMagnitude::Low,
        uncertainty: DecisionMagnitude::Low,
        scope: DecisionMagnitude::Low,
        magnitude_evidence_ids: Vec::new(),
    }
}

#[test]
fn deliberation_routing_is_predicate_based_and_never_downgrades_ensemble() {
    let fixture = Fixture::new();
    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let case = instantiated_lifecycle_case(&input);
    let packet = EvidenceCompiler.compile(&case, &input).unwrap();
    let template = templates.entries[0].template.as_ref().unwrap();
    let definitions = ProbePlanner.plan(&case, template, &packet, &[]).unwrap();
    let registry = ProbeResolverRegistry::built_in().unwrap();
    let results = definitions
        .iter()
        .map(|definition| registry.resolve(&case, definition, &packet).unwrap())
        .collect::<Vec<_>>();

    let deterministic = routing_facts(DeterministicDisposition::EvidenceUnavailable);
    let deterministic_outcome = DeliberationRouter::default()
        .route(RoutingInput {
            case: &case,
            packet: &packet,
            definitions: &definitions,
            results: &results,
            facts: &deterministic,
        })
        .unwrap();
    assert_eq!(
        deterministic_outcome.selection.mode,
        DeliberationMode::Deterministic
    );
    assert!(deterministic_outcome.is_ready());
    assert!(results
        .iter()
        .all(|result| result.resolver.kind != ResolverKind::ModelInterpretation));

    let single = routing_facts(DeterministicDisposition::Unresolved);
    let single_outcome = DeliberationRouter::default()
        .route(RoutingInput {
            case: &case,
            packet: &packet,
            definitions: &definitions,
            results: &results,
            facts: &single,
        })
        .unwrap();
    assert_eq!(
        single_outcome.selection.mode,
        DeliberationMode::SingleReviewer
    );

    let mut combined = single.clone();
    combined.consequence = DecisionMagnitude::Medium;
    combined.uncertainty = DecisionMagnitude::Medium;
    combined.scope = DecisionMagnitude::Medium;
    combined.magnitude_evidence_ids = vec![packet.items[0].id.clone()];
    let combined_outcome = DeliberationRouter::default()
        .route(RoutingInput {
            case: &case,
            packet: &packet,
            definitions: &definitions,
            results: &results,
            facts: &combined,
        })
        .unwrap();
    assert_eq!(combined_outcome.selection.mode, DeliberationMode::Ensemble);

    let mut ensemble = single;
    ensemble.predicate_evidence = vec![RoutePredicateEvidence {
        predicate: RoutePredicate::ResourceLifetimeChange,
        evidence_ids: vec![packet.items[0].id.clone()],
    }];
    ensemble.local_and_reversible = false;
    let unavailable = DeliberationRouter::default()
        .route(RoutingInput {
            case: &case,
            packet: &packet,
            definitions: &definitions,
            results: &results,
            facts: &ensemble,
        })
        .unwrap();
    assert_eq!(unavailable.selection.mode, DeliberationMode::Ensemble);
    let blocked = unavailable.closure.unwrap();
    assert_eq!(blocked.failure, Some(CaseFailure::ModeUnavailable));
    assert_eq!(blocked.reason, CaseClosureReason::BlockedByModeUnavailable);

    let available = DeliberationRouter::new(ModeAvailability {
        single_reviewer: true,
        ensemble: true,
    })
    .route(RoutingInput {
        case: &case,
        packet: &packet,
        definitions: &definitions,
        results: &results,
        facts: &ensemble,
    })
    .unwrap();
    assert_eq!(available.selection.mode, DeliberationMode::Ensemble);
    assert!(available.is_ready());

    let mut unknown_operator = routing_facts(DeterministicDisposition::Unresolved);
    unknown_operator.operator_intent_known = false;
    let blocked = DeliberationRouter::default()
        .route(RoutingInput {
            case: &case,
            packet: &packet,
            definitions: &definitions,
            results: &results,
            facts: &unknown_operator,
        })
        .unwrap()
        .closure
        .unwrap();
    assert_eq!(blocked.failure, Some(CaseFailure::OperatorInputRequired));
}

#[test]
fn case_state_is_append_only_records_skips_and_isolates_failure() {
    let case_id = auditor_review::StableId::from_identity(
        "engineering-case",
        1,
        &json!({"fixture": "state-ledger"}),
    )
    .unwrap();
    let mut deterministic = CaseStateLedger::new(case_id.clone(), DeliberationMode::Deterministic);
    deterministic
        .advance(CaseExecutionState::EvidenceCompiling, Vec::new())
        .unwrap();
    deterministic
        .advance(CaseExecutionState::ProbesResolving, Vec::new())
        .unwrap();
    deterministic
        .advance(
            CaseExecutionState::Finalizing,
            [
                CaseExecutionState::Reviewing,
                CaseExecutionState::Reconsidering,
                CaseExecutionState::Comparing,
                CaseExecutionState::Challenging,
                CaseExecutionState::Responding,
            ]
            .into_iter()
            .map(|state| SkippedStage {
                state,
                reason: StageSkipReason::DeterministicMode,
            })
            .collect(),
        )
        .unwrap();
    deterministic
        .close(
            CaseClosure::terminal(
                case_id.clone(),
                CaseClosureReason::NotSubstantiated,
                vec!["mandatory_probe_negative".to_string()],
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(deterministic.current(), CaseExecutionState::Closed);
    assert_eq!(
        deterministic
            .transitions()
            .iter()
            .map(|transition| transition.sequence)
            .collect::<Vec<_>>(),
        (0..deterministic.transitions().len()).collect::<Vec<_>>()
    );
    assert!(matches!(
        deterministic.advance(CaseExecutionState::Finalizing, Vec::new()),
        Err(auditor_review::CaseTransitionError::AlreadyClosed)
    ));

    let mut illegal = CaseStateLedger::new(case_id.clone(), DeliberationMode::SingleReviewer);
    assert!(matches!(
        illegal.advance(
            CaseExecutionState::ProbesResolving,
            vec![SkippedStage {
                state: CaseExecutionState::EvidenceCompiling,
                reason: StageSkipReason::NotApplicableToMode,
            }]
        ),
        Err(auditor_review::CaseTransitionError::InvalidSkippedStages)
    ));

    let other_case_id = auditor_review::StableId::from_identity(
        "engineering-case",
        1,
        &json!({"fixture": "independent-case"}),
    )
    .unwrap();
    let mut failed = CaseStateLedger::new(case_id.clone(), DeliberationMode::Ensemble);
    failed
        .advance(CaseExecutionState::EvidenceCompiling, Vec::new())
        .unwrap();
    failed
        .fail(CaseClosure::failed(
            case_id,
            CaseFailure::EvidencePacketInvalid,
            vec!["packet_digest_mismatch".to_string()],
        ))
        .unwrap();
    let independent = CaseStateLedger::new(other_case_id, DeliberationMode::SingleReviewer);
    assert_eq!(failed.current(), CaseExecutionState::Closed);
    assert_eq!(independent.current(), CaseExecutionState::Planned);
    assert!(failed.transitions().iter().any(|transition| {
        transition
            .skipped_stages
            .iter()
            .any(|skipped| skipped.reason == StageSkipReason::FailureShortCircuit)
    }));
}

#[path = "case_review_input/spec09_tests.rs"]
mod spec09_tests;

#[path = "case_review_input/spec10_tests.rs"]
mod spec10_tests;
#[path = "case_review_input/spec11_tests.rs"]
mod spec11_tests;
#[path = "case_review_input/spec13_tests.rs"]
mod spec13_tests;
#[path = "case_review_input/spec14_tests.rs"]
mod spec14_tests;
#[path = "case_review_input/spec15_tests.rs"]
mod spec15_tests;
#[path = "case_review_input/spec16_tests.rs"]
mod spec16_tests;
