//! One question's conversation with the reviewer: what it opened with, the
//! turns so far, what it spent, and the messages the host adds to it.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::answer::ANSWER_TOOL;
use super::calls::{Caller, QuestionSpend, REVIEWER_TURN, REVIEW_STAGE};
use super::record::{QuestionRecord, QuestionStatus};
use crate::{NativeConversationMessage, NativeToolCall, ProviderRequest, Question, ToolSpec};

/// One question's conversation with the reviewer.
pub(super) struct Conversation<'q> {
    pub(super) question: &'q Question,
    /// The opening user message: pack, brief, question, earlier answers.
    pub(super) user: String,
    pub(super) tools: Vec<ToolSpec>,
    pub(super) history: Vec<NativeConversationMessage>,
    pub(super) spent: QuestionSpend,
    pub(super) rejected: Vec<String>,
    /// The last rejected answer, which a revision builds on.
    pub(super) held: Option<Value>,
    /// Retrieval calls already answered, by tool and arguments.
    read: BTreeSet<String>,
    /// Set for the last requests, which take only an answer.
    pub(super) answering: bool,
}

impl<'q> Conversation<'q> {
    pub(super) fn new(question: &'q Question, user: String, tools: Vec<ToolSpec>) -> Self {
        Self {
            question,
            user,
            tools,
            history: Vec::new(),
            spent: QuestionSpend::default(),
            rejected: Vec::new(),
            held: None,
            read: BTreeSet::new(),
            answering: false,
        }
    }

    /// Bytes the next request sends.
    pub(super) fn input_bytes(&self, system: &str) -> usize {
        system.len() + self.user.len() + history_bytes(&self.history)
    }

    /// Whether this retrieval call, by tool and arguments, was already
    /// answered in this conversation; records it if not.
    pub(super) fn repeats(&mut self, call: &NativeToolCall) -> bool {
        !self
            .read
            .insert(format!("{}\0{}", call.name, call.arguments))
    }

    /// The question's record before its ending is known.
    pub(super) fn record(&self) -> QuestionRecord {
        let question = self.question;
        QuestionRecord {
            question_id: question.question_id.clone(),
            kind: question.kind,
            template_version: question.template_version,
            subject_ids: question
                .subjects
                .iter()
                .map(|subject| subject.id.clone())
                .collect(),
            finding_count: question.finding_ids.len(),
            depends_on: question.depends_on.clone(),
            markdown_digest: question.audit.markdown_digest.clone(),
            status: QuestionStatus::Unanswered,
            reason: None,
            target: None,
            requests: self.spent.requests,
            rejected_answers: self.rejected.clone(),
            answer: None,
        }
    }
    pub(super) fn request(
        &self,
        turn_number: usize,
        system: &str,
        cache_scope: &str,
        caller: &Caller<'_>,
    ) -> ProviderRequest {
        let budget = caller.question_budget();
        let question_id = &self.question.question_id;
        ProviderRequest {
            request_id: format!("{}#{turn_number}", question_id.as_str()),
            stage: REVIEW_STAGE.to_string(),
            system_instruction: system.to_string(),
            context: json!({"question_id": question_id, "turn": turn_number}),
            context_markdown: Some(self.user.clone()),
            output_schema_name: REVIEWER_TURN.to_string(),
            output_schema: json!({}),
            max_input_bytes: budget.max_input_bytes,
            max_output_tokens: budget
                .max_output_tokens
                .min(caller.routes.review_budget.max_output_tokens),
            timeout_ms: budget.timeout_ms,
            cache_scope: Some(cache_scope.to_string()),
        }
    }
}

fn history_bytes(history: &[NativeConversationMessage]) -> usize {
    history
        .iter()
        .map(|message| match message {
            NativeConversationMessage::Assistant { text, tool_calls } => {
                text.as_ref().map_or(0, String::len)
                    + tool_calls
                        .iter()
                        .map(|call| call.raw_arguments.len() + call.name.len())
                        .sum::<usize>()
            }
            NativeConversationMessage::ToolResult {
                content_markdown, ..
            }
            | NativeConversationMessage::UserCorrection { content_markdown }
            | NativeConversationMessage::UserContinuation { content_markdown } => {
                content_markdown.len()
            }
        })
        .sum()
}

pub(super) fn tool_result(
    call: &NativeToolCall,
    content_markdown: String,
) -> NativeConversationMessage {
    NativeConversationMessage::ToolResult {
        call_id: call.call_id.clone(),
        tool_name: call.name.clone(),
        content_markdown,
    }
}

/// Requests at the end of a question's budget that only take an answer.
pub(super) const FINAL_TURNS: usize = 2;

pub(super) fn answer_now() -> NativeConversationMessage {
    NativeConversationMessage::UserCorrection {
        content_markdown: format!(
            "This question's requests are nearly spent and reading is over: send your answer now with one {ANSWER_TOOL} call (revising a held answer if there is one), from the evidence already returned."
        ),
    }
}

/// Sent after a reply with no tool call, or one cut off.
pub(super) fn correction() -> NativeConversationMessage {
    NativeConversationMessage::UserCorrection {
        content_markdown: format!(
            "Your last reply made no tool call or was cut off. Read what you need with the tools, keep each reply short, and finish with one {ANSWER_TOOL} call."
        ),
    }
}

/// The result for a retrieval call repeated within a conversation.
pub(super) fn repeated(call: &NativeToolCall) -> NativeConversationMessage {
    tool_result(
        call,
        format!(
            "This {} call was already answered in this conversation; its result is above, unchanged.",
            call.name
        ),
    )
}
