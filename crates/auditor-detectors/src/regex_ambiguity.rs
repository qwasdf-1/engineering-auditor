//! Exponential backtracking read from a regex pattern alone: an unbounded
//! repetition whose body can match the same text in more than one way
//! (`(a+)+`, `(\w+\s?)*`, `(?:[^"\\]+|\\.)*`). A body with a second
//! mandatory part (`(?:,\s*\w+)*`) splits its input one way only.
//! Possessive quantifiers and atomic groups never backtrack. Patterns the
//! parser cannot read fail closed.

struct Term {
    min: u32,
    unbounded: bool,
    possessive: bool,
    zero_width: bool,
    group: Option<Group>,
    start: usize,
    end: usize,
}

struct Group {
    alternatives: Vec<Vec<Term>>,
    atomic: bool,
}

/// The first ambiguous repetition in `pattern`, as its source text.
pub(crate) fn ambiguous_repetition(pattern: &str) -> Option<String> {
    let mut parser = Parser {
        chars: pattern.chars().collect(),
        position: 0,
    };
    let alternatives = parser.alternation(0)?;
    if parser.position != parser.chars.len() {
        return None;
    }
    first_ambiguous(&alternatives).map(|term| parser.chars[term.start..term.end].iter().collect())
}

fn first_ambiguous(alternatives: &[Vec<Term>]) -> Option<&Term> {
    for term in alternatives.iter().flatten() {
        let Some(group) = &term.group else {
            continue;
        };
        let repeats = term.unbounded && !term.possessive && !term.zero_width && !group.atomic;
        if repeats
            && group
                .alternatives
                .iter()
                .any(|alternative| ambiguous(alternative))
        {
            return Some(term);
        }
        if let Some(found) = first_ambiguous(&group.alternatives) {
            return Some(found);
        }
    }
    None
}

/// A body that splits text more than one way: at most one of its consuming
/// parts is mandatory, and every mandatory part and at least one part is
/// itself repeatable. A part with fixed delimiters (`{[\d,]+}`) matches its
/// text one way, however variable its inside.
fn ambiguous(alternative: &[Term]) -> bool {
    let consuming = alternative
        .iter()
        .filter(|term| !term.zero_width)
        .collect::<Vec<_>>();
    let mandatory = consuming
        .iter()
        .filter(|term| !nullable(term))
        .collect::<Vec<_>>();
    consuming.iter().any(|term| repeatable(term))
        && mandatory.len() <= 1
        && mandatory.iter().all(|term| repeatable(term))
}

fn nullable(term: &Term) -> bool {
    term.zero_width
        || term.min == 0
        || term.group.as_ref().is_some_and(|group| {
            group
                .alternatives
                .iter()
                .any(|alternative| alternative.iter().all(nullable))
        })
}

/// A part whose text two consecutive matches of it could also form: an
/// unbounded repetition, or a group whose body is ambiguous itself.
fn repeatable(term: &Term) -> bool {
    !term.possessive
        && !term.zero_width
        && (term.unbounded
            || term.group.as_ref().is_some_and(|group| {
                !group.atomic
                    && group
                        .alternatives
                        .iter()
                        .any(|alternative| ambiguous(alternative))
            }))
}

struct Parser {
    chars: Vec<char>,
    position: usize,
}

