//! The review driver: asks each question in turn over the provider route,
//! answers the reviewer's retrieval calls, holds each answer to its
//! question, checks proposed patches, and records the whole review.
//!
//! Each question is one conversation that opens with the orientation pack,
//! the operator's brief, and the question (the plan also gets the earlier
//! answers). Retrieval and its budget are shared by the whole review, so
//! evidence one question read stays citable in the next. Every conversation
//! shares the opening as its prompt cache scope, and a read repeated within
//! a question is pointed at its earlier result instead of run again. What
//! the reviewer sees names hash IDs by short aliases ([`aliases`]),
//! expanded back before anything is checked or recorded.
//!
//! A review that stopped (every route target failed, or the run was
//! interrupted after a checkpoint) can be resumed: the questions answered
//! before the stop are kept, with the calls, spend, retrieval, aliases,
//! and patch checks, and the rest are asked again.

mod aliases;
mod answer;
mod calls;
mod conversation;
mod progress;
mod record;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use auditor_core::sha256_digest_bytes;
use serde::Deserialize;
use serde_json::{json, Value};

use super::document::{BoundedDocument, Item, Section};
use super::questions::QuestionKind;
use super::retrieval::{RetrievalBudget, RetrievalHost};
use super::view::RepositoryView;
use crate::{
    CallDispatch, HostedConsent, NativeConversationMessage, NativeToolCall, NativeToolTurn,
    Question, QuestionAnswer, QuestionTemplateBundle, ReviewBrief, ReviewPromptBundleV2,
    ReviewRoutesV2, ToolSpec, TranscriptRecorder, Transport,
};

use aliases::Aliases;
use answer::{
    accept, accepted_markdown, addressable, answer_tool, draft_answer, held_summary,
    wire_name as name, ANSWER_TOOL,
};
pub(super) use answer::{earlier_answers, wire_name};
pub(crate) use calls::REVIEWER_TURN;
use calls::{Caller, TurnOutcome};
use conversation::{answer_now, correction, repeated, tool_result, Conversation, FINAL_TURNS};
pub use record::{
    canonical_digest, CheckRequester, EngineeringReview, PatchCheckRecord, ProjectionSummary,
    QuestionRecord, QuestionStatus, ReviewCall, Spend, ENGINEERING_REVIEW_SCHEMA_VERSION,
};

const CHECK_TOOL: &str = "check_patch";
/// Patches one review may check by default; each check is a scan.
pub const DEFAULT_PATCH_CHECKS: usize = 8;
const BRIEF_BYTES: usize = 8 * 1024;

/// Applies a patch to a scratch copy of the repository and re-scans it
/// (`auditor_scan::PatchChecker`). The verdict is the checker's record,
/// with at least a `verdict` field.
pub trait RecommendationChecker {
    fn check(&self, patch: &str, cited_findings: &[String]) -> Value;
}

pub struct ReviewSetup<'a> {
    /// The tree the repository map was verified against.
    pub repo_root: &'a Path,
    pub brief: Option<&'a ReviewBrief>,
    pub templates: &'a QuestionTemplateBundle,
    pub prompts: &'a ReviewPromptBundleV2,
    pub routes: &'a ReviewRoutesV2,
    pub consent: HostedConsent,
    pub transport: &'a dyn Transport,
    pub transcript: &'a TranscriptRecorder,
    /// Offers `check_patch` and checks accepted patches when present.
    pub checker: Option<&'a dyn RecommendationChecker>,
    pub max_patch_checks: usize,
    /// A stopped review of the same inputs and configuration to continue.
    pub resume: Option<&'a EngineeringReview>,
    /// Gets the review so far after each question, marked stopped, so an
    /// interrupted run leaves a record it can be resumed from.
    pub checkpoint: Option<&'a dyn Fn(&EngineeringReview)>,
    /// Gets a line as each question starts and ends, before each retry,
    /// and after each slow request, so a log shows a run's progress.
    pub progress: Option<&'a dyn Fn(&str)>,
}

