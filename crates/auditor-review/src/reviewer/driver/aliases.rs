//! Short names for hash IDs in what the reviewer sees. A file, symbol,
//! finding, case, claim, or recommendation ID is a digest the reviewer
//! must copy exactly; the conversation shows `file:7` instead, and the
//! host turns every alias back into the ID before anything checks or
//! records it. Aliases are numbered per kind in the order the review
//! first shows them, so the same ID has the same alias in every question
//! and every repeated opening stays byte-identical for prompt caching.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::NativeConversationMessage;

/// Kinds whose IDs are digests, with the digest's shape after the colon.
const KINDS: [(&str, Shape); 6] = [
    ("contextual-recommendation", Shape::Hex64),
    ("engineering-claim", Shape::Hex64),
    ("engineering-case", Shape::Hex64),
    ("symbol", Shape::Hex64),
    ("file", Shape::Hex64),
    ("finding", Shape::Finding),
];

#[derive(Clone, Copy)]
enum Shape {
    /// 64 lowercase hex digits.
    Hex64,
    /// `EA-` and 16 lowercase hex digits.
    Finding,
}

impl Shape {
    /// The digest's length at the start of `rest`, if it is one.
    fn digest_len(self, rest: &[u8]) -> Option<usize> {
        let (prefix, digits): (&[u8], usize) = match self {
            Self::Hex64 => (b"", 64),
            Self::Finding => (b"EA-", 16),
        };
        let len = prefix.len() + digits;
        let hex = |byte: &u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte);
        (rest.len() >= len
            && rest.starts_with(prefix)
            && rest[prefix.len()..len].iter().all(hex)
            && !rest
                .get(len)
                .is_some_and(|byte| byte.is_ascii_alphanumeric()))
        .then_some(len)
    }
}

/// The review's alias table.
#[derive(Default)]
pub(super) struct Aliases {
    by_id: BTreeMap<String, String>,
    by_alias: BTreeMap<String, String>,
    /// Aliases minted so far, by kind.
    minted: BTreeMap<&'static str, usize>,
}

impl Aliases {
    /// `text` with every digest ID replaced by its alias, minting aliases
    /// for IDs shown for the first time.
    pub(super) fn shorten(&mut self, text: &str) -> String {
        self.rewrite(text, |aliases, kind, rest| {
            let len = kind.1.digest_len(rest)?;
            let id = format!("{}:{}", kind.0, std::str::from_utf8(&rest[..len]).ok()?);
            let alias = match aliases.by_id.get(&id) {
                Some(alias) => alias.clone(),
                None => {
                    let number = aliases.minted.entry(kind.0).or_default();
                    *number += 1;
                    let alias = format!("{}:{number}", kind.0);
                    aliases.by_id.insert(id.clone(), alias.clone());
                    aliases.by_alias.insert(alias.clone(), id);
                    alias
                }
            };
            Some((len, alias))
        })
    }

    /// `text` with every alias replaced by its ID; an alias the review
    /// never showed stays as written, so it fails as unknown evidence.
    pub(super) fn expand_text(&mut self, text: &str) -> String {
        self.rewrite(text, |aliases, kind, rest| {
            let len = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
            if len == 0
                || rest
                    .get(len)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                return None;
            }
            let alias = format!("{}:{}", kind.0, std::str::from_utf8(&rest[..len]).ok()?);
            aliases.by_alias.get(&alias).map(|id| (len, id.clone()))
        })
    }

    /// Tool arguments with every alias in every string expanded, except in
    /// a patch, which is source text.
    pub(super) fn expand(&mut self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.expand_text(text)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.expand(item)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, field)| {
                        let field = if key == "patch" {
                            field.clone()
                        } else {
                            self.expand(field)
                        };
                        (key.clone(), field)
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// A tool result as the reviewer sees it, with hash IDs aliased.
    pub(super) fn shown(
        &mut self,
        message: NativeConversationMessage,
    ) -> NativeConversationMessage {
        match message {
            NativeConversationMessage::ToolResult {
                call_id,
                tool_name,
                content_markdown,
            } => NativeConversationMessage::ToolResult {
                call_id,
                tool_name,
                content_markdown: self.shorten(&content_markdown),
            },
            other => other,
        }
    }

    /// Alias to ID, for the record.
    pub(super) fn table(&self) -> &BTreeMap<String, String> {
        &self.by_alias
    }

    /// Copies `text`, replacing each `kind:` token that `replace` accepts;
    /// `replace` gets the bytes after the colon and returns how many it
    /// consumed and what to write instead.
    fn rewrite(
        &mut self,
        text: &str,
        mut replace: impl FnMut(&mut Self, (&'static str, Shape), &[u8]) -> Option<(usize, String)>,
    ) -> String {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        let mut at = 0;
        while at < bytes.len() {
            let boundary = at == 0 || !is_id_byte(bytes[at - 1]);
            let hit = boundary
                .then(|| {
                    KINDS.iter().find_map(|kind| {
                        let head = kind.0.len() + 1;
                        (bytes[at..].starts_with(kind.0.as_bytes())
                            && bytes.get(at + kind.0.len()) == Some(&b':'))
                        .then(|| {
                            replace(self, *kind, &bytes[at + head..])
                                .map(|(len, with)| (head + len, with))
                        })
                        .flatten()
                    })
                })
                .flatten();
            match hit {
                Some((len, with)) => {
                    out.push_str(&text[copied..at]);
                    out.push_str(&with);
                    at += len;
                    copied = at;
                }
                None => at += 1,
            }
        }
        out.push_str(&text[copied..]);
        out
    }
}

fn is_id_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FILE: &str = "file:61d78c4a72f0cb72204ca681ae2493768f5bf1cb9850c8ec303ab672e4b20ed5";
    const SYMBOL: &str = "symbol:61aee2e7ed4222edd78833fad0bd98edfb4d664772d65c13a763f2d8c43603fe";

    #[test]
    fn digest_ids_get_stable_numbered_aliases_per_kind() {
        let mut aliases = Aliases::default();
        let text = format!("- a [{FILE}] b [finding:EA-725830b34e0612af]\n- {SYMBOL}, {FILE}");
        let short = aliases.shorten(&text);
        assert_eq!(short, "- a [file:1] b [finding:1]\n- symbol:1, file:1");
        assert_eq!(
            aliases.shorten(&text),
            short,
            "the same text shortens the same way"
        );
        assert_eq!(aliases.expand_text(&short), text);
    }

    #[test]
    fn only_whole_well_formed_digests_are_aliased() {
        let mut aliases = Aliases::default();
        for text in [
            "finding:EA-725830b34e0612a",   // too short
            "finding:EA-725830b34e0612afc", // too long: a mis-copy
            "module:internal/api",          // not a digest kind
            "source-file:7",                // `file` inside another word
            "file:internal/api.go",         // a path, not a digest
        ] {
            assert_eq!(aliases.shorten(text), text);
        }
        assert!(aliases.table().is_empty());
    }

    #[test]
    fn unknown_aliases_and_patches_are_left_as_written() {
        let mut aliases = Aliases::default();
        aliases.shorten(FILE);
        let arguments = json!({
            "claims": [{"supporting_evidence_ids": ["file:1", "file:9", "file:1x"], "proposition": "see file:1."}],
            "patch": "+ file:1\n",
        });
        assert_eq!(
            aliases.expand(&arguments),
            json!({
                "claims": [{"supporting_evidence_ids": [FILE, "file:9", "file:1x"], "proposition": format!("see {FILE}.")}],
                "patch": "+ file:1\n",
            })
        );
    }
}
