//! Recovery for sources a grammar rejects in places, by blanking text with
//! spaces so every byte offset and line is kept and parsing again.
//!
//! For C, C++, and C#, one configuration of the preprocessor conditionals is
//! chosen first, when that removes errors. Then each declaration holding a
//! syntax error is fixed or quarantined: in C and C++, a macro in its header
//! is blanked when that lets it parse; otherwise the whole declaration is
//! quarantined. A quarantined declaration is a whole top-level item or a
//! whole member of a type, namespace, or preprocessor block, never part of a
//! function body, so every function left in the tree is complete: a function
//! the reparse finds overlapping a quarantined range is quarantined whole.
//! Recovery fails, and the file stays unparsed, when errors remain after
//! [`MAX_ROUNDS`] or when quarantine would cover half the source.

use std::ops::Range;

use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::AstLang;

#[path = "quarantine_branches.rs"]
mod branches;
#[path = "quarantine_macros.rs"]
mod macros;
#[path = "quarantine_nodes.rs"]
mod nodes;

use nodes::{cut_functions, error_count, error_units, has_error_within, merged};

/// Rounds of quarantine and reparse: blanking one declaration can expose an
/// error the parser had absorbed into it, or rejoin a function it had split.
const MAX_ROUNDS: usize = 6;

/// Reparses a file may spend trying macro candidates.
const MACRO_TRIALS: usize = 24;

/// Bytes of reparsing a file may spend trying macro candidates, so a large
/// file gets fewer trials.
const MACRO_TRIAL_BYTES: usize = 2_000_000;

/// Why text was removed from a recovered tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuarantineReason {
    /// A declaration holding a syntax error.
    SyntaxError,
    /// A preprocessor branch outside the chosen configuration.
    InactiveBranch,
    /// A macro in a declaration header.
    Macro,
}

/// Text removed from a recovered tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quarantine {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub reason: QuarantineReason,
    /// The removed original text.
    pub text: Box<str>,
}

pub(crate) struct Recovered {
    pub(crate) tree: Tree,
    pub(crate) removals: Vec<(Range<usize>, QuarantineReason)>,
}

/// A source being recovered: its text, tree, and removals so far.
struct Recovery<'a> {
    parser: &'a mut Parser,
    text: Vec<u8>,
    /// The byte offset of each line; blanking keeps every newline.
    line_starts: Vec<usize>,
    tree: Tree,
    removals: Vec<(Range<usize>, QuarantineReason)>,
    macro_trials: usize,
}

impl Recovery<'_> {
    /// The tree of the text with `ranges` also blanked, reparsed
    /// incrementally: blanking keeps lengths and lines, so the parser
    /// reuses every subtree outside the edits.
    fn trial(&mut self, ranges: &[Range<usize>]) -> Option<(Vec<u8>, Tree)> {
        let mut text = self.text.clone();
        let mut edited = self.tree.clone();
        for range in ranges {
            blank(&mut text, range.clone());
            let (start, end) = (self.point(range.start), self.point(range.end));
            edited.edit(&InputEdit {
                start_byte: range.start,
                old_end_byte: range.end,
                new_end_byte: range.end,
                start_position: start,
                old_end_position: end,
                new_end_position: end,
            });
        }
        // Blanking replaces whole UTF-8 sequences with ASCII spaces.
        let tree = self
            .parser
            .parse(std::str::from_utf8(&text).ok()?, Some(&edited))?;
        Some((text, tree))
    }

    fn point(&self, byte: usize) -> Point {
        let row = self.line_starts.partition_point(|start| *start <= byte) - 1;
        Point::new(row, byte - self.line_starts[row])
    }

    fn accept(
        &mut self,
        (text, tree): (Vec<u8>, Tree),
        ranges: &[Range<usize>],
        reason: QuarantineReason,
    ) {
        self.text = text;
        self.tree = tree;
        self.removals
            .extend(ranges.iter().map(|range| (range.clone(), reason)));
    }

    fn quarantined(&self) -> Vec<Range<usize>> {
        merged(
            self.removals
                .iter()
                .filter(|(_, reason)| *reason == QuarantineReason::SyntaxError)
                .map(|(range, _)| range.clone())
                .collect(),
        )
    }

    /// Quarantines the declarations holding a function that overlaps a
    /// quarantine: whether there were any, or `None` when reparsing fails.
    fn cut_split_functions(&mut self) -> Option<bool> {
        let cuts = cut_functions(self.tree.root_node(), &self.quarantined());
        if cuts.is_empty() {
            return Some(false);
        }
        let trial = self.trial(&cuts)?;
        self.accept(trial, &cuts, QuarantineReason::SyntaxError);
        Some(true)
    }

    /// Quarantines each declaration still holding an error, or `None` when
    /// quarantine would cover half of the `length` source bytes.
    fn quarantine_errors(&mut self, length: usize) -> Option<()> {
        let units = error_units(self.tree.root_node());
        if units.is_empty() {
            return Some(());
        }
        let covered = merged([self.quarantined(), units.clone()].concat())
            .iter()
            .map(Range::len)
            .sum::<usize>();
        if covered * 2 >= length {
            return None;
        }
        let trial = self.trial(&units)?;
        self.accept(trial, &units, QuarantineReason::SyntaxError);
        Some(())
    }

    /// Keeps one configuration of the preprocessor conditionals when that
    /// leaves fewer errors.
    fn choose_branches(&mut self) {
        let Ok(text) = std::str::from_utf8(&self.text) else {
            return;
        };
        let inactive = merged(branches::inactive_branches(text));
        if inactive.is_empty() {
            return;
        }
        if let Some(trial) = self.trial(&inactive) {
            if error_count(trial.1.root_node()) < error_count(self.tree.root_node()) {
                self.accept(trial, &inactive, QuarantineReason::InactiveBranch);
            }
        }
    }

    /// Blanks macros in the header of the declaration at `unit` while that
    /// clears its errors without adding others.
    fn elide_macros(&mut self, unit: Range<usize>) {
        let Ok(text) = std::str::from_utf8(&self.text) else {
            return;
        };
        for candidate in macros::candidates(text, unit.clone()) {
            if self.macro_trials == 0 {
                return;
            }
            self.macro_trials -= 1;
            let errors = error_count(self.tree.root_node());
            let Some(trial) = self.trial(std::slice::from_ref(&candidate)) else {
                return;
            };
            if error_count(trial.1.root_node()) < errors {
                let fixed = !has_error_within(trial.1.root_node(), &unit);
                self.accept(trial, &[candidate], QuarantineReason::Macro);
                if fixed {
                    return;
                }
            }
        }
    }
}