/// Runs the review. Setup problems (consent, routes, a pack or question
/// that cannot be built) are errors; a provider that fails or a budget
/// that runs out leaves questions unanswered, with the reason recorded.
pub fn run_engineering_review(
    view: &RepositoryView<'_>,
    setup: ReviewSetup<'_>,
) -> Result<EngineeringReview, String> {
    let mut caller = Caller::new(&setup)?;
    let pack = view.orientation_pack(setup.brief)?;
    let questions = view.questions(setup.templates)?;
    let mut host = RetrievalHost::new(
        view,
        setup.repo_root,
        RetrievalBudget::default().tightened_by(setup.brief),
    )?;
    let opening = match setup.brief {
        Some(brief) => format!(
            "{}\n{}",
            pack.markdown,
            brief_markdown(brief, &view.source_digest)?
        ),
        None => pack.markdown.clone(),
    };
    let mut review = Review {
        cache_scope: opening.clone(),
        system: format!(
            "{}\n\n{}",
            setup.prompts.core_instruction, setup.prompts.reviewer_instruction
        ),
        known: pack.evidence_ids.clone(),
        answers: Vec::new(),
        records: Vec::new(),
        checks: Vec::new(),
        cache: BTreeMap::new(),
        prior_checks: 0,
        stopped: None,
        aliases: Aliases::default(),
        setup: &setup,
    };
    let header = Header {
        source_digest: view.source_digest.clone(),
        template_bundle_digest: setup.templates.digest().as_str().to_string(),
        prompt_bundle_id: setup.prompts.bundle_id.clone(),
        routes_digest: canonical_digest(setup.routes),
        brief_digest: setup.brief.map(canonical_digest),
        orientation: ProjectionSummary {
            markdown_digest: pack.audit.markdown_digest.clone(),
            markdown_bytes: pack.audit.markdown_bytes,
            evidence_ids: pack.evidence_ids.len(),
            truncated: pack.truncated,
        },
        resumed: 0,
    };
    let (header, kept) = match setup.resume {
        Some(prior) => {
            let kept = review.resume(prior, &header, &questions, &mut caller, &mut host)?;
            let resumed = prior.resumed + 1;
            (Header { resumed, ..header }, kept)
        }
        None => (header, 0),
    };
    for (asked, question) in questions.iter().enumerate().skip(kept) {
        caller.progress.question_started(
            (asked + 1, questions.len()),
            question.kind,
            review.stopped.is_some(),
        );
        review.ask(question, &opening, &mut caller, &mut host, view)?;
        if let Some(checkpoint) = setup.checkpoint.filter(|_| asked + 1 < questions.len()) {
            let interrupted = format!(
                "the run was interrupted after question {} of {}",
                asked + 1,
                questions.len()
            );
            let stopped = review.stopped.clone().unwrap_or(interrupted);
            checkpoint(&review.record(&header, &caller, &host, Some(stopped)));
        }
    }
    let stopped = review.stopped.clone();
    Ok(review.record(&header, &caller, &host, stopped))
}

/// What a review record says about its inputs and configuration.
struct Header {
    source_digest: String,
    template_bundle_digest: String,
    prompt_bundle_id: crate::StableId,
    routes_digest: String,
    brief_digest: Option<String>,
    orientation: ProjectionSummary,
    resumed: u32,
}

struct Review<'s, 'a> {
    system: String,
    /// The pack and brief every question opens with.
    cache_scope: String,
    /// Every evidence ID shown so far, by the pack, a question, or a tool.
    known: BTreeSet<String>,
    answers: Vec<(QuestionKind, QuestionAnswer)>,
    records: Vec<QuestionRecord>,
    checks: Vec<PatchCheckRecord>,
    /// Checker verdicts by patch digest and cited findings.
    cache: BTreeMap<(String, Vec<String>), Value>,
    /// Checks a resumed review's earlier run made, which count against
    /// the limit.
    prior_checks: usize,
    stopped: Option<String>,
    aliases: Aliases,
    setup: &'s ReviewSetup<'a>,
}

/// How a question's conversation ended.
enum Ending {
    Answered(Box<QuestionAnswer>, crate::RouteTarget),
    Unanswered(String),
}

