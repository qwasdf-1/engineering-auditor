//! A Java or C# method's declaration head: its annotations or attributes,
//! modifiers, and return type, up to the parameter list.

/// A Java method's text up to its parameter list, less its annotations'
/// arguments (`@Redirect(method = "tick")`), so a later annotation or
/// modifier still reads.
pub(crate) fn java_method_head(source: &str) -> String {
    let mut head = String::new();
    let mut rest = source;
    while let Some(open) = rest.find('(') {
        let before = &rest[..open];
        head.push_str(before);
        let annotation = before
            .split_whitespace()
            .next_back()
            .is_some_and(|word| word.starts_with('@'));
        match annotation
            .then(|| crate::util::closing_paren(&rest[open..]))
            .flatten()
        {
            Some(close) => rest = &rest[open + close + 1..],
            None => return head.trim().to_string(),
        }
    }
    head.push_str(rest);
    head.trim().to_string()
}

/// A C# method's text up to its parameter list, less the attribute lists
/// before it (`[DllImport("libc")]`).
pub(crate) fn csharp_method_head(source: &str) -> String {
    let mut rest = source.trim_start();
    while rest.starts_with('[') {
        let mut depth = 0usize;
        let Some(close) = rest.char_indices().find_map(|(index, character)| {
            match character {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                _ => {}
            }
            (character == ']' && depth == 0).then_some(index)
        }) else {
            break;
        };
        rest = rest[close + 1..].trim_start();
    }
    rest.split('(').next().unwrap_or(rest).trim().to_string()
}