/// `parsed` of `lang` parsed again without the text holding its errors, or
/// `None` when recovery fails. `tree` is `parsed` under `parser`'s current
/// language.
pub(crate) fn recover(
    parser: &mut Parser,
    lang: &AstLang,
    tree: &Tree,
    parsed: &str,
) -> Option<Recovered> {
    let mut recovery = Recovery {
        parser,
        text: parsed.as_bytes().to_vec(),
        line_starts: line_starts(parsed),
        tree: tree.clone(),
        removals: Vec::new(),
        macro_trials: MACRO_TRIALS.min(MACRO_TRIAL_BYTES / parsed.len().max(1)),
    };
    if matches!(lang, AstLang::C | AstLang::Cpp | AstLang::CSharp) {
        recovery.choose_branches();
    }
    for _ in 0..MAX_ROUNDS {
        if !recovery.tree.root_node().has_error() {
            if recovery.cut_split_functions()? {
                continue;
            }
            return Some(Recovered {
                tree: recovery.tree,
                removals: recovery.removals,
            });
        }
        if matches!(lang, AstLang::C | AstLang::Cpp) {
            for unit in error_units(recovery.tree.root_node()) {
                recovery.elide_macros(unit);
            }
        }
        recovery.quarantine_errors(parsed.len())?;
    }
    None
}

/// The quarantine records for `removals` of `source`.
pub(crate) fn records(
    source: &str,
    removals: &[(Range<usize>, QuarantineReason)],
) -> Vec<Quarantine> {
    let line_starts = line_starts(source);
    let line_of = |byte: usize| line_starts.partition_point(|start| *start <= byte);
    let mut records = removals
        .iter()
        .map(|(range, reason)| Quarantine {
            start_byte: range.start,
            end_byte: range.end,
            start_line: line_of(range.start),
            end_line: line_of(range.end),
            reason: *reason,
            text: source[range.clone()].into(),
        })
        .collect::<Vec<_>>();
    records.sort_by_key(|record| (record.start_byte, record.end_byte));
    records
}

/// `source` with `removals` blanked.
pub(crate) fn blanked(source: &str, removals: &[(Range<usize>, QuarantineReason)]) -> String {
    let mut text = source.as_bytes().to_vec();
    for (range, _) in removals {
        blank(&mut text, range.clone());
    }
    String::from_utf8(text).unwrap_or_else(|_| source.to_string())
}

/// The byte offset of each line of `source`.
fn line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(source.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
}

fn blank(text: &mut [u8], range: Range<usize>) {
    for byte in &mut text[range] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

#[cfg(test)]
#[path = "quarantine_tests.rs"]
mod tests;
