use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{
    CompatibleVariantGroup, PortfolioBlocker, PortfolioBlockerKind, PortfolioConflict,
    PortfolioConflictKind, PortfolioDependency,
};
use crate::{
    ContextualRecommendation, EngineeringCase, EnsembleComparison, FinalizedCase,
    RecommendationFamilyState, StableId, TemplateStatus,
};

pub const REVIEW_PORTFOLIO_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAxisKind {
    Consequence,
    Urgency,
    EvidenceStrength,
    OperatorAlignment,
    ChangeScope,
    EstimatedEffort,
    DependencyReadiness,
}

impl DecisionAxisKind {
    pub const ALL: [Self; 7] = [
        Self::Consequence,
        Self::Urgency,
        Self::EvidenceStrength,
        Self::OperatorAlignment,
        Self::ChangeScope,
        Self::EstimatedEffort,
        Self::DependencyReadiness,
    ];
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AxisValue {
    Unknown,
    Explicit {
        value: String,
        provenance_refs: Vec<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionAxis {
    pub axis: DecisionAxisKind,
    pub value: AxisValue,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendationDecisionAxes {
    pub recommendation_id: StableId,
    pub axes: Vec<DecisionAxis>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorPriorityClass {
    Immediate,
    Planned,
    Opportunistic,
    Deferred,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendationPriority {
    pub recommendation_id: StableId,
    pub class: OperatorPriorityClass,
    pub provenance_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AlternativeComparisonRecord {
    pub compared_recommendation_ids: Vec<StableId>,
    pub tradeoff_comparison: String,
    pub missing_operator_decision: String,
    pub evidence_probe_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortfolioAssemblyDirectives {
    pub dependencies: Vec<PortfolioDependency>,
    pub blockers: Vec<PortfolioBlocker>,
    pub action_conflicts: Vec<PortfolioConflict>,
    pub decision_axes: Vec<RecommendationDecisionAxes>,
    pub priority_classes: Vec<RecommendationPriority>,
    pub alternative_comparison: Option<AlternativeComparisonRecord>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectRecommendationGroup {
    pub subject_id: String,
    pub recommendation_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPortfolio {
    pub schema_version: u32,
    pub accepted_recommendation_ids: Vec<StableId>,
    pub shadow_excluded_recommendation_ids: Vec<StableId>,
    pub recommendations_by_subject: Vec<SubjectRecommendationGroup>,
    pub affected_finding_ids: Vec<String>,
    pub dependencies: Vec<PortfolioDependency>,
    pub blockers: Vec<PortfolioBlocker>,
    pub conflicts: Vec<PortfolioConflict>,
    pub compatible_variants: Vec<CompatibleVariantGroup>,
    pub decision_axes: Vec<RecommendationDecisionAxes>,
    pub priority_classes: Vec<RecommendationPriority>,
    pub topological_order: Vec<StableId>,
    pub cyclic_recommendation_ids: Vec<StableId>,
    pub dependency_blocked_ids: Vec<StableId>,
    pub alternative_comparison: Option<AlternativeComparisonRecord>,
}

pub struct PortfolioAssembler;

impl PortfolioAssembler {
    pub fn assemble(
        &self,
        finalizations: &[FinalizedCase],
        cases: &[EngineeringCase],
        comparisons: &[EnsembleComparison],
        mut directives: PortfolioAssemblyDirectives,
    ) -> Result<ReviewPortfolio, String> {
        let (accepted, shadow_excluded) = collect_accepted_recommendations(
            finalizations,
            cases,
            DuplicateRecommendationPolicy::Reject,
        )?;
        normalize_directives(&mut directives);
        validate_directives(&directives, &accepted)?;
        let accepted_ids = accepted.keys().cloned().collect::<BTreeSet<_>>();
        let (topological_order, cyclic, dependency_blocked) = dependency_order(
            &accepted_ids,
            &directives.dependencies,
            &directives.priority_classes,
        );
        let mut conflicts = directives.action_conflicts.clone();
        conflicts.extend(cycle_conflicts(&cyclic, &directives.dependencies));
        conflicts.sort();
        conflicts.dedup();
        let compatible_variants = compatible_variants(comparisons, &accepted_ids);
        let portfolio = ReviewPortfolio {
            schema_version: REVIEW_PORTFOLIO_SCHEMA_VERSION,
            accepted_recommendation_ids: accepted_ids.into_iter().collect(),
            shadow_excluded_recommendation_ids: shadow_excluded.into_iter().collect(),
            recommendations_by_subject: group_by_subject(&accepted),
            affected_finding_ids: accepted
                .values()
                .flat_map(|(recommendation, _)| recommendation.affected_finding_ids.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            dependencies: directives.dependencies,
            blockers: directives.blockers,
            conflicts,
            compatible_variants,
            decision_axes: complete_axes(&accepted, directives.decision_axes),
            priority_classes: directives.priority_classes,
            topological_order,
            cyclic_recommendation_ids: cyclic,
            dependency_blocked_ids: dependency_blocked,
            alternative_comparison: directives.alternative_comparison,
        };
        portfolio.validate(finalizations, cases, comparisons)?;
        Ok(portfolio)
    }
}

impl ReviewPortfolio {
    pub fn empty() -> Self {
        Self {
            schema_version: REVIEW_PORTFOLIO_SCHEMA_VERSION,
            accepted_recommendation_ids: Vec::new(),
            shadow_excluded_recommendation_ids: Vec::new(),
            recommendations_by_subject: Vec::new(),
            affected_finding_ids: Vec::new(),
            dependencies: Vec::new(),
            blockers: Vec::new(),
            conflicts: Vec::new(),
            compatible_variants: Vec::new(),
            decision_axes: Vec::new(),
            priority_classes: Vec::new(),
            topological_order: Vec::new(),
            cyclic_recommendation_ids: Vec::new(),
            dependency_blocked_ids: Vec::new(),
            alternative_comparison: None,
        }
    }

    pub fn validate(
        &self,
        finalizations: &[FinalizedCase],
        cases: &[EngineeringCase],
        comparisons: &[EnsembleComparison],
    ) -> Result<(), String> {
        if self.schema_version != REVIEW_PORTFOLIO_SCHEMA_VERSION {
            return Err("unsupported review portfolio version".to_string());
        }
        let directives = PortfolioAssemblyDirectives {
            dependencies: self.dependencies.clone(),
            blockers: self.blockers.clone(),
            action_conflicts: self
                .conflicts
                .iter()
                .filter(|conflict| conflict.kind == PortfolioConflictKind::ActionConflict)
                .cloned()
                .collect(),
            decision_axes: self.decision_axes.clone(),
            priority_classes: self.priority_classes.clone(),
            alternative_comparison: self.alternative_comparison.clone(),
        };
        let expected =
            PortfolioAssembler.assemble_unchecked(finalizations, cases, comparisons, directives)?;
        (self == &expected)
            .then_some(())
            .ok_or_else(|| "review portfolio is stale".to_string())
    }
}

impl PortfolioAssembler {
    fn assemble_unchecked(
        &self,
        finalizations: &[FinalizedCase],
        cases: &[EngineeringCase],
        comparisons: &[EnsembleComparison],
        directives: PortfolioAssemblyDirectives,
    ) -> Result<ReviewPortfolio, String> {
        // Avoid recursive validation while preserving one public construction path.
        let (accepted, shadow) = collect_accepted_recommendations(
            finalizations,
            cases,
            DuplicateRecommendationPolicy::Allow,
        )?;
        let mut directives = directives;
        normalize_directives(&mut directives);
        validate_directives(&directives, &accepted)?;
        let ids = accepted.keys().cloned().collect::<BTreeSet<_>>();
        let (order, cyclic, blocked) =
            dependency_order(&ids, &directives.dependencies, &directives.priority_classes);
        let mut conflicts = directives.action_conflicts.clone();
        conflicts.extend(cycle_conflicts(&cyclic, &directives.dependencies));
        conflicts.sort();
        conflicts.dedup();
        Ok(ReviewPortfolio {
            schema_version: REVIEW_PORTFOLIO_SCHEMA_VERSION,
            accepted_recommendation_ids: ids.iter().cloned().collect(),
            shadow_excluded_recommendation_ids: shadow.into_iter().collect(),
            recommendations_by_subject: group_by_subject(&accepted),
            affected_finding_ids: accepted
                .values()
                .flat_map(|(recommendation, _)| recommendation.affected_finding_ids.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            dependencies: directives.dependencies,
            blockers: directives.blockers,
            conflicts,
            compatible_variants: compatible_variants(comparisons, &ids),
            decision_axes: complete_axes(&accepted, directives.decision_axes),
            priority_classes: directives.priority_classes,
            topological_order: order,
            cyclic_recommendation_ids: cyclic,
            dependency_blocked_ids: blocked,
            alternative_comparison: directives.alternative_comparison,
        })
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DuplicateRecommendationPolicy {
    Reject,
    Allow,
}

type AcceptedRecommendations<'a> =
    BTreeMap<StableId, (&'a ContextualRecommendation, &'a EngineeringCase)>;

fn collect_accepted_recommendations<'a>(
    finalizations: &'a [FinalizedCase],
    cases: &'a [EngineeringCase],
    duplicate_policy: DuplicateRecommendationPolicy,
) -> Result<(AcceptedRecommendations<'a>, BTreeSet<StableId>), String> {
    let cases = cases
        .iter()
        .map(|case| (&case.case_id, case))
        .collect::<BTreeMap<_, _>>();
    let mut accepted = BTreeMap::new();
    let mut shadow = BTreeSet::new();
    for finalized in finalizations {
        let case = cases
            .get(&finalized.case_id)
            .copied()
            .ok_or_else(|| "portfolio finalization references an unknown case".to_string())?;
        for recommendation in &finalized.accepted_recommendations {
            if case.template_status != TemplateStatus::Production {
                shadow.insert(recommendation.recommendation_id.clone());
                continue;
            }
            let duplicate = accepted
                .insert(
                    recommendation.recommendation_id.clone(),
                    (recommendation, case),
                )
                .is_some();
            if duplicate && duplicate_policy == DuplicateRecommendationPolicy::Reject {
                return Err("portfolio contains a duplicate accepted recommendation".to_string());
            }
        }
    }
    Ok((accepted, shadow))
}

fn normalize_directives(directives: &mut PortfolioAssemblyDirectives) {
    for dependency in &mut directives.dependencies {
        normalize_values(&mut dependency.evidence_ids);
    }
    normalize_values(&mut directives.dependencies);
    for blocker in &mut directives.blockers {
        normalize_values(&mut blocker.evidence_ids);
    }
    normalize_values(&mut directives.blockers);
    for conflict in &mut directives.action_conflicts {
        normalize_values(&mut conflict.recommendation_ids);
        normalize_values(&mut conflict.evidence_ids);
    }
    normalize_values(&mut directives.action_conflicts);
    for record in &mut directives.decision_axes {
        for axis in &mut record.axes {
            normalize_axis_provenance(&mut axis.value);
        }
        normalize_values(&mut record.axes);
    }
    normalize_values(&mut directives.decision_axes);
    for priority in &mut directives.priority_classes {
        normalize_values(&mut priority.provenance_refs);
    }
    normalize_values(&mut directives.priority_classes);
    if let Some(comparison) = &mut directives.alternative_comparison {
        comparison.compared_recommendation_ids.sort();
        comparison.compared_recommendation_ids.dedup();
        comparison.evidence_probe_ids.sort();
        comparison.evidence_probe_ids.dedup();
    }
}

fn normalize_values<T: Ord>(values: &mut Vec<T>) {
    values.sort();
    values.dedup();
}

fn normalize_axis_provenance(value: &mut AxisValue) {
    if let AxisValue::Explicit {
        provenance_refs, ..
    } = value
    {
        provenance_refs.sort();
        provenance_refs.dedup();
    }
}

fn dependency_valid(edge: &PortfolioDependency, ids: &BTreeSet<&StableId>) -> bool {
    edge.prerequisite_id != edge.dependent_id
        && ids.contains(&edge.prerequisite_id)
        && ids.contains(&edge.dependent_id)
        && !edge.rationale.trim().is_empty()
}

/// A blocker of an accepted recommendation, bound to a blocking accepted
/// recommendation exactly when it is a recommendation blocker.
fn blocker_valid(blocker: &PortfolioBlocker, ids: &BTreeSet<&StableId>) -> bool {
    let binding_valid = match blocker.kind {
        PortfolioBlockerKind::Recommendation => blocker
            .blocking_recommendation_id
            .as_ref()
            .is_some_and(|id| ids.contains(id)),
        PortfolioBlockerKind::OperatorDecision | PortfolioBlockerKind::EvidenceGap => {
            blocker.blocking_recommendation_id.is_none()
        }
    };
    ids.contains(&blocker.blocked_recommendation_id)
        && binding_valid
        && !blocker.reason.trim().is_empty()
}

fn action_conflict_valid(conflict: &PortfolioConflict, ids: &BTreeSet<&StableId>) -> bool {
    conflict.kind == PortfolioConflictKind::ActionConflict
        && conflict.recommendation_ids.len() >= 2
        && conflict
            .recommendation_ids
            .iter()
            .all(|id| ids.contains(id))
        && !conflict.reason.trim().is_empty()
}

fn validate_directives(
    directives: &PortfolioAssemblyDirectives,
    accepted: &BTreeMap<StableId, (&ContextualRecommendation, &EngineeringCase)>,
) -> Result<(), String> {
    let ids = accepted.keys().collect::<BTreeSet<_>>();
    if !directives
        .dependencies
        .iter()
        .all(|edge| dependency_valid(edge, &ids))
    {
        return Err(
            "portfolio dependency is invalid or outside accepted recommendations".to_string(),
        );
    }
    if !directives
        .blockers
        .iter()
        .all(|blocker| blocker_valid(blocker, &ids))
    {
        return Err("portfolio blocker is invalid or outside accepted recommendations".to_string());
    }
    if !directives
        .action_conflicts
        .iter()
        .all(|conflict| action_conflict_valid(conflict, &ids))
    {
        return Err("portfolio action conflict is invalid".to_string());
    }
    validate_axes(&directives.decision_axes, &ids)?;
    let priority_ids = directives
        .priority_classes
        .iter()
        .map(|value| &value.recommendation_id)
        .collect::<BTreeSet<_>>();
    if priority_ids.len() != directives.priority_classes.len()
        || directives.priority_classes.iter().any(|value| {
            !ids.contains(&value.recommendation_id) || value.provenance_refs.is_empty()
        })
    {
        return Err(
            "portfolio priority classes are duplicated, unbound, or lack provenance".to_string(),
        );
    }
    if let Some(comparison) = &directives.alternative_comparison {
        validate_alternative_comparison(comparison, accepted)?;
    }
    Ok(())
}

fn validate_axes(
    records: &[RecommendationDecisionAxes],
    accepted_ids: &BTreeSet<&StableId>,
) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for record in records {
        let kinds = record
            .axes
            .iter()
            .map(|axis| axis.axis)
            .collect::<BTreeSet<_>>();
        if !accepted_ids.contains(&record.recommendation_id)
            || !ids.insert(&record.recommendation_id)
            || kinds.len() != record.axes.len()
            || record.axes.iter().any(|axis| match &axis.value {
                AxisValue::Unknown => false,
                AxisValue::Explicit {
                    value,
                    provenance_refs,
                } => value.trim().is_empty() || provenance_refs.is_empty(),
            })
        {
            return Err("portfolio decision axes are duplicated, unbound, or invalid".to_string());
        }
    }
    Ok(())
}

fn validate_alternative_comparison(
    comparison: &AlternativeComparisonRecord,
    accepted: &BTreeMap<StableId, (&ContextualRecommendation, &EngineeringCase)>,
) -> Result<(), String> {
    let ids = comparison
        .compared_recommendation_ids
        .iter()
        .collect::<BTreeSet<_>>();
    let allowed_probe_ids = ids
        .iter()
        .filter_map(|id| accepted.get(*id))
        .flat_map(|(recommendation, _)| {
            recommendation
                .expected_benefits
                .iter()
                .flat_map(|benefit| &benefit.evidence_probe_ids)
                .chain(
                    recommendation
                        .verification_steps
                        .iter()
                        .flat_map(|step| &step.evidence_probe_ids),
                )
        })
        .collect::<BTreeSet<_>>();
    if ids.len() != 2
        || ids.iter().any(|id| !accepted.contains_key(*id))
        || comparison.tradeoff_comparison.trim().is_empty()
        || comparison.missing_operator_decision.trim().is_empty()
        || comparison
            .evidence_probe_ids
            .iter()
            .any(|id| !allowed_probe_ids.contains(id))
    {
        return Err(
            "alternative comparison adds an unbound recommendation or citation".to_string(),
        );
    }
    Ok(())
}

fn complete_axes(
    accepted: &BTreeMap<StableId, (&ContextualRecommendation, &EngineeringCase)>,
    supplied: Vec<RecommendationDecisionAxes>,
) -> Vec<RecommendationDecisionAxes> {
    let supplied = supplied
        .into_iter()
        .map(|record| (record.recommendation_id.clone(), record))
        .collect::<BTreeMap<_, _>>();
    accepted
        .keys()
        .map(|id| {
            let values = supplied
                .get(id)
                .map(|record| {
                    record
                        .axes
                        .iter()
                        .map(|axis| (axis.axis, axis.value.clone()))
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default();
            RecommendationDecisionAxes {
                recommendation_id: id.clone(),
                axes: DecisionAxisKind::ALL
                    .into_iter()
                    .map(|axis| DecisionAxis {
                        axis,
                        value: values.get(&axis).cloned().unwrap_or(AxisValue::Unknown),
                    })
                    .collect(),
            }
        })
        .collect()
}

fn group_by_subject(
    accepted: &BTreeMap<StableId, (&ContextualRecommendation, &EngineeringCase)>,
) -> Vec<SubjectRecommendationGroup> {
    let mut groups = BTreeMap::<String, BTreeSet<StableId>>::new();
    for (id, (_, case)) in accepted {
        for subject in &case.subject_ids {
            groups
                .entry(subject.clone())
                .or_default()
                .insert(id.clone());
        }
    }
    groups
        .into_iter()
        .map(|(subject_id, ids)| SubjectRecommendationGroup {
            subject_id,
            recommendation_ids: ids.into_iter().collect(),
        })
        .collect()
}

fn priority_rank(id: &StableId, priorities: &[RecommendationPriority]) -> (u8, StableId) {
    let rank = priorities
        .iter()
        .find(|value| value.recommendation_id == *id)
        .map_or(4, |value| match value.class {
            OperatorPriorityClass::Immediate => 0,
            OperatorPriorityClass::Planned => 1,
            OperatorPriorityClass::Opportunistic => 2,
            OperatorPriorityClass::Deferred => 3,
        });
    (rank, id.clone())
}

fn dependency_order(
    ids: &BTreeSet<StableId>,
    edges: &[PortfolioDependency],
    priorities: &[RecommendationPriority],
) -> (Vec<StableId>, Vec<StableId>, Vec<StableId>) {
    let mut indegree = ids
        .iter()
        .map(|id| (id.clone(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for edge in edges {
        if outgoing
            .entry(edge.prerequisite_id.clone())
            .or_default()
            .insert(edge.dependent_id.clone())
        {
            *indegree
                .get_mut(&edge.dependent_id)
                .expect("validated dependency") += 1;
        }
    }
    let mut ready = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let mut order = Vec::new();
    while !ready.is_empty() {
        ready.sort_by(|left, right| {
            priority_rank(left, priorities).cmp(&priority_rank(right, priorities))
        });
        let id = ready.remove(0);
        order.push(id.clone());
        for dependent in outgoing.get(&id).into_iter().flatten() {
            let degree = indegree.get_mut(dependent).expect("validated dependency");
            *degree -= 1;
            if *degree == 0 {
                ready.push(dependent.clone());
            }
        }
    }
    let cyclic = exact_cycle_nodes(ids, &outgoing);
    let emitted = order.iter().collect::<BTreeSet<_>>();
    let cyclic_set = cyclic.iter().collect::<BTreeSet<_>>();
    let blocked = ids
        .iter()
        .filter(|id| !emitted.contains(id) && !cyclic_set.contains(id))
        .cloned()
        .collect();
    (order, cyclic, blocked)
}

fn exact_cycle_nodes(
    ids: &BTreeSet<StableId>,
    outgoing: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Vec<StableId> {
    cycle_components(ids, outgoing)
        .into_iter()
        .flatten()
        .collect()
}

fn cycle_conflicts(cyclic: &[StableId], edges: &[PortfolioDependency]) -> Vec<PortfolioConflict> {
    let ids = cyclic.iter().cloned().collect::<BTreeSet<_>>();
    let mut outgoing = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for edge in edges {
        outgoing
            .entry(edge.prerequisite_id.clone())
            .or_default()
            .insert(edge.dependent_id.clone());
    }
    cycle_components(&ids, &outgoing)
        .into_iter()
        .map(|component| {
            let set = component.iter().collect::<BTreeSet<_>>();
            let evidence_ids = edges
                .iter()
                .filter(|edge| {
                    set.contains(&edge.prerequisite_id) && set.contains(&edge.dependent_id)
                })
                .flat_map(|edge| edge.evidence_ids.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            PortfolioConflict {
                kind: PortfolioConflictKind::DependencyCycle,
                recommendation_ids: component,
                reason: "explicit prerequisite edges form a dependency cycle".to_string(),
                evidence_ids,
            }
        })
        .collect()
}

fn cycle_components(
    ids: &BTreeSet<StableId>,
    outgoing: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Vec<Vec<StableId>> {
    let mut assigned = BTreeSet::new();
    let mut components = Vec::new();
    for start in ids {
        if assigned.contains(start) || !reachable(start, start, outgoing, true) {
            continue;
        }
        let component = ids
            .iter()
            .filter(|candidate| {
                reachable(start, candidate, outgoing, false)
                    && reachable(candidate, start, outgoing, false)
            })
            .cloned()
            .collect::<Vec<_>>();
        assigned.extend(component.iter().cloned());
        components.push(component);
    }
    components
}

fn reachable(
    start: &StableId,
    target: &StableId,
    outgoing: &BTreeMap<StableId, BTreeSet<StableId>>,
    require_edge: bool,
) -> bool {
    if start == target && !require_edge {
        return true;
    }
    let mut seen = BTreeSet::new();
    let mut stack = outgoing
        .get(start)
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    while let Some(next) = stack.pop() {
        if &next == target {
            return true;
        }
        if seen.insert(next.clone()) {
            stack.extend(outgoing.get(&next).into_iter().flatten().cloned());
        }
    }
    false
}

fn compatible_variants(
    comparisons: &[EnsembleComparison],
    accepted_ids: &BTreeSet<StableId>,
) -> Vec<CompatibleVariantGroup> {
    let mut groups = comparisons
        .iter()
        .flat_map(|comparison| &comparison.recommendation_families)
        .filter(|family| family.state == RecommendationFamilyState::CompatibleVariant)
        .filter_map(|family| {
            let ids = family
                .members
                .values()
                .flatten()
                .filter(|id| accepted_ids.contains(*id))
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            (ids.len() >= 2).then(|| CompatibleVariantGroup {
                recommendation_ids: ids,
                decision_dependencies: family.decision_dependencies.clone(),
            })
        })
        .collect::<Vec<_>>();
    groups.sort();
    groups.dedup();
    groups
}
