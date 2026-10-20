use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use auditor_review::{
    plan_engineering_review, run_case_engineering_review, CaseKind, CaseProviderContext,
    HostedConsent, HttpTransport, ProbeAnswer, ResolverKind, ReviewPlanRequest, ReviewRunRequest,
    StableId, Transport, TransportRequest, TransportResponse, UncontrolledReview,
    TOOL_ANSWER_CASE_PROBE, TOOL_COMPLETE_CASE_CHALLENGE, TOOL_COMPLETE_CASE_REVIEW,
    TOOL_COMPLETE_CHALLENGE_RESPONSE, TOOL_EMIT_QUALIFICATION_NONCE, TOOL_RECORD_CASE_CLAIM,
    TOOL_RECORD_CASE_OBJECTION, TOOL_RECORD_CASE_RECOMMENDATION, TOOL_RECORD_OBJECTION_RESPONSE,
};
use serde_json::{json, Value};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut report = None;
    let mut repo_root = None;
    let mut output = None;
    let mut case_templates = None;
    let mut routes = None;
    let mut prompt_bundle = None;
    let mut seed_unsupported_numeric = false;
    let mut live_challenger = false;
    let mut mock_scenario = MockScenario::Immutable;
    let mut mock_split_turns = false;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--report" => report = Some(required(&mut args, &argument)?),
            "--repo-root" => repo_root = Some(required(&mut args, &argument)?),
            "--output" => output = Some(required(&mut args, &argument)?),
            "--case-templates" => case_templates = Some(required(&mut args, &argument)?),
            "--routes" => routes = Some(required(&mut args, &argument)?),
            "--prompt-bundle" => prompt_bundle = Some(required(&mut args, &argument)?),
            "--seed-unsupported-numeric" => seed_unsupported_numeric = true,
            "--mock-intervening-mutation" => {
                set_mock_scenario(&mut mock_scenario, MockScenario::InterveningMutation)?
            }
            "--mock-test-setup-only" => {
                set_mock_scenario(&mut mock_scenario, MockScenario::TestSetupOnly)?
            }
            "--mock-freshness-constraint" => {
                set_mock_scenario(&mut mock_scenario, MockScenario::FreshnessConstraint)?
            }
            "--mock-retention-risk" => {
                set_mock_scenario(&mut mock_scenario, MockScenario::RetentionRisk)?
            }
            "--mock-missing-evidence" => {
                set_mock_scenario(&mut mock_scenario, MockScenario::MissingEvidence)?
            }
            "--mock-split-turns" => mock_split_turns = true,
            "--live-challenger" => {
                live_challenger = true;
                seed_unsupported_numeric = true;
            }
            other => return Err(format!("unknown mock_v2_case argument `{other}`")),
        }
    }
    let plan_request = ReviewPlanRequest {
        report: PathBuf::from(report.ok_or_else(|| "--report is required".to_string())?),
        repo_root: PathBuf::from(repo_root.ok_or_else(|| "--repo-root is required".to_string())?),
        case_templates: PathBuf::from(
            case_templates.ok_or_else(|| "--case-templates is required".to_string())?,
        ),
        routes: PathBuf::from(routes.ok_or_else(|| "--routes is required".to_string())?),
        prompt_bundle: PathBuf::from(
            prompt_bundle.ok_or_else(|| "--prompt-bundle is required".to_string())?,
        ),
        brief: None,
        evidence: Vec::new(),
    };
    let output = PathBuf::from(output.ok_or_else(|| "--output is required".to_string())?);
    let plan = plan_engineering_review(&plan_request).map_err(|error| error.to_string())?;
    let transport = MockCaseTransport::from_plan(
        &plan,
        seed_unsupported_numeric,
        live_challenger,
        mock_scenario,
        mock_split_turns,
    )?;
    let run = run_case_engineering_review(
        &ReviewRunRequest {
            plan: plan_request,
            output,
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&run).map_err(|error| error.to_string())?
    );
    Ok(())
}

struct MockProbe {
    slot: String,
    probe_id: StableId,
    answer: ProbeAnswer,
    evidence_ids: Vec<String>,
}

