//! The citation and contradiction gates. Citations: each answer is checked
//! again against the evidence the review showed (the pack, the questions
//! so far, every retrieval result, and for the plan the earlier answers),
//! and every ID an answer cites or a retrieval call issued must resolve in
//! the inputs. Contradictions: a claim may not cite one ID both for and
//! against itself, nor state a different number for a value a cited
//! finding measured (`cyclomatic_complexity 12` when the finding says 31).
//! Whether a claim's judgment is right is the usefulness rubric's call.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::super::driver::{earlier_answers, EngineeringReview};
use super::super::questions::{Question, QuestionAnswer, QuestionKind};
use super::super::view::RepositoryView;
use super::super::OrientationPack;
use crate::{EngineeringClaim, RetrievalOutcome};

/// Words allowed between a measured key and the number a claim gives it.
const FILLERS: [&str; 6] = ["of", "is", "was", "at", "has", "reaches"];

pub(super) fn citation_failures(
    view: &RepositoryView<'_>,
    pack: &OrientationPack,
    questions: &[Question],
    review: &EngineeringReview,
) -> Vec<String> {
    let mut failures = Vec::new();
    let mut shown = pack.evidence_ids.clone();
    for record in &review.retrieval {
        if record.outcome != RetrievalOutcome::Answered {
            continue;
        }
        shown.extend(record.evidence_ids.iter().cloned());
        failures.extend(
            record
                .evidence_ids
                .iter()
                .filter(|id| !view.holds_evidence(id))
                .map(|id| {
                    format!(
                        "retrieval call {} issued {id}, which the inputs do not hold",
                        record.call_id
                    )
                }),
        );
    }
    let mut answers: Vec<(QuestionKind, &QuestionAnswer)> = Vec::new();
    for question in questions {
        shown.extend(question.evidence_ids.iter().cloned());
        let Some(answer) = review
            .questions
            .iter()
            .find(|record| record.question_id == question.question_id)
            .and_then(|record| record.answer.as_ref())
        else {
            continue;
        };
        failures.extend(answer_failures(view, question, answer, &shown, &answers));
        answers.push((question.kind, answer));
    }
    failures
}

fn answer_failures(
    view: &RepositoryView<'_>,
    question: &Question,
    answer: &QuestionAnswer,
    shown: &BTreeSet<String>,
    earlier: &[(QuestionKind, &QuestionAnswer)],
) -> Vec<String> {
    let id = question.question_id.as_str();
    let mut known = shown.clone();
    if question.kind == QuestionKind::RemediationPlan && !earlier.is_empty() {
        let depended = earlier
            .iter()
            .filter(|(_, answer)| question.depends_on.contains(&answer.question_id))
            .copied()
            .collect::<Vec<_>>();
        match earlier_answers(&depended, &view.source_digest) {
            Ok(rendered) => known.extend(rendered.evidence_ids),
            Err(error) => return vec![format!("{id}: the earlier answers do not render: {error}")],
        }
    }
    let earlier = earlier
        .iter()
        .map(|(_, answer)| (*answer).clone())
        .collect::<Vec<_>>();
    let mut failures = Vec::new();
    if let Err(error) = question.check_answer(answer, &known, &earlier) {
        failures.push(format!("{id}: {error}"));
    }
    failures.extend(
        answer
            .cited_ids()
            .into_iter()
            .filter(|cited| !view.holds_evidence(cited))
            .map(|cited| format!("{id} cites {cited}, which the inputs do not hold")),
    );
    failures
}

pub(super) fn contradiction_failures(
    view: &RepositoryView<'_>,
    review: &EngineeringReview,
) -> Vec<String> {
    review
        .answered()
        .flat_map(|answer| &answer.claims)
        .flat_map(|claim| claim_contradictions(view, claim))
        .collect()
}

fn claim_contradictions(view: &RepositoryView<'_>, claim: &EngineeringClaim) -> Vec<String> {
    let id = claim.claim_id.as_str();
    let both = claim
        .supporting_evidence_ids
        .iter()
        .filter(|cited| claim.counter_evidence_ids.contains(cited))
        .map(|cited| format!("claim {id} cites {cited} both for and against itself"));
    let proposition = claim.proposition.to_lowercase();
    let evidence = claim
        .supporting_evidence_ids
        .iter()
        .chain(&claim.counter_evidence_ids)
        .filter_map(|cited| {
            let finding = cited.strip_prefix("finding:")?;
            view.findings
                .iter()
                .find(|candidate| candidate.id == finding)
                .map(|finding| &finding.evidence)
        });
    let misquotes =
        misquoted_values(&proposition, evidence)
            .into_iter()
            .map(|(key, said, measured)| {
                format!(
                    "claim {id} gives {key} as {said}, but the cited findings measured {measured}"
                )
            });
    both.chain(misquotes).collect()
}

