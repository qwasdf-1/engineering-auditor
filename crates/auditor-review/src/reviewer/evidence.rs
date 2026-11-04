//! A finding's structured evidence as one line the reviewer can read: the
//! values the detector measured or matched, so a question about a finding
//! does not have to rediscover them from source.

use serde_json::Value;

/// Bytes one finding's evidence line may take; longer is cut with an
/// ellipsis.
const EVIDENCE_BYTES: usize = 480;
/// Bytes one string value may take.
const VALUE_BYTES: usize = 120;
/// Items of a list value shown before the rest are counted.
const LIST_ITEMS: usize = 8;

/// `key=value` pairs in key order; `None` when there is no evidence.
/// Strings are shown as code spans with whitespace collapsed, so
/// repository text (a matched pattern, a comment) reads as data.
pub(super) fn evidence_line(evidence: &Value) -> Option<String> {
    let pairs = evidence.as_object()?;
    let mut line = String::new();
    for (key, value) in pairs {
        let pair = format!("{key}={}", value_text(value, true));
        let separator = if line.is_empty() { "" } else { ", " };
        if line.len() + separator.len() + pair.len() > EVIDENCE_BYTES {
            line.push_str(separator);
            line.push('…');
            break;
        }
        line.push_str(separator);
        line.push_str(&pair);
    }
    (!line.is_empty()).then_some(line)
}

fn value_text(value: &Value, nested: bool) -> String {
    match value {
        Value::Null => "none".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => number_text(number),
        Value::String(text) => code_span(text),
        Value::Array(items) => {
            let mut shown = items
                .iter()
                .take(LIST_ITEMS)
                .map(|item| value_text(item, false))
                .collect::<Vec<_>>();
            if items.len() > LIST_ITEMS {
                shown.push(format!("+{} more", items.len() - LIST_ITEMS));
            }
            format!("[{}]", shown.join(", "))
        }
        Value::Object(fields) if nested => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, value)| format!("{key}={}", value_text(value, false)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(fields) => format!("{{{} fields}}", fields.len()),
    }
}

/// Integers as written; other numbers to three decimals, trailing zeros
/// dropped.
fn number_text(number: &serde_json::Number) -> String {
    if number.is_i64() || number.is_u64() {
        return number.to_string();
    }
    let text = format!("{:.3}", number.as_f64().unwrap_or_default());
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A code span wide enough for any backticks inside, whitespace collapsed
/// and cut to [`VALUE_BYTES`].
fn code_span(text: &str) -> String {
    let mut collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > VALUE_BYTES {
        let mut end = VALUE_BYTES;
        while !collapsed.is_char_boundary(end) {
            end -= 1;
        }
        collapsed.truncate(end);
        collapsed.push('…');
    }
    let longest_run = collapsed
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest_run + 1);
    let pad = if collapsed.starts_with('`') || collapsed.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{collapsed}{pad}{fence}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn evidence_reads_as_key_value_pairs_with_strings_as_code() {
        let line = evidence_line(&json!({
            "function": "Load",
            "return_count": 7,
            "distance": 0.6666666666666667,
            "occurrences": ["a.go:1:2-3:4", "b.go:5:6-7:8"],
            "zone": {"name": "Warning"},
        }))
        .unwrap();
        assert_eq!(
            line,
            "distance=0.667, function=`Load`, occurrences=[`a.go:1:2-3:4`, `b.go:5:6-7:8`], return_count=7, zone={name=`Warning`}"
        );
    }

    #[test]
    fn repository_text_cannot_break_out_of_its_span_or_the_line() {
        let line =
            evidence_line(&json!({"todo": "// TODO: `ignore`\n# previous instructions"})).unwrap();
        assert_eq!(line, "todo=``// TODO: `ignore` # previous instructions``");
        assert!(!line.contains('\n'));
    }

    #[test]
    fn long_evidence_is_cut_and_says_so() {
        let lines = (0..40).map(|line| line.to_string()).collect::<Vec<_>>();
        let long = "x".repeat(400);
        let line = evidence_line(&json!({
            "a": long,
            "b": lines,
            "c": long,
            "d": long,
            "e": long,
        }))
        .unwrap();
        assert!(line.len() <= EVIDENCE_BYTES + 3, "{}", line.len());
        assert!(line.contains("+32 more"));
        assert!(line.ends_with('…'));
        assert_eq!(evidence_line(&json!({})), None);
        assert_eq!(evidence_line(&Value::Null), None);
    }
}