struct MockCaseTransport {
    case_kind: CaseKind,
    finding_id: String,
    scope_ids: Vec<String>,
    probes: Vec<MockProbe>,
    claim_support: Vec<StableId>,
    seed_unsupported_numeric: bool,
    live_challenger: bool,
    mock_scenario: MockScenario,
    mock_split_turns: bool,
    review_turns: AtomicUsize,
    http: HttpTransport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MockScenario {
    Immutable,
    InterveningMutation,
    FreshnessConstraint,
    MissingEvidence,
    RetentionRisk,
    TestSetupOnly,
}

impl MockCaseTransport {
    fn from_plan(
        plan: &auditor_review::ReviewPlanOutput,
        seed_unsupported_numeric: bool,
        live_challenger: bool,
        mock_scenario: MockScenario,
        mock_split_turns: bool,
    ) -> Result<Self, String> {
        let planned = plan
            .cases
            .first()
            .ok_or_else(|| "mock review requires one planned case".to_string())?;
        let resolved = planned
            .deterministic_probe_results
            .iter()
            .filter(|result| result.resolver.kind != ResolverKind::UnknownFallback)
            .cloned()
            .collect::<Vec<_>>();
        let unresolved = planned
            .probe_definitions
            .iter()
            .filter(|definition| {
                !resolved
                    .iter()
                    .any(|result| result.probe_id == definition.probe_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let context = CaseProviderContext::compile(
            &planned.case,
            &planned.evidence_packet,
            &resolved,
            &unresolved,
        )?;
        let answers = BTreeMap::from([
            (
                "repetition",
                if mock_scenario == MockScenario::TestSetupOnly {
                    ProbeAnswer::No
                } else {
                    ProbeAnswer::Yes
                },
            ),
            (
                "input_equivalence",
                if matches!(
                    mock_scenario,
                    MockScenario::InterveningMutation | MockScenario::FreshnessConstraint
                ) {
                    ProbeAnswer::No
                } else if mock_scenario == MockScenario::MissingEvidence {
                    ProbeAnswer::Unknown
                } else {
                    ProbeAnswer::Yes
                },
            ),
            (
                "intervening_mutation",
                if matches!(
                    mock_scenario,
                    MockScenario::InterveningMutation
                        | MockScenario::FreshnessConstraint
                        | MockScenario::RetentionRisk
                ) {
                    ProbeAnswer::Yes
                } else if mock_scenario == MockScenario::MissingEvidence {
                    ProbeAnswer::Unknown
                } else {
                    ProbeAnswer::No
                },
            ),
            ("material_cost", ProbeAnswer::Unknown),
            (
                "consumer_distinction",
                if matches!(
                    mock_scenario,
                    MockScenario::InterveningMutation
                        | MockScenario::FreshnessConstraint
                        | MockScenario::RetentionRisk
                ) {
                    ProbeAnswer::Yes
                } else if mock_scenario == MockScenario::MissingEvidence {
                    ProbeAnswer::Unknown
                } else {
                    ProbeAnswer::No
                },
            ),
            (
                "wider_owner",
                if matches!(
                    mock_scenario,
                    MockScenario::FreshnessConstraint | MockScenario::RetentionRisk
                ) {
                    ProbeAnswer::No
                } else {
                    ProbeAnswer::Yes
                },
            ),
            (
                "invalidation_feasibility",
                if matches!(
                    mock_scenario,
                    MockScenario::FreshnessConstraint | MockScenario::RetentionRisk
                ) {
                    ProbeAnswer::No
                } else {
                    ProbeAnswer::Yes
                },
            ),
            (
                "retention_risk",
                if matches!(
                    mock_scenario,
                    MockScenario::FreshnessConstraint | MockScenario::RetentionRisk
                ) {
                    ProbeAnswer::Yes
                } else {
                    ProbeAnswer::No
                },
            ),
        ]);
        let probes = context
            .model_probe_contracts
            .iter()
            .map(|probe| {
                let answer = answers
                    .get(probe.slot.as_str())
                    .copied()
                    .unwrap_or(ProbeAnswer::Unknown);
                MockProbe {
                    slot: probe.slot.clone(),
                    probe_id: probe.probe_id.clone(),
                    answer,
                    evidence_ids: if answer == ProbeAnswer::Unknown {
                        Vec::new()
                    } else {
                        probe
                            .admissible_evidence_ids
                            .first()
                            .cloned()
                            .into_iter()
                            .collect()
                    },
                }
            })
            .collect::<Vec<_>>();
        let claim_support = resolved
            .iter()
            .map(|result| result.probe_id.clone())
            .chain(
                probes
                    .iter()
                    .filter(|probe| probe.answer != ProbeAnswer::Unknown)
                    .map(|probe| probe.probe_id.clone()),
            )
            .collect();
        Ok(Self {
            case_kind: planned.case.case_kind,
            finding_id: planned.case.finding_ids[0].clone(),
            scope_ids: context.decision.recommendation_scope_ids,
            probes,
            claim_support,
            seed_unsupported_numeric,
            live_challenger,
            mock_scenario,
            mock_split_turns,
            review_turns: AtomicUsize::new(0),
            http: HttpTransport::default(),
        })
    }
}

impl Transport for MockCaseTransport {
    fn post(&self, request: TransportRequest) -> Result<TransportResponse, String> {
        let body: Value =
            serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
        let tool_names = body["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect::<Vec<_>>();
        let user_context = body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| message["role"].as_str() == Some("user"))
            .filter_map(|message| message["content"].as_str())
            .next()
            .unwrap_or_default();
        let latest_user_context = body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| message["role"].as_str() == Some("user"))
            .filter_map(|message| message["content"].as_str())
            .next_back()
            .unwrap_or_default();
        let continuation_requested = latest_user_context
            .contains("Reviewer completion is still required")
            || latest_user_context.contains("Reviewer evidence-boundary audit is required")
            || latest_user_context.contains("Reviewer task calls need correction")
            || latest_user_context.contains("completed reviewer ledger failed semantic admission");
        let production_shaped_qualification =
            user_context.starts_with("# Production-shaped native tool qualification");
        let mock_live_reviewer = self.live_challenger
            && !production_shaped_qualification
            && tool_names.contains(&TOOL_COMPLETE_CASE_REVIEW);
        let mock_live_response =
            self.live_challenger && tool_names.contains(&TOOL_COMPLETE_CHALLENGE_RESPONSE);
        if self.live_challenger && !mock_live_reviewer && !mock_live_response {
            return self.http.post(request);
        }
        if production_shaped_qualification {
            let properties = &body["tools"][0]["function"]["parameters"]["properties"];
            return Ok(tool_response(
                TOOL_ANSWER_CASE_PROBE,
                json!({
                    "answer": properties["answer"]["const"].clone(),
                    "evidence_ids": properties["evidence_ids"]["const"].clone(),
                    "explanation": properties["explanation"]["const"].clone(),
                    "limitations": properties["limitations"]["const"].clone(),
                    "probe_id": properties["probe_id"]["const"].clone(),
                }),
            ));
        }
        if tool_names.contains(&TOOL_EMIT_QUALIFICATION_NONCE) {
            let nonce = markdown_field(user_context, "nonce")
                .ok_or_else(|| "qualification nonce is absent".to_string())?;
            let classification = markdown_field(user_context, "qualification_case_id")
                .map(|case| {
                    match case.as_str() {
                        "semantic-boundary-1" => "positive",
                        "semantic-boundary-2" => "negative",
                        "semantic-boundary-3" => "unknown",
                        "semantic-boundary-4" => "conflicting_evidence",
                        "semantic-boundary-5" => "forbidden_redesign",
                        _ => "positive",
                    }
                    .to_string()
                })
                .or_else(|| markdown_field(user_context, "classification"))
                .unwrap_or_else(|| "positive".to_string());
            return Ok(tool_response(
                TOOL_EMIT_QUALIFICATION_NONCE,
                json!({
                    "classification": classification,
                    "nonce": nonce,
                    "native_array_markers": ["native-array-one", "native-array-two"]
                }),
            ));
        }
        if tool_names.contains(&TOOL_COMPLETE_CASE_REVIEW) {
            if continuation_requested {
                let completion = match self.case_kind {
                    CaseKind::LifecycleMismatch => match self.mock_scenario {
                        MockScenario::Immutable => json!({
                            "preferred_recommendation_ref": "move-sort-before-loop",
                            "unresolved_gaps": ["Runtime benefit remains unmeasured."]
                        }),
                        MockScenario::InterveningMutation => json!({
                            "preferred_recommendation_ref": "retain-mutation-boundary",
                            "unresolved_gaps": []
                        }),
                        MockScenario::FreshnessConstraint => json!({
                            "preferred_recommendation_ref": "retain-freshness-boundary",
                            "unresolved_gaps": []
                        }),
                        MockScenario::RetentionRisk => json!({
                            "preferred_recommendation_ref": "retain-destruction-boundary",
                            "unresolved_gaps": []
                        }),
                        MockScenario::MissingEvidence => json!({
                            "preferred_recommendation_ref": "inspect-refresh-behavior",
                            "unresolved_gaps": ["The opaque refresh callable may or may not mutate values between sort operations."]
                        }),
                        MockScenario::TestSetupOnly => json!({
                            "unresolved_gaps": ["No runtime measurement establishes the test-only sort cost."]
                        }),
                    },
                    CaseKind::SharedRootCause | CaseKind::RecommendationConsolidation => json!({
                        "preferred_recommendation_ref": "measure-shadow-template-evidence",
                        "unresolved_gaps": ["The deterministic mock does not assert shadow-template semantics; live evidence is required."]
                    }),
                };
                return Ok(tool_response(TOOL_COMPLETE_CASE_REVIEW, completion));
            }
            let mut calls = self
                .probes
                .iter()
                .map(|probe| {
                    (
                        TOOL_ANSWER_CASE_PROBE,
                        json!({
                            "probe_id": probe.probe_id,
                            "answer": serde_json::to_value(probe.answer).unwrap(),
                            "evidence_ids": probe.evidence_ids,
                            "explanation": if probe.answer == ProbeAnswer::Unknown {
                                "The packet contains no bound runtime measurement, so material cost remains unknown."
                            } else {
                                "The complete function source establishes this probe boundary for the deterministic mock review."
                            },
                            "limitations": if probe.answer == ProbeAnswer::Unknown {
                                vec!["No benchmark, trace, or runtime measurement is bound to this case.".to_string()]
                            } else {
                                Vec::<String>::new()
                            }
                        }),
                    )
                })
                .collect::<Vec<_>>();
            if self.case_kind != CaseKind::LifecycleMismatch {
                let probe_ids = self
                    .probes
                    .iter()
                    .map(|probe| probe.probe_id.clone())
                    .collect::<Vec<_>>();
                let claim_ref = "shadow-template-evidence-gap";
                let recommendation_ref = "measure-shadow-template-evidence";
                let claim = (
                    TOOL_RECORD_CASE_CLAIM,
                    json!({
                        "claim_ref": claim_ref,
                        "slot": "remediation_boundary",
                        "kind": "actionability",
                        "proposition": "The shadow-template case lacks enough model-qualified evidence to choose a repository-specific remediation boundary.",
                        "status": "blocked",
                        "supporting_probe_ids": probe_ids,
                        "counter_probe_ids": [],
                        "dependency_claim_refs": [],
                        "assumptions": [],
                        "confidence": "low"
                    }),
                );
                let recommendation = (
                    TOOL_RECORD_CASE_RECOMMENDATION,
                    json!({
                        "recommendation_ref": recommendation_ref,
                        "family": "measurement_before_change",
                        "summary": "Collect live evidence for the shadow-template case before selecting a repository-specific action.",
                        "scope_ids": self.scope_ids,
                        "premise_claim_refs": [claim_ref],
                        "affected_finding_ids": [self.finding_id],
                        "detector_advice_relationship": "narrows",
                        "preconditions": ["A currently qualified native-tool route is available for the case."],
                        "expected_benefits": [{
                            "description": "Establishes whether a shared or consolidated action is justified without inventing unsupported semantics.",
                            "evidence_mode": "qualitative_only",
                            "evidence_probe_ids": probe_ids
                        }],
                        "risks": ["The live evidence may confirm that the findings require independent actions."],
                        "costs": ["Requires a bounded live review run."],
                        "alternatives": [{
                            "summary": "Keep the shadow template disabled until its live evidence predicate is met.",
                            "tradeoffs": ["Preserves safety but delays promotion evidence."]
                        }],
                        "verification_steps": [{
                            "method": "Run the digest-bound native-tool case and verify every mandatory probe, closure, and citation.",
                            "expected_observation": "The artifact contains a valid typed conclusion and closure for the shadow template.",
                            "evidence_probe_ids": probe_ids
                        }],
                        "materiality": "low",
                        "materiality_basis": "This deterministic mock deliberately makes no model-quality claim.",
                        "confidence": "low"
                    }),
                );
                let completion = (
                    TOOL_COMPLETE_CASE_REVIEW,
                    json!({
                        "preferred_recommendation_ref": recommendation_ref,
                        "unresolved_gaps": ["The deterministic mock does not assert shadow-template semantics; live evidence is required."]
                    }),
                );
                if self.review_turns.fetch_add(1, Ordering::Relaxed) > 0 {
                    return Ok(tool_calls_response(vec![completion]));
                }
                calls.push(claim);
                calls.push(recommendation);
                if self.mock_split_turns {
                    return Ok(tool_calls_response(calls));
                }
                calls.push(completion);
                return Ok(tool_calls_response(calls));
            }
            if self.mock_scenario == MockScenario::Immutable {
                calls.push((
                    TOOL_RECORD_CASE_CLAIM,
                    json!({
                        "claim_ref": "ownership-premise",
                        "slot": "wider_owner",
                        "kind": "actionability",
                        "proposition": "The function activation is the concrete wider owner for a single pre-loop sort.",
                        "status": "substantiated",
                        "supporting_probe_ids": self.claim_support,
                        "counter_probe_ids": [],
                        "dependency_claim_refs": [],
                        "assumptions": [],
                        "confidence": "high"
                    }),
                ));
            }
            if matches!(
                self.mock_scenario,
                MockScenario::InterveningMutation
                    | MockScenario::FreshnessConstraint
                    | MockScenario::RetentionRisk
            ) {
                let (
                    claim_ref,
                    slot,
                    proposition,
                    recommendation_ref,
                    summary,
                    precondition,
                    benefit,
                    risk,
                    alternative,
                    expected_observation,
                ) = if self.mock_scenario == MockScenario::RetentionRisk {
                    (
                        "destruction-boundary",
                        "retention_risk",
                        "Immediate zeroization is an explicit destruction and privacy constraint, so no wider owner may retain one parser across authorizations.",
                        "retain-destruction-boundary",
                        "Keep parser construction and destruction at each authorization boundary.",
                        "The authentication pattern remains zeroized immediately after every authorization.",
                        "Preserves the explicit secret-destruction policy without claiming an unmeasured performance gain.",
                        "The repeated parser construction cost remains.",
                        "Use a separately audited secret representation with an equally strict per-authorization destruction contract.",
                        "Each authorization receives a fresh parser and its secret pattern is zeroized immediately afterward.",
                    )
                } else if self.mock_scenario == MockScenario::FreshnessConstraint {
                    (
                        "fresh-snapshot-boundary",
                        "retention_risk",
                        "Each epoch refresh adds current state before sorting, and the explicit snapshot contract forbids reusing an earlier sorted value.",
                        "retain-freshness-boundary",
                        "Keep sorting after each epoch refresh so every published snapshot reflects that epoch's state.",
                        "Each returned snapshot remains paired with its observed epoch and includes the value added by that refresh.",
                        "Preserves the explicit per-epoch freshness contract without claiming an unmeasured performance gain.",
                        "The per-epoch sort cost remains.",
                        "Use an incrementally ordered representation whose update contract is proven to preserve every epoch-specific snapshot.",
                        "Every returned snapshot contains the value introduced for its paired observed epoch.",
                    )
                } else {
                    (
                        "retention-boundary",
                        "mutation_boundary",
                        "Consumer-specific mutation changes the collection before each sort, so the repeated operation must remain at the current invalidation boundary.",
                        "retain-mutation-boundary",
                        "Keep the sort at the consumer-specific mutation boundary instead of hoisting or retaining one shared result.",
                        "Each consumer-specific mutation remains observable in its ordered output.",
                        "Preserves consumer-specific ordered outputs without claiming an unmeasured performance gain.",
                        "The repeated sort cost remains when many consumers are processed.",
                        "Redesign the consumer input representation so independently ordered snapshots can be computed without shared mutation.",
                        "Each consumer receives its replacement-specific sorted values in the original iteration order.",
                    )
                };
                calls.push((
                    TOOL_RECORD_CASE_CLAIM,
                    json!({
                        "claim_ref": claim_ref,
                        "slot": slot,
                        "kind": "root_cause",
                        "proposition": proposition,
                        "status": "substantiated",
                        "supporting_probe_ids": self.claim_support,
                        "counter_probe_ids": [],
                        "dependency_claim_refs": [],
                        "assumptions": [],
                        "confidence": "high"
                    }),
                ));
                calls.push((
                    TOOL_RECORD_CASE_RECOMMENDATION,
                    json!({
                        "recommendation_ref": recommendation_ref,
                        "family": "explicit_retention_avoidance",
                        "summary": summary,
                        "scope_ids": self.scope_ids,
                        "premise_claim_refs": [claim_ref],
                        "affected_finding_ids": [self.finding_id],
                        "detector_advice_relationship": "narrows",
                        "preconditions": [precondition],
                        "expected_benefits": [{
                            "description": benefit,
                            "evidence_mode": "qualitative_only",
                            "evidence_probe_ids": self.claim_support
                        }],
                        "risks": [risk],
                        "costs": ["No implementation change; retain the existing recomputation boundary."],
                        "alternatives": [{
                            "summary": alternative,
                            "tradeoffs": ["Could reduce repeated work but changes the current ownership and data-flow contract."]
                        }],
                        "verification_steps": [{
                            "method": "Exercise the affected lifecycle boundary and verify the retained-state contract.",
                            "expected_observation": expected_observation,
                            "evidence_probe_ids": self.claim_support
                        }],
                        "materiality": "low",
                        "materiality_basis": "The detector finding is low severity and has no bound runtime measurement.",
                        "confidence": "high"
                    }),
                ));
                let completion = (
                    TOOL_COMPLETE_CASE_REVIEW,
                    json!({
                        "unresolved_gaps": []
                    }),
                );
                if self.mock_split_turns && self.review_turns.fetch_add(1, Ordering::Relaxed) == 0 {
                    return Ok(tool_calls_response(calls));
                }
                if self.mock_split_turns {
                    return Ok(tool_calls_response(vec![completion]));
                }
                calls.push(completion);
                return Ok(tool_calls_response(calls));
            }
            if self.mock_scenario == MockScenario::MissingEvidence {
                let mutation_probe = self
                    .probes
                    .iter()
                    .find(|probe| probe.slot == "intervening_mutation")
                    .ok_or_else(|| "missing-evidence mock lacks mutation probe".to_string())?;
                let invalidation_probe = self
                    .probes
                    .iter()
                    .find(|probe| probe.slot == "invalidation_feasibility")
                    .ok_or_else(|| "missing-evidence mock lacks invalidation probe".to_string())?;
                calls.push((
                    TOOL_RECORD_CASE_CLAIM,
                    json!({
                        "claim_ref": "opaque-mutation-gap",
                        "slot": "mutation_boundary",
                        "kind": "root_cause",
                        "proposition": "The opaque refresh callable leaves intervening mutation unconfirmed.",
                        "status": "blocked",
                        "supporting_probe_ids": [mutation_probe.probe_id],
                        "counter_probe_ids": [],
                        "dependency_claim_refs": [],
                        "assumptions": [],
                        "confidence": "medium"
                    }),
                ));
                calls.push((
                    TOOL_RECORD_CASE_CLAIM,
                    json!({
                        "claim_ref": "bounded-inspection-path",
                        "slot": "invalidation_feasibility",
                        "kind": "actionability",
                        "proposition": "Inspecting or measuring the refresh callable is a bounded way to resolve the mutation gap.",
                        "status": "substantiated",
                        "supporting_probe_ids": [invalidation_probe.probe_id],
                        "counter_probe_ids": [],
                        "dependency_claim_refs": [],
                        "assumptions": [],
                        "confidence": "high"
                    }),
                ));
                calls.push((
                    TOOL_RECORD_CASE_RECOMMENDATION,
                    json!({
                        "recommendation_ref": "inspect-refresh-behavior",
                        "family": "measurement_before_change",
                        "summary": "Inspect or measure the refresh callable before changing sort ownership.",
                        "scope_ids": self.scope_ids,
                        "premise_claim_refs": ["bounded-inspection-path"],
                        "affected_finding_ids": [self.finding_id],
                        "detector_advice_relationship": "narrows",
                        "preconditions": ["The refresh implementation or its runtime effects can be observed."],
                        "expected_benefits": [{
                            "description": "Resolves whether mutation invalidates a retained sorted result without claiming an unmeasured gain.",
                            "evidence_mode": "qualitative_only",
                            "evidence_probe_ids": [invalidation_probe.probe_id]
                        }],
                        "risks": ["The inspection may show that per-iteration sorting is required."],
                        "costs": ["Requires focused source inspection or instrumentation."],
                        "alternatives": [{
                            "summary": "Keep the current sort placement without resolving the finding.",
                            "tradeoffs": ["Avoids investigation but leaves ownership undecided."]
                        }],
                        "verification_steps": [{
                            "method": "Inspect or trace refresh across representative calls.",
                            "expected_observation": "Evidence establishes either mutation or no mutation between sort operations.",
                            "evidence_probe_ids": [invalidation_probe.probe_id]
                        }],
                        "materiality": "low",
                        "materiality_basis": "No bound runtime measurement establishes material cost.",
                        "confidence": "medium"
                    }),
                ));
                let completion = (
                    TOOL_COMPLETE_CASE_REVIEW,
                    json!({
                        "preferred_recommendation_ref": "inspect-refresh-behavior",
                        "unresolved_gaps": ["The opaque refresh callable may or may not mutate values between sort operations."]
                    }),
                );
                if self.mock_split_turns && self.review_turns.fetch_add(1, Ordering::Relaxed) == 0 {
                    return Ok(tool_calls_response(calls));
                }
                if self.mock_split_turns {
                    return Ok(tool_calls_response(vec![completion]));
                }
                calls.push(completion);
                return Ok(tool_calls_response(calls));
            }
            if self.mock_scenario == MockScenario::TestSetupOnly {
                let repetition_probe = self
                    .probes
                    .iter()
                    .find(|probe| probe.slot == "repetition")
                    .ok_or_else(|| "test-only mock lacks repetition probe".to_string())?;
                let material_probe = self
                    .probes
                    .iter()
                    .find(|probe| probe.slot == "material_cost")
                    .ok_or_else(|| "test-only mock lacks material-cost probe".to_string())?;
                calls.push((
                    TOOL_RECORD_CASE_CLAIM,
                    json!({
                        "claim_ref": "test-only-boundary",
                        "slot": "repetition",
                        "kind": "root_cause",
                        "proposition": "The guarded loop executes only in test builds, so it is outside the production repetition boundary.",
                        "status": "substantiated",
                        "supporting_probe_ids": [repetition_probe.probe_id],
                        "counter_probe_ids": [],
                        "dependency_claim_refs": [],
                        "assumptions": [],
                        "confidence": "high"
                    }),
                ));
                calls.push((
                    TOOL_RECORD_CASE_CLAIM,
                    json!({
                        "claim_ref": "optional-material-cost",
                        "slot": "material_impact",
                        "kind": "expected_benefit",
                        "proposition": "No bound measurement establishes the test-only sort cost.",
                        "status": "blocked",
                        "supporting_probe_ids": [material_probe.probe_id],
                        "counter_probe_ids": [],
                        "dependency_claim_refs": [],
                        "assumptions": [],
                        "confidence": "low"
                    }),
                ));
                let completion = (
                    TOOL_COMPLETE_CASE_REVIEW,
                    json!({
                        "unresolved_gaps": ["No runtime measurement establishes the test-only sort cost."]
                    }),
                );
                if self.mock_split_turns && self.review_turns.fetch_add(1, Ordering::Relaxed) == 0 {
                    return Ok(tool_calls_response(calls));
                }
                if self.mock_split_turns {
                    return Ok(tool_calls_response(vec![completion]));
                }
                calls.push(completion);
                return Ok(tool_calls_response(calls));
            }
            let expected_benefit = if self.seed_unsupported_numeric {
                json!({
                    "description": "Hoisting the sort will reduce latency by 70 percent.",
                    "evidence_mode": "direct_measurement",
                    "evidence_probe_ids": self.claim_support,
                    "numeric_value": 70.0,
                    "unit": "percent latency reduction"
                })
            } else {
                json!({
                    "description": "Eliminates repeated structurally redundant sort operations without claiming an unmeasured numeric improvement.",
                    "evidence_mode": "structural_inference",
                    "evidence_probe_ids": self.claim_support
                })
            };
            calls.push((
                TOOL_RECORD_CASE_RECOMMENDATION,
                json!({
                    "recommendation_ref": "move-sort-before-loop",
                    "family": "lifecycle_reownership",
                    "summary": "Sort the unchanged collection once before entering the loop.",
                    "scope_ids": self.scope_ids,
                    "premise_claim_refs": ["ownership-premise"],
                    "affected_finding_ids": [self.finding_id],
                    "detector_advice_relationship": "preserves",
                    "preconditions": ["The loop body continues to contain no mutation that invalidates sorted order."],
                    "expected_benefits": [expected_benefit],
                    "risks": ["A future loop-body mutation could invalidate the hoist."],
                    "costs": ["A focused regression test must pin output equivalence."],
                    "alternatives": [{
                        "summary": "Keep the sort local and measure before changing ownership.",
                        "tradeoffs": ["Preserves current placement but retains repeated work."]
                    }],
                    "verification_steps": [{
                        "method": "Run equivalence tests with unsorted, sorted, empty, and duplicate inputs.",
                        "expected_observation": "Outputs match while the sort call executes once per function activation.",
                        "evidence_probe_ids": self.claim_support
                    }],
                    "materiality": "medium",
                    "materiality_basis": "Static structure proves repeated work but no runtime measurement quantifies the gain.",
                    "confidence": "high"
                }),
            ));
            calls.push((
                TOOL_COMPLETE_CASE_REVIEW,
                json!({
                    "preferred_recommendation_ref": "move-sort-before-loop",
                    "unresolved_gaps": ["Runtime benefit remains unmeasured."]
                }),
            ));
            return Ok(tool_calls_response(calls));
        }
        if tool_names.contains(&TOOL_COMPLETE_CASE_CHALLENGE) {
            if self.seed_unsupported_numeric {
                let objection_tool = body["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|tool| {
                        tool["function"]["name"].as_str() == Some(TOOL_RECORD_CASE_OBJECTION)
                    })
                    .ok_or_else(|| "seeded challenge lacks objection tool".to_string())?;
                let target_id = objection_tool["function"]["parameters"]["properties"]["target_id"]
                    ["enum"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .find(|id| id.starts_with("contextual-recommendation:"))
                    .ok_or_else(|| "seeded challenge lacks recommendation target".to_string())?;
                let evidence_id = objection_tool["function"]["parameters"]["properties"]
                    ["evidence_ids"]["items"]["enum"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .find(|id| id.starts_with("source-span:"))
                    .ok_or_else(|| "seeded challenge lacks source evidence".to_string())?;
                return Ok(tool_calls_response(vec![
                    (
                        TOOL_RECORD_CASE_OBJECTION,
                        json!({
                            "objection_ref": "unsupported-numeric-benefit",
                            "target_type": "recommendation",
                            "target_id": target_id,
                            "reason": "overclaimed_benefit",
                            "evidence_ids": [evidence_id],
                            "materiality": "high",
                            "required_resolution": "Remove the unmeasured 70 percent claim or replace it with a qualitative structural benefit."
                        }),
                    ),
                    (TOOL_COMPLETE_CASE_CHALLENGE, json!({})),
                ]));
            }
            return Ok(tool_response(TOOL_COMPLETE_CASE_CHALLENGE, json!({})));
        }
        if tool_names.contains(&TOOL_COMPLETE_CHALLENGE_RESPONSE) {
            let response_tool = body["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|tool| {
                    tool["function"]["name"].as_str() == Some(TOOL_RECORD_OBJECTION_RESPONSE)
                })
                .ok_or_else(|| "seeded response lacks objection-response tool".to_string())?;
            let objection_id = response_tool["function"]["parameters"]["properties"]
                ["objection_id"]["enum"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .next()
                .ok_or_else(|| "seeded response lacks objection ID".to_string())?;
            return Ok(tool_calls_response(vec![
                (
                    TOOL_RECORD_OBJECTION_RESPONSE,
                    json!({
                        "objection_id": objection_id,
                        "action": "withdraw",
                        "rationale": "The source explicitly says the 70 percent value is unmeasured, so the recommendation is withdrawn rather than preserving an unsupported benefit.",
                        "evidence_probe_ids": []
                    }),
                ),
                (TOOL_COMPLETE_CHALLENGE_RESPONSE, json!({})),
            ]));
        }
        Err(format!(
            "mock transport received unknown tool set: {tool_names:?}"
        ))
    }

    fn get(&self, request: TransportRequest) -> Result<TransportResponse, String> {
        if self.live_challenger {
            self.http.get(request)
        } else {
            Err("deterministic mock does not perform provider discovery".to_string())
        }
    }
}

fn markdown_field(markdown: &str, field: &str) -> Option<String> {
    let marker = format!("**{field}:** ");
    markdown.lines().find_map(|line| {
        line.find(&marker)
            .map(|offset| line[offset + marker.len()..].trim().to_string())
    })
}

fn set_mock_scenario(current: &mut MockScenario, requested: MockScenario) -> Result<(), String> {
    if *current != MockScenario::Immutable && *current != requested {
        return Err("mock scenario flags are mutually exclusive".to_string());
    }
    *current = requested;
    Ok(())
}

fn tool_response(name: &str, arguments: Value) -> TransportResponse {
    tool_calls_response(vec![(name, arguments)])
}

fn tool_calls_response(calls: Vec<(&str, Value)>) -> TransportResponse {
    TransportResponse {
        status: 200,
        body: serde_json::to_vec(&json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "content": "Deterministic mock reasoning is represented by the native task calls.",
                    "tool_calls": calls.into_iter().enumerate().map(|(index, (name, arguments))| json!({
                        "id": format!("mock-call-{index}"),
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": serde_json::to_string(&arguments).unwrap()
                        }
                    })).collect::<Vec<_>>()
                }
            }],
            "id": "deterministic-mock-response",
            "usage": {"completion_tokens": 0, "prompt_tokens": 0}
        }))
        .expect("mock response is serializable"),
    }
}

fn required(args: &mut impl Iterator<Item = String>, argument: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{argument} requires a value"))
}