const MAX_DEPTH: usize = 64;

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn advance(&mut self) {
        self.position += 1;
    }

    fn alternation(&mut self, depth: usize) -> Option<Vec<Vec<Term>>> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut alternatives = vec![Vec::new()];
        while let Some(character) = self.peek() {
            match character {
                ')' => break,
                '|' => {
                    self.advance();
                    alternatives.push(Vec::new());
                }
                _ => {
                    if let Some(term) = self.term(depth)? {
                        alternatives.last_mut()?.push(term);
                    }
                }
            }
        }
        Some(alternatives)
    }

    /// One atom and its quantifier; `Some(None)` for a construct that
    /// matches nothing (an inline flag group, a comment).
    fn term(&mut self, depth: usize) -> Option<Option<Term>> {
        let start = self.position;
        let (group, zero_width) = match self.peek()? {
            '\\' => {
                let anchor = matches!(
                    self.chars.get(start + 1),
                    Some('b' | 'B' | 'A' | 'z' | 'Z' | 'G')
                );
                self.escape()?;
                (None, anchor)
            }
            '[' => {
                self.class()?;
                (None, false)
            }
            '(' => match self.group(depth)? {
                Some((group, zero_width)) => (Some(group), zero_width),
                None => return Some(None),
            },
            '^' | '$' => {
                self.advance();
                (None, true)
            }
            '*' | '+' | '?' => return None,
            _ => {
                self.advance();
                (None, false)
            }
        };
        let (min, unbounded, possessive) = self.quantifier();
        Some(Some(Term {
            min,
            unbounded,
            possessive,
            zero_width,
            group,
            start,
            end: self.position,
        }))
    }

    fn escape(&mut self) -> Option<()> {
        self.advance();
        let escaped = self.peek()?;
        self.advance();
        match escaped {
            'x' | 'u' | 'p' | 'P' | 'N' if self.peek() == Some('{') => self.skip_past('}'),
            'k' => match self.peek() {
                Some('<') => self.skip_past('>'),
                Some('{') => self.skip_past('}'),
                Some('\'') => {
                    self.advance();
                    self.skip_past('\'')
                }
                _ => Some(()),
            },
            // Java `\Q..\E` quotes its contents literally.
            'Q' => {
                while self.position + 1 < self.chars.len()
                    && !(self.chars[self.position] == '\\' && self.chars[self.position + 1] == 'E')
                {
                    self.advance();
                }
                self.position = (self.position + 2).min(self.chars.len());
                Some(())
            }
            _ => Some(()),
        }
    }

    fn skip_past(&mut self, close: char) -> Option<()> {
        while let Some(character) = self.peek() {
            self.advance();
            if character == close {
                return Some(());
            }
        }
        None
    }

    fn class(&mut self) -> Option<()> {
        self.advance();
        if self.peek() == Some('^') {
            self.advance();
        }
        if self.peek() == Some(']') {
            self.advance();
        }
        while let Some(character) = self.peek() {
            self.advance();
            match character {
                '\\' => self.advance(),
                ']' => return Some(()),
                _ => {}
            }
        }
        None
    }

    /// A group and whether it is zero-width (a lookaround); `Some(None)` for
    /// an inline flag group, a comment, or a named backreference.
    fn group(&mut self, depth: usize) -> Option<Option<(Group, bool)>> {
        self.advance();
        let (atomic, zero_width) = match self.group_prefix()? {
            Some(kind) => kind,
            None => return Some(None),
        };
        let alternatives = self.alternation(depth + 1)?;
        if self.peek() != Some(')') {
            return None;
        }
        self.advance();
        Some(Some((
            Group {
                alternatives,
                atomic,
            },
            zero_width,
        )))
    }

    /// The `(?..` prefix after a group's `(`: `(atomic, zero_width)` for a
    /// group with a body, `Some(None)` for one that matches nothing.
    fn group_prefix(&mut self) -> Option<Option<(bool, bool)>> {
        if self.peek() != Some('?') {
            return Some(Some((false, false)));
        }
        self.advance();
        let marker = self.peek()?;
        match marker {
            ':' | '=' | '!' | '>' => {
                self.advance();
                Some(Some((marker == '>', matches!(marker, '=' | '!'))))
            }
            '<' => self.angle_group(),
            'P' => self.python_named_group(),
            '\'' => {
                self.advance();
                self.skip_past('\'')?;
                Some(Some((false, false)))
            }
            '#' => self.skip_past(')').map(|()| None),
            _ => self.inline_flags(),
        }
    }

    /// `(?<=..)` and `(?<!..)` look behind; `(?<name>..)` captures.
    fn angle_group(&mut self) -> Option<Option<(bool, bool)>> {
        self.advance();
        let lookbehind = matches!(self.peek()?, '=' | '!');
        if lookbehind {
            self.advance();
        } else {
            self.skip_past('>')?;
        }
        Some(Some((false, lookbehind)))
    }

    /// Python's `(?P<name>..)` captures; `(?P=name)` is a backreference.
    fn python_named_group(&mut self) -> Option<Option<(bool, bool)>> {
        self.advance();
        let named = self.peek()? == '<';
        self.skip_past(if named { '>' } else { ')' })?;
        Some(named.then_some((false, false)))
    }

    /// `(?i)` sets flags and matches nothing; `(?i:..)` is a group.
    fn inline_flags(&mut self) -> Option<Option<(bool, bool)>> {
        while matches!(self.peek(), Some(flag) if flag.is_ascii_alphabetic() || flag == '-') {
            self.advance();
        }
        let directive = self.peek()? == ')';
        if !directive && self.peek()? != ':' {
            return None;
        }
        self.advance();
        Some((!directive).then_some((false, false)))
    }

    /// `(min, unbounded, possessive)`; a brace that is not a valid bound is
    /// a literal and is left unconsumed.
    fn quantifier(&mut self) -> (u32, bool, bool) {
        let (min, unbounded) = match self.peek() {
            Some('*') => {
                self.advance();
                (0, true)
            }
            Some('+') => {
                self.advance();
                (1, true)
            }
            Some('?') => {
                self.advance();
                (0, false)
            }
            Some('{') => match self.bounds() {
                Some(bounds) => bounds,
                None => return (1, false, false),
            },
            _ => return (1, false, false),
        };
        let possessive = match self.peek() {
            Some('+') => {
                self.advance();
                true
            }
            Some('?') => {
                self.advance();
                false
            }
            _ => false,
        };
        (min, unbounded, possessive)
    }

    /// `{n}`, `{n,}`, or `{n,m}` at the cursor, consumed.
    fn bounds(&mut self) -> Option<(u32, bool)> {
        let body = self.chars[self.position + 1..]
            .iter()
            .take_while(|character| **character != '}')
            .collect::<String>();
        if self.position + 1 + body.chars().count() >= self.chars.len() {
            return None;
        }
        let (low, high) = match body.split_once(',') {
            Some((low, high)) => (low, Some(high)),
            None => (body.as_str(), None),
        };
        let min = low.trim().parse::<u32>().ok()?;
        let unbounded = match high.map(str::trim) {
            None => false,
            Some("") => true,
            Some(high) => {
                high.parse::<u32>().ok()?;
                false
            }
        };
        self.position += body.chars().count() + 2;
        Some((min, unbounded))
    }
}