impl Review<'_, '_> {
    /// The review record so far.
    fn record(
        &self,
        header: &Header,
        caller: &Caller<'_>,
        host: &RetrievalHost<'_, '_>,
        stopped: Option<String>,
    ) -> EngineeringReview {
        let mut spend = caller.spend;
        spend.patch_checks = self.prior_checks + self.cache.len();
        EngineeringReview {
            schema_version: ENGINEERING_REVIEW_SCHEMA_VERSION,
            source_digest: header.source_digest.clone(),
            template_bundle_digest: header.template_bundle_digest.clone(),
            prompt_bundle_id: header.prompt_bundle_id.clone(),
            routes_digest: header.routes_digest.clone(),
            brief_digest: header.brief_digest.clone(),
            max_patch_checks: self.setup.max_patch_checks,
            orientation: header.orientation.clone(),
            questions: self.records.clone(),
            patch_checks: self.checks.clone(),
            retrieval: host.log().to_vec(),
            calls: caller.calls.clone(),
            spend,
            stopped,
            resumed: header.resumed,
            id_aliases: self.aliases.table().clone(),
        }
    }

    /// Takes over a stopped review: keeps its questions up to the one it
    /// stopped on (all of them if it was interrupted between questions)
    /// and the state they left; how many questions it kept.
    fn resume(
        &mut self,
        prior: &EngineeringReview,
        header: &Header,
        questions: &[Question],
        caller: &mut Caller<'_>,
        host: &mut RetrievalHost<'_, '_>,
    ) -> Result<usize, String> {
        let Some(stopped) = &prior.stopped else {
            return Err("the review to resume finished; only a stopped review resumes".to_string());
        };
        let same = [
            (
                "schema version",
                prior.schema_version == ENGINEERING_REVIEW_SCHEMA_VERSION,
            ),
            ("source digest", prior.source_digest == header.source_digest),
            (
                "question templates",
                prior.template_bundle_digest == header.template_bundle_digest,
            ),
            (
                "prompt bundle",
                prior.prompt_bundle_id == header.prompt_bundle_id,
            ),
            ("routes", prior.routes_digest == header.routes_digest),
            ("brief", prior.brief_digest == header.brief_digest),
            (
                "patch-check limit",
                prior.max_patch_checks == self.setup.max_patch_checks,
            ),
        ];
        if let Some((what, _)) = same.iter().find(|(_, same)| !same) {
            return Err(format!(
                "the review to resume ran with a different {what}; resume only with the inputs and configuration it ran with"
            ));
        }
        let kept = prior
            .questions
            .iter()
            .take_while(|record| record.reason.as_ref() != Some(stopped))
            .collect::<Vec<_>>();
        if kept.len() > questions.len() {
            return Err(
                "the review to resume asked more questions than this review has".to_string(),
            );
        }
        for (record, question) in kept.iter().zip(questions) {
            if record.question_id != question.question_id {
                return Err(format!(
                    "the review to resume asked {} where this review asks {}",
                    record.question_id, question.question_id
                ));
            }
            self.known.extend(question.evidence_ids.iter().cloned());
            if let Some(answer) = &record.answer {
                self.answers.push((question.kind, answer.clone()));
            }
            self.records.push((*record).clone());
        }
        self.aliases = Aliases::restore(&prior.id_aliases)?;
        self.checks = prior.patch_checks.clone();
        self.prior_checks = prior.spend.patch_checks;
        host.resume(&prior.retrieval);
        self.known
            .extend(host.issued_evidence_ids().iter().cloned());
        caller.resume(prior);
        Ok(kept.len())
    }

    fn ask(
        &mut self,
        question: &Question,
        opening: &str,
        caller: &mut Caller<'_>,
        host: &mut RetrievalHost<'_, '_>,
        view: &RepositoryView<'_>,
    ) -> Result<(), String> {
        self.known.extend(question.evidence_ids.iter().cloned());
        host.start_question(question.kind == QuestionKind::RemediationPlan);
        let mut user = format!("{opening}\n{}", question.markdown);
        if question.kind == QuestionKind::RemediationPlan && !self.answers.is_empty() {
            let earlier = self
                .answers
                .iter()
                .filter(|(_, answer)| question.depends_on.contains(&answer.question_id))
                .map(|(kind, answer)| (*kind, answer))
                .collect::<Vec<_>>();
            let rendered = earlier_answers(&earlier, &view.source_digest)?;
            self.known.extend(rendered.evidence_ids);
            user = format!("{user}\n{}", rendered.markdown);
        }
        let tools = self.tools(question, host);
        let mut conversation = Conversation::new(question, self.aliases.shorten(&user), tools);
        conversation.spent = caller.question_spend(&question.question_id);
        let asked = self.stopped.is_none();
        let ending = match &self.stopped {
            Some(reason) => Ending::Unanswered(reason.clone()),
            None => self.converse(&mut conversation, caller, host),
        };
        let mut record = conversation.record();
        match ending {
            Ending::Answered(answer, target) => {
                self.check_accepted(question, &answer);
                record.status = QuestionStatus::Answered;
                record.target = Some(target);
                record.answer = Some((*answer).clone());
                self.answers.push((question.kind, *answer));
            }
            Ending::Unanswered(reason) => record.reason = Some(reason),
        }
        if asked {
            caller.progress.question_ended(&record);
        }
        self.records.push(record);
        Ok(())
    }

