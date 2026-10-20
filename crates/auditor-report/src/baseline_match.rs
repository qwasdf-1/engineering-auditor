//! Deterministic carried-finding matching between an accepted baseline and a
//! current scan. Exact finding-ID matches come first; the remaining findings
//! are paired in fixed passes, and a pass pairs a key only when exactly one
//! baseline entry and one current finding share it. Ambiguity never matches,
//! so a fixed finding plus an unrelated new one stays resolved plus new.

use std::collections::{BTreeMap, BTreeSet};

use auditor_core::{IDENTITY_ANCHOR_KEY, IDENTITY_ORDINAL_KEY};

/// The identity-bearing fields of one baseline entry or current finding.
#[derive(Clone, Copy, Debug)]
pub struct MatchSubject<'a> {
    pub id: &'a str,
    pub detector: &'a str,
    pub title: &'a str,
    pub path: &'a str,
    pub message: &'a str,
    pub identity: &'a BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CarryPass {
    /// Same anchor and subject; only the occurrence ordinal moved.
    Ordinal,
    /// Same file, subject, and normalized message; the anchor symbol was
    /// renamed or the flagged line was edited.
    Renamed,
    /// Same anchor, subject, and normalized message in a different file.
    Moved,
}

impl CarryPass {
    pub const ALL: [Self; 3] = [Self::Ordinal, Self::Renamed, Self::Moved];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ordinal => "ordinal",
            Self::Renamed => "renamed",
            Self::Moved => "moved",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|pass| pass.as_str() == value)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CarriedFinding {
    pub baseline_id: String,
    pub current_id: String,
    pub pass: CarryPass,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BaselineMatch {
    pub unchanged: BTreeSet<String>,
    pub carried: Vec<CarriedFinding>,
    /// Current finding IDs with no baseline counterpart.
    pub new: BTreeSet<String>,
    /// Baseline IDs with no current counterpart.
    pub resolved: BTreeSet<String>,
}

pub fn match_baseline(
    baseline: &[MatchSubject<'_>],
    current: &[MatchSubject<'_>],
) -> BaselineMatch {
    let current_ids = current
        .iter()
        .map(|subject| subject.id)
        .collect::<BTreeSet<_>>();
    let baseline_ids = baseline
        .iter()
        .map(|subject| subject.id)
        .collect::<BTreeSet<_>>();
    let mut old = baseline
        .iter()
        .filter(|subject| !current_ids.contains(subject.id))
        .copied()
        .collect::<Vec<_>>();
    let mut new = current
        .iter()
        .filter(|subject| !baseline_ids.contains(subject.id))
        .copied()
        .collect::<Vec<_>>();
    let mut carried = Vec::new();
    for pass in CarryPass::ALL {
        carried.extend(pair_unique(pass, &mut old, &mut new));
    }
    carried.sort();
    BaselineMatch {
        unchanged: baseline_ids
            .intersection(&current_ids)
            .map(|id| id.to_string())
            .collect(),
        carried,
        new: new.iter().map(|subject| subject.id.to_string()).collect(),
        resolved: old.iter().map(|subject| subject.id.to_string()).collect(),
    }
}

fn pair_unique(
    pass: CarryPass,
    old: &mut Vec<MatchSubject<'_>>,
    new: &mut Vec<MatchSubject<'_>>,
) -> Vec<CarriedFinding> {
    let mut groups: BTreeMap<String, (Vec<usize>, Vec<usize>)> = BTreeMap::new();
    for (index, subject) in old.iter().enumerate() {
        if let Some(key) = pass_key(pass, subject) {
            groups.entry(key).or_default().0.push(index);
        }
    }
    for (index, subject) in new.iter().enumerate() {
        if let Some(key) = pass_key(pass, subject) {
            groups.entry(key).or_default().1.push(index);
        }
    }
    let pairs = groups
        .into_values()
        .filter_map(|(old, new)| match (old.as_slice(), new.as_slice()) {
            ([old], [new]) => Some((*old, *new)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let carried = pairs
        .iter()
        .map(|&(old_index, new_index)| CarriedFinding {
            baseline_id: old[old_index].id.to_string(),
            current_id: new[new_index].id.to_string(),
            pass,
        })
        .collect();
    let matched_old = pairs.iter().map(|pair| pair.0).collect::<BTreeSet<_>>();
    let matched_new = pairs.iter().map(|pair| pair.1).collect::<BTreeSet<_>>();
    retain_unmatched(old, &matched_old);
    retain_unmatched(new, &matched_new);
    carried
}

fn retain_unmatched(subjects: &mut Vec<MatchSubject<'_>>, matched: &BTreeSet<usize>) {
    let mut index = 0;
    subjects.retain(|_| {
        index += 1;
        !matched.contains(&(index - 1))
    });
}

fn pass_key(pass: CarryPass, subject: &MatchSubject<'_>) -> Option<String> {
    let anchor = subject.identity.get(IDENTITY_ANCHOR_KEY);
    let structural = anchor.is_some_and(|anchor| anchor.starts_with("structural:"));
    let subject_identity = |drop_anchor: bool| {
        subject
            .identity
            .iter()
            .filter(|(key, _)| key.as_str() != IDENTITY_ORDINAL_KEY)
            .filter(|(key, _)| !(drop_anchor && key.as_str() == IDENTITY_ANCHOR_KEY))
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("\u{1f}")
    };
    let parts = match pass {
        CarryPass::Ordinal => vec![subject.path.to_string(), subject_identity(false)],
        CarryPass::Renamed if !structural => vec![
            subject.path.to_string(),
            subject_identity(true),
            normalized_message(subject.message, anchor.map(String::as_str)),
        ],
        CarryPass::Moved if !structural => vec![
            subject_identity(false),
            normalized_message(subject.message, anchor.map(String::as_str)),
        ],
        CarryPass::Renamed | CarryPass::Moved => return None,
    };
    let mut key = vec![subject.detector.to_string(), subject.title.to_string()];
    key.extend(parts);
    Some(key.join("\u{1e}"))
}

/// Remove what a rename or line shift legitimately changes: the anchoring
/// symbol's own name and `line N` positions. Everything else, including
/// metrics, must still agree.
fn normalized_message(message: &str, anchor: Option<&str>) -> String {
    let leaf = anchor
        .and_then(|anchor| anchor.strip_prefix("symbol:"))
        .and_then(|path| path.rsplit("::").next())
        .filter(|leaf| !leaf.is_empty());
    let message = match leaf {
        Some(leaf) => message.replace(&format!("`{leaf}`"), "`\u{0}`"),
        None => message.to_string(),
    };
    let mut normalized = String::with_capacity(message.len());
    let mut rest = message.as_str();
    while let Some(position) = rest.find("line ") {
        let (head, tail) = rest.split_at(position + "line ".len());
        normalized.push_str(head);
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            normalized.push('#');
        }
        rest = &tail[digits..];
    }
    normalized.push_str(rest);
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn subject<'a>(
        id: &'a str,
        path: &'a str,
        message: &'a str,
        identity: &'a BTreeMap<String, String>,
    ) -> MatchSubject<'a> {
        MatchSubject {
            id,
            detector: "cognitive_complexity",
            title: "Function cognitive complexity exceeds budget",
            path,
            message,
            identity,
        }
    }

    #[test]
    fn rename_with_unchanged_metrics_is_carried() {
        let old_identity = identity(&[("anchor", "symbol:Parser::parse")]);
        let new_identity = identity(&[("anchor", "symbol:Parser::parse_all")]);
        let result = match_baseline(
            &[subject(
                "EA-old",
                "src/a.rs",
                "Function `parse` has complexity 23 at line 4.",
                &old_identity,
            )],
            &[subject(
                "EA-new",
                "src/a.rs",
                "Function `parse_all` has complexity 23 at line 9.",
                &new_identity,
            )],
        );
        assert_eq!(
            result.carried,
            [CarriedFinding {
                baseline_id: "EA-old".to_string(),
                current_id: "EA-new".to_string(),
                pass: CarryPass::Renamed,
            }]
        );
        assert!(result.new.is_empty() && result.resolved.is_empty());
    }

    #[test]
    fn fixed_finding_plus_unrelated_new_finding_is_not_carried() {
        let old_identity = identity(&[("anchor", "symbol:parse")]);
        let new_identity = identity(&[("anchor", "symbol:render")]);
        let result = match_baseline(
            &[subject(
                "EA-old",
                "src/a.rs",
                "Function `parse` has complexity 23.",
                &old_identity,
            )],
            &[subject(
                "EA-new",
                "src/a.rs",
                "Function `render` has complexity 31.",
                &new_identity,
            )],
        );
        assert!(result.carried.is_empty());
        assert_eq!(result.new, BTreeSet::from(["EA-new".to_string()]));
        assert_eq!(result.resolved, BTreeSet::from(["EA-old".to_string()]));
    }

    #[test]
    fn moved_file_and_shifted_ordinal_are_carried_but_ambiguity_is_not() {
        let anchor = identity(&[("anchor", "symbol:parse")]);
        let second = identity(&[("anchor", "symbol:parse"), ("ordinal", "1")]);
        let moved = match_baseline(
            &[subject(
                "EA-a",
                "src/a.rs",
                "Function `parse` has complexity 23.",
                &anchor,
            )],
            &[subject(
                "EA-b",
                "src/b.rs",
                "Function `parse` has complexity 23.",
                &anchor,
            )],
        );
        assert_eq!(moved.carried[0].pass, CarryPass::Moved);
        let ordinal = match_baseline(
            &[subject("EA-a", "src/a.rs", "m", &second)],
            &[subject("EA-b", "src/a.rs", "m", &anchor)],
        );
        assert_eq!(ordinal.carried[0].pass, CarryPass::Ordinal);
        let ambiguous = match_baseline(
            &[subject("EA-a", "src/a.rs", "m", &anchor)],
            &[
                subject("EA-b", "src/b.rs", "m", &anchor),
                subject("EA-c", "src/c.rs", "m", &anchor),
            ],
        );
        assert!(ambiguous.carried.is_empty());
        assert_eq!(ambiguous.new.len(), 2);
    }

    #[test]
    fn message_normalization_keeps_metrics_and_drops_positions() {
        assert_eq!(
            normalized_message(
                "Function `f` at line 12 has 7 branches",
                Some("symbol:T::f")
            ),
            "Function `\u{0}` at line # has 7 branches"
        );
        assert_ne!(
            normalized_message("has 7 branches", None),
            normalized_message("has 8 branches", None)
        );
    }
}