#[cfg(test)]
mod tests {
    use super::ambiguous_repetition;

    #[test]
    fn reports_repetitions_that_split_text_more_than_one_way() {
        for (pattern, group) in [
            ("^(a+)+$", "(a+)+"),
            ("(\\w+\\s?)*$", "(\\w+\\s?)*"),
            ("\"(?:[^\"\\\\]+|\\\\.)*\"", "(?:[^\"\\\\]+|\\\\.)*"),
            ("(a*)*b", "(a*)*"),
            ("^((ab)+)+$", "((ab)+)+"),
            ("([^,]+,?)*;", "([^,]+,?)*"),
            ("(?:x+){2,}y", "(?:x+){2,}"),
            ("(?<word>\\d+)+!", "(?<word>\\d+)+"),
        ] {
            assert_eq!(
                ambiguous_repetition(pattern).as_deref(),
                Some(group),
                "{pattern}"
            );
        }
    }

    #[test]
    fn ignores_repetitions_with_one_split() {
        for pattern in [
            "(?:,\\s*\\w+)*",
            "([a-z0-9]+[-_])*",
            "(\\.\\d+)*",
            "(a+){3}",
            "(a+)?",
            "^\\w+$",
            "(?>a+)+",
            "(a++)+",
            "(?i)abc",
            "x{2,",
            "(?=a+)+",
            "[(a+)+]",
            "\\(a+\\)+",
            "(a|b)+",
            "(?:(?:{[\\d,]+})|[^}])+",
            "(unclosed",
        ] {
            assert_eq!(ambiguous_repetition(pattern), None, "{pattern}");
        }
    }
}