    fn tools(&self, question: &Question, host: &RetrievalHost<'_, '_>) -> Vec<ToolSpec> {
        let mut tools = host.tool_specs();
        tools.push(answer_tool(question));
        if self.setup.checker.is_some() {
            tools.push(check_tool());
        }
        tools
    }

    fn converse(
        &mut self,
        conversation: &mut Conversation<'_>,
        caller: &mut Caller<'_>,
        host: &mut RetrievalHost<'_, '_>,
    ) -> Ending {
        let mut turn_number = 0;
        loop {
            turn_number += 1;
            // The last requests offer only the answer tool, so a question
            // that read until its budget ran out still gets answered.
            if !conversation.answering && caller.requests_left(&conversation.spent) <= FINAL_TURNS {
                conversation.answering = true;
                conversation.history.push(answer_now());
            }
            let tools = if conversation.answering {
                vec![answer_tool(conversation.question)]
            } else {
                conversation.tools.clone()
            };
            let request =
                conversation.request(turn_number, &self.system, &self.cache_scope, caller);
            let input_bytes = conversation.input_bytes(&self.system);
            let dispatch =
                CallDispatch::tool_call(tools).with_conversation(conversation.history.clone());
            let outcome = caller.turn(
                &conversation.question.question_id,
                &request,
                &dispatch,
                input_bytes,
                &mut conversation.spent,
            );
            match outcome {
                TurnOutcome::Turn(turn, target) => {
                    if let Some(answer) = self.take_turn(conversation, host, turn) {
                        return Ending::Answered(Box::new(answer), target);
                    }
                }
                TurnOutcome::NoCalls => conversation.history.push(correction()),
                TurnOutcome::Spent(reason) => {
                    let review_spent = reason.starts_with("the review's");
                    return self.unanswered(reason, review_spent);
                }
                TurnOutcome::Failed(reason) => return self.unanswered(reason, true),
            }
        }
    }

    /// Runs one turn's tool calls; the accepted answer, if one was given.
    fn take_turn(
        &mut self,
        conversation: &mut Conversation<'_>,
        host: &mut RetrievalHost<'_, '_>,
        turn: NativeToolTurn,
    ) -> Option<QuestionAnswer> {
        host.start_reply();
        conversation
            .history
            .push(NativeConversationMessage::Assistant {
                text: turn.text,
                tool_calls: turn.tool_calls.clone(),
            });
        let mut accepted = None;
        for call in &turn.tool_calls {
            // The history keeps the call as sent; the host runs it on IDs.
            let call = &NativeToolCall {
                arguments: self.aliases.expand(&call.arguments),
                ..call.clone()
            };
            let message = match (&accepted, call.name.as_str()) {
                (Some(_), _) => tool_result(
                    call,
                    "The question is already answered; this call was not run.".to_string(),
                ),
                (None, ANSWER_TOOL) => {
                    let (answer, markdown) = self.answer_call(conversation, call);
                    accepted = answer;
                    tool_result(call, markdown)
                }
                (None, _) if conversation.answering => tool_result(
                    call,
                    format!("Reading is over for this question; this call was not run. Answer with {ANSWER_TOOL}."),
                ),
                (None, CHECK_TOOL) => {
                    tool_result(call, self.check_call(conversation.question, call))
                }
                (None, _) if host.reply_spent() => host.deferred(call),
                (None, _) if conversation.repeats(call) => repeated(call),
                (None, _) => {
                    let message = host.answer(call);
                    self.known
                        .extend(host.issued_evidence_ids().iter().cloned());
                    message
                }
            };
            conversation.history.push(self.aliases.shown(message));
        }
        accepted
    }