/// `(key, number the text gives, numbers measured)` for each numeric
/// evidence value the text restates as no cited finding measured it. A
/// claim about two findings may give each one's value. Only specific keys
/// (with an underscore, or eight characters or more) are matched, by
/// their own name or with spaces for underscores, as whole words.
fn misquoted_values<'e>(
    text: &str,
    evidence: impl IntoIterator<Item = &'e Value>,
) -> Vec<(String, String, String)> {
    let mut measured = BTreeMap::<&str, Vec<&Value>>::new();
    for (key, value) in evidence.into_iter().filter_map(Value::as_object).flatten() {
        if value.is_number() && (key.contains('_') || key.len() >= 8) {
            measured.entry(key).or_default().push(value);
        }
    }
    let mut misquotes = Vec::new();
    for (key, values) in measured {
        let spaced = key.replace('_', " ").to_lowercase();
        let forms = BTreeSet::from([key.to_lowercase(), spaced]);
        let matches = |said: f64| {
            values
                .iter()
                .filter_map(|value| value.as_f64())
                .any(|measured| (said - measured).abs() <= f64::EPSILON * measured.abs().max(1.0))
        };
        for said in forms.iter().flat_map(|form| numbers_after(text, form)) {
            if !matches(said) {
                let listed = values.iter().map(ToString::to_string).collect::<Vec<_>>();
                misquotes.push((key.to_string(), said.to_string(), listed.join(" or ")));
            }
        }
    }
    misquotes
}

/// The number following each whole-word occurrence of `word`, if one does
/// within a filler word or two.
fn numbers_after(text: &str, word: &str) -> Vec<f64> {
    let is_word = |character: char| character.is_alphanumeric() || character == '_';
    text.match_indices(word)
        .filter(|(start, _)| !text[..*start].chars().next_back().is_some_and(is_word))
        .filter_map(|(start, _)| {
            let rest = &text[start + word.len()..];
            if rest.chars().next().is_some_and(is_word) {
                return None;
            }
            number_after(rest)
        })
        .collect()
}

fn number_after(text: &str) -> Option<f64> {
    let mut rest = text;
    for _ in 0..3 {
        rest = rest.trim_start_matches(|character: char| {
            character.is_whitespace() || character == '=' || character == ':'
        });
        let end = rest
            .find(|character: char| !character.is_alphanumeric() && character != '.')
            .unwrap_or(rest.len());
        let token = &rest[..end];
        if let Ok(number) = token.trim_end_matches('.').parse::<f64>() {
            return Some(number);
        }
        if !FILLERS.contains(&token) {
            return None;
        }
        rest = &rest[end..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_restated_measure_must_match_the_finding() {
        let evidence = json!({"cyclomatic_complexity": 31, "threshold": 10, "lines": 900});
        let misquotes = |text: &str| misquoted_values(text, [&evidence]);
        assert!(misquotes("cyclomatic_complexity=31 makes it risky").is_empty());
        assert!(misquotes("its cyclomatic complexity of 31 is high").is_empty());
        assert_eq!(
            misquotes("cyclomatic complexity is 12, which is fine"),
            [(
                "cyclomatic_complexity".to_string(),
                "12".to_string(),
                "31".to_string()
            )]
        );
        assert!(
            misquotes("cyclomatic complexity of about 30").is_empty(),
            "an approximation names no exact value"
        );
        assert!(
            misquotes("lines 10 to 20 do the work").is_empty(),
            "short keys are too generic to match"
        );
        assert!(misquotes("max_cyclomatic_complexity_total 3").is_empty());
        let other = json!({"cyclomatic_complexity": 12});
        assert!(
            misquoted_values(
                "cyclomatic complexity 31 in one, cyclomatic complexity 12 in the other",
                [&evidence, &other]
            )
            .is_empty(),
            "a claim about two findings may give each one's value"
        );
    }
}
