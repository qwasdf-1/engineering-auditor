//! Whether a predictable random value is made for a security purpose. Most
//! `Math.random()`/`random.randint` calls pick test data, jitter a retry,
//! bust a cache, or draw a multipart boundary, where predictability is
//! harmless. The value matters when it becomes a token, nonce, password,
//! salt, one-time code, PKCE verifier, or cipher input. The data-flow layer
//! follows the value to where it goes: a name or key that says so, a cookie,
//! HMAC, or cipher call, or an `Authorization` header; or the enclosing
//! function's name says so. A value a named function returns is judged by
//! its callers (`security_random_helpers`).

use auditor_ast::{call_arguments, AstNode, CallFlow, Destination, FunctionFlow};

use crate::dataflow_sources::callee_name;

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

/// Calls that make a value a secret: cookies, HMACs, signatures, ciphers,
/// and key derivation (lowercased callee names).
const SECURITY_SINKS: &[&str] = &[
    "cookie",
    "setcookie",
    "set_cookie",
    "createhmac",
    "hmac",
    "sign",
    "encrypt",
    "createcipheriv",
    "pbkdf2",
    "pbkdf2sync",
    "scrypt",
    "hashpw",
];

/// Header names whose values are credentials.
const SECRET_HEADERS: &[&str] = &["authorization", "x-api-key", "x-csrf", "cookie"];

/// What a predictable random value is used for.
pub(crate) enum RandomUse<'a> {
    /// A security value.
    Security,
    /// Returned from this named function, whose callers decide.
    ReturnedFrom(AstNode<'a>),
    Other,
}

/// What the random value `site` (a call or expression) is used for.
pub(crate) fn random_use(site: AstNode<'_>) -> RandomUse<'_> {
    let flow = FunctionFlow::enclosing(site);
    if enclosing_function_names(site, &flow).any(names_security_value) {
        return RandomUse::Security;
    }
    let destinations = flow.destinations(site, &random_policy);
    if destinations.iter().any(is_security_destination) {
        return RandomUse::Security;
    }
    let returned = destinations.iter().any(|destination| {
        matches!(destination, Destination::Returned { function } if *function == flow.scope())
    });
    match flow.function_name() {
        Some(_) if returned => RandomUse::ReturnedFrom(flow.scope()),
        _ => RandomUse::Other,
    }
}

/// A random value flows through every call except one that makes it a
/// secret, where the walk stops to report it.
fn random_policy(call: AstNode<'_>) -> CallFlow {
    if is_security_sink(call) {
        CallFlow::Opaque
    } else {
        CallFlow::All
    }
}

fn is_security_destination(destination: &Destination<'_>) -> bool {
    match destination {
        Destination::Bound(binding) => names_security_value(binding.name),
        Destination::Property { key } => names_security_value(key.text()),
        Destination::Argument { call, .. } | Destination::Receiver(call) => is_security_sink(*call),
        Destination::Returned { .. } => false,
    }
}

fn is_security_sink(call: AstNode<'_>) -> bool {
    let name = callee_name(call).to_ascii_lowercase();
    if SECURITY_SINKS.contains(&name.as_str()) {
        return true;
    }
    let header = matches!(
        name.as_str(),
        "setheader" | "set_header" | "header" | "set" | "append" | "add"
    );
    header
        && call_arguments(call).first().is_some_and(|key| {
            let key = key.text().to_ascii_lowercase();
            SECRET_HEADERS.iter().any(|secret| key.contains(secret))
        })
}

/// The names of the functions around `site`: the named scope's, and each
/// anonymous function's binding (`const generateCodeVerifier = () => ..`,
/// `getValue: () => ..`).
fn enclosing_function_names<'a>(
    site: AstNode<'a>,
    flow: &FunctionFlow<'a>,
) -> impl Iterator<Item = &'a str> {
    let scope = flow.scope();
    std::iter::successors(site.parent(), AstNode::parent)
        .take_while(move |ancestor| *ancestor != scope)
        .filter_map(anonymous_function_name)
        .chain(flow.function_name())
}

fn anonymous_function_name(node: AstNode<'_>) -> Option<&str> {
    if !matches!(
        node.kind(),
        "arrow_function" | "function_expression" | "function" | "lambda" | "lambda_expression"
    ) {
        return None;
    }
    let holder = node.parent()?;
    ["name", "key", "left"]
        .iter()
        .find_map(|field| holder.child_by_field_name(field))
        .filter(|name| *name != node)
        .map(|name| name.text())
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
            .map(|call| {
                (
                    call.start_position().0 + 1,
                    matches!(random_use(call), RandomUse::Security),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            contexts.into_iter().collect::<Vec<_>>(),
            [(1, true), (3, true), (5, false), (6, false), (7, false)]
        );
    }
}
