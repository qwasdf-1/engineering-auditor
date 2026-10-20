//! One configuration of the C, C++, or C# preprocessor conditionals: the
//! first branch of each `#if` group is kept, or the next one when the first
//! is `#if 0`, every other branch is inactive, and the directives go. Branches that each hold half
//! of a declaration (`#if X` before one function header, `#else` before
//! another, one shared body) only parse with one of them removed.

use std::ops::Range;

struct Group {
    /// Whether a branch of this group has been kept.
    kept: bool,
    /// Whether the current branch is kept.
    active: bool,
}

/// The byte ranges a preprocessor would drop from `source` in this
/// configuration: every conditional directive line, and each non-kept
/// branch's body. A directive inside a declaration (`#if` within a base
/// list) is itself what the grammar cannot read.
pub(super) fn inactive_branches(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let active = |groups: &[Group]| groups.iter().all(|group| group.active);
        let blank = match directive(line) {
            Some(("if" | "ifdef" | "ifndef", condition)) => {
                let parent = active(&groups);
                let taken = parent && !is_false(condition);
                groups.push(Group {
                    kept: taken,
                    active: taken,
                });
                true
            }
            Some(("elif" | "elifdef" | "elifndef" | "else", condition)) if !groups.is_empty() => {
                let parent = active(&groups[..groups.len() - 1]);
                let group = groups.last_mut().expect("group");
                group.active = parent && !group.kept && !is_false(condition);
                group.kept |= group.active;
                true
            }
            Some(("endif", _)) => {
                groups.pop();
                true
            }
            _ => !active(&groups),
        };
        if blank {
            ranges.push(start..offset);
        }
    }
    ranges
}

/// A preprocessor conditional directive's keyword and condition.
fn directive(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start().strip_prefix('#')?.trim_start();
    let end = rest
        .find(|character: char| !character.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let (keyword, condition) = rest.split_at(end);
    matches!(
        keyword,
        "if" | "ifdef" | "ifndef" | "elif" | "elifdef" | "elifndef" | "else" | "endif"
    )
    .then(|| (keyword, condition.trim()))
}

/// `#if 0`, `#if false`: a branch no configuration compiles.
fn is_false(condition: &str) -> bool {
    let condition = condition.split("//").next().unwrap_or_default();
    let condition = condition.split("/*").next().unwrap_or_default().trim();
    matches!(condition, "0" | "false" | "(0)")
}