    /// Ends the question unanswered; `stops` ends the review too.
    fn unanswered(&mut self, reason: String, stops: bool) -> Ending {
        if stops {
            self.stopped = Some(reason.clone());
        }
        Ending::Unanswered(reason)
    }

    /// The reviewer's `answer_question` call: the accepted answer, or the
    /// rejection recorded and the answer held for a revision; with the
    /// tool result either way.
    fn answer_call(
        &self,
        conversation: &mut Conversation<'_>,
        call: &NativeToolCall,
    ) -> (Option<QuestionAnswer>, String) {
        let draft = draft_answer(conversation.held.as_ref(), &call.arguments);
        let checked = match &call.validation_error {
            Some(error) => Err(format!("invalid arguments: {error}")),
            None => self.answer(conversation.question, &draft),
        };
        match checked {
            Ok((answer, markdown)) => (Some(answer), markdown),
            Err(reason) => {
                let draft = addressable(draft);
                let markdown = format!(
                    "# Answer rejected\n\n{reason}\n\n{}\n",
                    held_summary(&draft)
                );
                conversation.rejected.push(reason);
                conversation.held = Some(draft);
                (None, markdown)
            }
        }
    }

    fn answer(
        &self,
        question: &Question,
        arguments: &Value,
    ) -> Result<(QuestionAnswer, String), String> {
        let earlier = self
            .answers
            .iter()
            .map(|(_, answer)| answer.clone())
            .collect::<Vec<_>>();
        let accepted = accept(question, arguments, &self.known, &earlier)?;
        let markdown = accepted_markdown(&accepted);
        Ok((accepted.answer, markdown))
    }

    /// The reviewer's `check_patch` call.
    fn check_call(&mut self, question: &Question, call: &NativeToolCall) -> String {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct CheckArguments {
            patch: String,
            resolves: Vec<String>,
        }
        let arguments = match serde_json::from_value::<CheckArguments>(call.arguments.clone()) {
            Ok(arguments) => arguments,
            Err(error) => return format!("# Check refused\n\ninvalid arguments: {error}\n"),
        };
        if arguments.resolves.is_empty()
            || arguments
                .resolves
                .iter()
                .any(|id| !question.finding_ids.contains(id))
        {
            return "# Check refused\n\n`resolves` must list finding IDs of this question.\n"
                .to_string();
        }
        match self.check(question, &arguments.patch, &arguments.resolves, None) {
            Some((check, repeated)) => {
                let mut markdown = check_markdown(&check);
                if repeated {
                    markdown.push_str(REPEATED_CHECK);
                }
                markdown
            }
            None => "# Check refused\n\nThe review's patch checks are spent.\n".to_string(),
        }
    }

    /// The host checks every accepted recommendation that carries a patch.
    fn check_accepted(&mut self, question: &Question, answer: &QuestionAnswer) {
        for step in &answer.recommendations {
            if let Some(patch) = &step.patch {
                let check = self.check(
                    question,
                    patch,
                    &step.affected_finding_ids,
                    Some(step.recommendation_id.clone()),
                );
                if check.is_none() {
                    self.checks.push(PatchCheckRecord {
                        question_id: question.question_id.clone(),
                        requested_by: CheckRequester::Host,
                        recommendation_id: Some(step.recommendation_id.clone()),
                        cited_findings: step.affected_finding_ids.clone(),
                        check: json!({"verdict": "unchecked", "reason": "the review's patch checks are spent"}),
                    });
                }
            }
        }
    }

