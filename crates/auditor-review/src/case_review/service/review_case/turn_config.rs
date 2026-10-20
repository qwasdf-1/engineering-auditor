use crate::{
    CaseProviderContext, NativeConversationMessage, ReviewPromptBundleV2, ReviewRole, ReviewStage,
};

use super::{
    failed, reconsideration_instruction, CaseCallFollowup, ReviewServiceError, RoleTurnConfig,
};

pub(super) fn role_turn_config(
    prompts: &ReviewPromptBundleV2,
    role: ReviewRole,
    first_pass: bool,
    followup: CaseCallFollowup,
) -> Result<RoleTurnConfig, ReviewServiceError> {
    match role {
        ReviewRole::SingleReviewer
        | ReviewRole::ReviewerA
        | ReviewRole::ReviewerB
        | ReviewRole::ReviewerC => Ok(reviewer_config(prompts, role, first_pass, followup)),
        ReviewRole::Challenger => Ok(challenger_config(prompts, first_pass, followup)),
        _ => Err(failed(
            "ensemble roles cannot execute a single-reviewer case turn",
        )),
    }
}

fn followup_note(followup: CaseCallFollowup) -> Option<&'static str> {
    match followup {
        CaseCallFollowup::None => None,
        CaseCallFollowup::AcceptedProgress => Some(
            "Continue the same case task from the admitted native ledger. Every prior call whose tool result says Accepted was recorded. Do not repeat accepted create or answer calls. Complete only the remaining work with the declared native tools and invoke the stage completion tool last.",
        ),
        CaseCallFollowup::HostCorrection => Some(
            "One or more prior task calls failed host admission. Read the following correction message and preserve calls that were accepted. An accepted create call already exists and may be changed only with its native revision tool. A rejected create call was not recorded and must be resubmitted with its original create tool, not a revision tool. Do not repeat an accepted create call or request more evidence.",
        ),
    }
}

/// Delivers a follow-up turn's instruction at the end of the conversation sent
/// for that call, leaving the system instruction and earlier turns unchanged
/// so the conversation stays an append-only, prompt-cacheable prefix. The note
/// joins a trailing user message rather than adding a second one, and the
/// recorded conversation history is not modified.
pub(super) fn with_followup_note(
    mut conversation: Vec<NativeConversationMessage>,
    followup: CaseCallFollowup,
) -> Vec<NativeConversationMessage> {
    let Some(note) = followup_note(followup) else {
        return conversation;
    };
    match conversation.last_mut() {
        Some(
            NativeConversationMessage::UserCorrection { content_markdown }
            | NativeConversationMessage::UserContinuation { content_markdown },
        ) => *content_markdown = format!("{note}\n\n{content_markdown}"),
        _ => conversation.push(NativeConversationMessage::UserContinuation {
            content_markdown: note.to_string(),
        }),
    }
    conversation
}

fn reviewer_config(
    prompts: &ReviewPromptBundleV2,
    role: ReviewRole,
    first_pass: bool,
    followup: CaseCallFollowup,
) -> RoleTurnConfig {
    let is_followup = followup != CaseCallFollowup::None;
    let is_repair = followup == CaseCallFollowup::HostCorrection;
    RoleTurnConfig {
        base_instruction: if first_pass || is_followup {
            CaseProviderContext::reviewer_instruction(prompts)
        } else {
            reconsideration_instruction(prompts, role)
        },
        stage: if is_repair || (!first_pass && !is_followup) {
            ReviewStage::Reconsidering
        } else {
            ReviewStage::Reviewing
        },
        stage_name: reviewer_stage_name(role, first_pass, followup),
    }
}

fn reviewer_stage_name(
    role: ReviewRole,
    first_pass: bool,
    followup: CaseCallFollowup,
) -> &'static str {
    let phase = if followup == CaseCallFollowup::HostCorrection {
        0
    } else if followup == CaseCallFollowup::AcceptedProgress {
        1
    } else if first_pass {
        2
    } else {
        3
    };
    let names = match role {
        ReviewRole::SingleReviewer => [
            "case_review_repair",
            "case_review_continuation",
            "case_reviewing",
            "case_reconsidering",
        ],
        ReviewRole::ReviewerA => [
            "ensemble_reviewer_a_repair",
            "ensemble_reviewer_a_continuation",
            "ensemble_reviewer_a_pass_a",
            "ensemble_reviewer_a_pass_b",
        ],
        ReviewRole::ReviewerB => [
            "ensemble_reviewer_b_repair",
            "ensemble_reviewer_b_continuation",
            "ensemble_reviewer_b_pass_a",
            "ensemble_reviewer_b_pass_b",
        ],
        ReviewRole::ReviewerC => [
            "ensemble_reviewer_c_repair",
            "ensemble_reviewer_c_continuation",
            "ensemble_reviewer_c_pass_a",
            "ensemble_reviewer_c_pass_b",
        ],
        _ => ["case_reviewing"; 4],
    };
    names[phase]
}

fn challenger_config(
    prompts: &ReviewPromptBundleV2,
    first_pass: bool,
    followup: CaseCallFollowup,
) -> RoleTurnConfig {
    let is_followup = followup != CaseCallFollowup::None;
    let is_repair = followup == CaseCallFollowup::HostCorrection;
    RoleTurnConfig {
        base_instruction: if first_pass || is_followup {
            CaseProviderContext::challenger_instruction(prompts)
        } else {
            reconsideration_instruction(prompts, ReviewRole::Challenger)
        },
        stage: ReviewStage::Challenging,
        stage_name: if is_repair {
            "case_challenge_repair"
        } else if followup == CaseCallFollowup::AcceptedProgress {
            "case_challenge_continuation"
        } else if first_pass {
            "case_challenging"
        } else {
            "case_challenge_reconsidering"
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn correction(text: &str) -> NativeConversationMessage {
        NativeConversationMessage::UserCorrection {
            content_markdown: text.to_string(),
        }
    }

    fn tail_text(conversation: &[NativeConversationMessage]) -> &str {
        match conversation.last() {
            Some(
                NativeConversationMessage::UserCorrection { content_markdown }
                | NativeConversationMessage::UserContinuation { content_markdown },
            ) => content_markdown,
            _ => "",
        }
    }

    #[test]
    fn followup_notes_append_to_the_conversation_tail_without_rewriting_history() {
        let history = vec![correction("Rejected: probe already answered.")];
        assert_eq!(
            with_followup_note(history.clone(), CaseCallFollowup::None),
            history
        );

        let repaired = with_followup_note(history.clone(), CaseCallFollowup::HostCorrection);
        assert_eq!(repaired.len(), 1, "the note joins the trailing correction");
        assert!(tail_text(&repaired).starts_with("One or more prior task calls failed"));
        assert!(tail_text(&repaired).ends_with("Rejected: probe already answered."));

        let continued = with_followup_note(Vec::new(), CaseCallFollowup::AcceptedProgress);
        assert_eq!(continued.len(), 1);
        assert!(tail_text(&continued).starts_with("Continue the same case task"));
    }
}
