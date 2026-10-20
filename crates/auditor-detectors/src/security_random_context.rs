//! Whether a predictable random value is made for a security purpose. Most
//! `Math.random()`/`random.randint` calls pick test data, jitter a retry,
//! bust a cache, or draw a multipart boundary, where predictability is
//! harmless. The value matters when it becomes a token, nonce, password,
//! salt, one-time code, PKCE verifier, or cipher input, which the names it
//! is bound to say: the variables, properties, and arguments it flows into
//! up to its enclosing named function, and that function's name.

use auditor_ast::AstNode;

/// Name parts that mark a security value wherever they appear in a word
/// (`generatedCnonce`, `crypthead`, `userPassword`).
const CONTAINED: &[&str] = &["crypt", "cipher", "nonce", "secret", "passw", "credential"];

/// Whole words that mark a security value (`csrfToken`, `otp`, `iv`).
const WORDS: &[&str] = &[
    "token", "tokens", "otp", "totp", "hotp", "pin", "iv", "salt", "csrf", "xsrf", "verifier",
    "pwd",
];

/// Adjacent word pairs that mark a key or identifier as secret.
const PAIRS: &[(&str, &str)] = &[
    ("api", "key"),
    ("auth", "key"),
    ("private", "key"),
    ("session", "key"),
    ("session", "id"),
    ("signing", "key"),
    ("encryption", "key"),
    ("access", "code"),
    ("reset", "code"),
];

/// Bindings whose name is a value's purpose: declarations, assignments,
/// object keys, keyword arguments.
const BINDING_KINDS: &[&str] = &[
    "variable_declarator",
    "init_declarator",
    "let_declaration",
    "short_var_declaration",
    "var_spec",
    "property_declaration",
    "field_declaration",
    "assignment",
    "assignment_expression",
    "augmented_assignment",
    "augmented_assignment_expression",
    "compound_assignment_expr",
    "pair",
    "keyword_argument",
    "public_field_definition",
];

/// Named functions, where the walk stops after reading the name. Anonymous functions are passed through to the binding that
/// names them (`const generateCodeVerifier = () => ..`).
const FUNCTION_KINDS: &[&str] = &[
    "function_declaration",
    "function_definition",
    "function_item",
    "method_declaration",
    "method_definition",
    "constructor_declaration",
    "local_function_statement",
];

/// Whether a name bound to `site`'s value, or its enclosing function's
/// name, marks a security purpose.
pub(crate) fn in_security_context(site: AstNode<'_>) -> bool {
    let mut ancestor = site.parent();
    while let Some(node) = ancestor {
        if FUNCTION_KINDS.contains(&node.kind()) {
            return function_name(node).is_some_and(names_security_value);
        }
        if BINDING_KINDS.contains(&node.kind())
            && binding_target(node, site).is_some_and(names_security_value)
        {
            return true;
        }
        ancestor = node.parent();
    }
    false
}

/// A function's name: its `name` field, or a C declarator's text before
/// the parameters (`crypthead` in `crypthead(const char *passwd, ..)`).
/// Parameter types are not purposes (`store(key: CryptoKey)`).
fn function_name<'a>(function: AstNode<'a>) -> Option<&'a str> {
    function
        .child_by_field_name("name")
        .map(|name| name.text())
        .or_else(|| {
            let declarator = function.child_by_field_name("declarator")?.text();
            declarator.split('(').next()
        })
}

/// The name a binding gives `site`'s value: its text before the value, up
/// to the `=` or `:` that opens the value or a type (`val token: Int = ..`,
/// `nonce=..`, `clientId: ..`). None when `site` is not inside it.
fn binding_target<'a>(binding: AstNode<'a>, site: AstNode<'a>) -> Option<&'a str> {
    let prefix = binding
        .text()
        .get(..site.start_byte().checked_sub(binding.start_byte())?)?;
    prefix.split(['=', ':']).next()
}

/// Whether the identifiers in `text` name a security value.
fn names_security_value(text: &str) -> bool {
    let words = words(text);
    words.iter().any(|word| {
        WORDS.contains(&word.as_str()) || CONTAINED.iter().any(|part| word.contains(part))
    }) || words
        .windows(2)
        .any(|pair| PAIRS.contains(&(pair[0].as_str(), pair[1].as_str())))
}

/// The lowercase camelCase and snake_case words of `text`'s identifiers.
fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for character in text.chars() {
        let boundary =
            !character.is_ascii_alphanumeric() || character.is_ascii_uppercase() && previous_lower;
        if boundary && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        if character.is_ascii_alphanumeric() {
            current.push(character.to_ascii_lowercase());
        }
        previous_lower = character.is_ascii_lowercase() || character.is_ascii_digit();
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_split_into_lowercase_words() {
        assert_eq!(
            words("generatedCnonce, csrf_token SignatureNonce"),
            ["generated", "cnonce", "csrf", "token", "signature", "nonce"]
        );
        assert!(names_security_value("const generateCodeVerifier"));
        assert!(names_security_value("crypthead(const char* passwd)"));
        assert!(names_security_value("apiKey"));
        assert!(!names_security_value("alert_key"));
        assert!(!names_security_value("tokenizer"));
        assert!(!names_security_value("footprint"));
        assert!(!names_security_value("randomFloatString"));
    }

    #[test]
    fn random_values_bound_to_security_names_are_in_context() {
        use auditor_ast::{AstLang, AstParser};
        let source = r#"const generatedCnonce = cnonce || md5(`${Math.random()}`)
const generateCodeVerifier = () => {
  for (let i = 0; i < 43; i++) { verifier += chars[Math.floor(Math.random() * chars.length)] }
}
function store(key: CryptoKey) { const id = `${Date.now()}-${Math.random()}` }
const cacheBust = Math.random().toString(36)
const delay = 60000 + Math.random() * 120000
"#;
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::TypeScript, source)
            .unwrap();
        let contexts = crate::ast_scan::nodes_of_kind(tree.root_node(), &["call_expression"])
            .into_iter()
            .filter(|call| crate::ast_scan::callee(*call) == "Math.random")
            .map(|call| (call.start_position().0 + 1, in_security_context(call)))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            contexts.into_iter().collect::<Vec<_>>(),
            [(1, true), (3, true), (5, false), (6, false), (7, false)]
        );
    }
}