    /// Runs (or reuses) one check and records it, saying whether it was
    /// reused; `None` once the budget is spent.
    fn check(
        &mut self,
        question: &Question,
        patch: &str,
        resolves: &[String],
        recommendation_id: Option<crate::StableId>,
    ) -> Option<(Value, bool)> {
        let checker = self.setup.checker?;
        let cited = resolves
            .iter()
            .map(|id| id.strip_prefix("finding:").unwrap_or(id).to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let key = (sha256_digest_bytes(patch.as_bytes()), cited.clone());
        let (check, repeated) = match self.cache.get(&key) {
            Some(check) => (check.clone(), true),
            None if self.prior_checks + self.cache.len() >= self.setup.max_patch_checks => {
                return None
            }
            None => {
                let check = checker.check(patch, &cited);
                self.cache.insert(key, check.clone());
                (check, false)
            }
        };
        self.checks.push(PatchCheckRecord {
            question_id: question.question_id.clone(),
            requested_by: if recommendation_id.is_some() {
                CheckRequester::Host
            } else {
                CheckRequester::Reviewer
            },
            recommendation_id,
            cited_findings: cited,
            check: check.clone(),
        });
        Some((check, repeated))
    }
}

/// After a patch that did not apply: what to change before checking again.
const NOT_APPLIED: &str = "\nThe patch was not applied, so nothing was scanned. Read the lines the reason names, correct the hunk's context and removed lines to match them exactly, and check again; split a long hunk into small ones.\n";

/// After a patch identical to one already checked.
const REPEATED_CHECK: &str = "\nThis is the patch an earlier check already ran, so this is that verdict again and cost no check. Change the patch before checking it again.\n";

fn check_tool() -> ToolSpec {
    ToolSpec {
        name: CHECK_TOOL,
        description: "Check a unified diff before proposing it: EA applies it to a scratch copy, re-scans, and runs the operator's tests if any are configured. Verified means every finding it resolves is gone, none is added, and the tests pass. Each check is a full scan, and a review has few. A hunk applies only where its context and removed lines equal the file's, so copy them from a read rather than from memory and keep about three lines of context around each change.",
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "patch": {"type": "string", "description": "A unified diff against the repository."},
                "resolves": {"type": "array", "items": {"type": "string"}, "description": "Finding IDs (finding:...) of this question the patch resolves."},
            },
            "required": ["patch", "resolves"],
        }),
        strict: false,
    }
}

/// The checker's verdict for the reviewer.
fn check_markdown(check: &Value) -> String {
    let list = |field: &str| {
        check[field]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .take(10)
                    .map(|value| match value {
                        Value::String(text) => text.clone(),
                        other => format!(
                            "{} at {}:{}: {}",
                            other["detector"].as_str().unwrap_or("?"),
                            other["path"].as_str().unwrap_or("?"),
                            other["line"],
                            other["message"].as_str().unwrap_or("")
                        ),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let mut markdown = format!(
        "# Patch check\n\n- **Verdict:** {}\n",
        check["verdict"].as_str().unwrap_or("unchecked")
    );
    for field in ["step", "reason"] {
        if let Some(value) = check[field].as_str() {
            markdown.push_str(&format!("- **{field}:** {value}\n"));
        }
    }
    if check["step"] == "apply" {
        markdown.push_str(NOT_APPLIED);
    }
    for (label, field) in [
        ("Resolved", "resolved_findings"),
        ("Still reported", "remaining_findings"),
        ("Added", "added_findings"),
    ] {
        let values = list(field);
        if !values.is_empty() {
            markdown.push_str(&format!("- **{label}:**\n"));
            for value in values {
                markdown.push_str(&format!("  - {}\n", value.replace('\n', " ")));
            }
        }
    }
    markdown
}

/// The operator's goals, constraints, and risk posture, shown after the
/// pack.
fn brief_markdown(brief: &ReviewBrief, source_digest: &str) -> Result<String, String> {
    let mut items = vec![Item::notes(
        "Goals",
        brief
            .goals
            .iter()
            .map(|goal| format!("{} ({})", goal.text, name(&goal.priority)))
            .collect(),
    )];
    if !brief.constraints.is_empty() {
        items.push(Item::notes(
            "Constraints",
            brief
                .constraints
                .iter()
                .map(|constraint| format!("{}: {}", name(&constraint.kind), constraint.text))
                .collect(),
        ));
    }
    let posture = &brief.risk_posture;
    items.push(Item::text(
        "Risk posture",
        format!(
            "change tolerance {}, operational criticality {}, security sensitivity {}",
            name(&posture.change_tolerance),
            name(&posture.operational_criticality),
            name(&posture.security_sensitivity)
        ),
    ));
    if !brief.excluded_domains.is_empty() {
        items.push(Item::notes("Out of scope", brief.excluded_domains.clone()));
    }
    BoundedDocument::new("Operator brief", vec![Section::new("Brief", items)])
        .render(BRIEF_BYTES, source_digest)
        .map(|rendered| rendered.markdown)
}
