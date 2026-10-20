//! Whether a credential-named target bound to a literal holds a credential,
//! or only names something about one: where it lives, what kind it is, or
//! the variable, key, or header that carries it.

use crate::util;

fn credential_context(target: &str) -> bool {
    let normalized = target
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "password",
        "passwd",
        "secret",
        "apikey",
        "accesstoken",
        "authtoken",
        "credential",
        "clientsecret",
        "privatekey",
    ]
    .iter()
    .any(|keyword| normalized.contains(keyword))
}

/// `(target, value)` when a credential-named target holds a literal that is
/// not a placeholder.
pub(super) fn credential_literal(target: String, value: String) -> Option<(String, String)> {
    if !credential_context(&target) || names_a_credential(&target, &value) {
        return None;
    }
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() < 4
        || value.contains('$')
        || [
            "password",
            "secret",
            "changeme",
            "example",
            "placeholder",
            "test",
            "xxx",
            "none",
            "provider",
            "warning",
            "api_key=",
        ]
        .contains(&normalized.as_str())
        || ["paste your", "enter your", "api key here", "token here"]
            .iter()
            .any(|marker| normalized.contains(marker))
    {
        return None;
    }
    Some((target, value))
}

/// Last words of a target that names something about a credential, not the
/// credential: where it lives, what kind it is, who owns it.
const NAMING_WORDS: &[&str] = &[
    "env",
    "var",
    "name",
    "header",
    "field",
    "param",
    "dir",
    "directory",
    "path",
    "file",
    "kind",
    "type",
    "owner",
    "version",
    "route",
    "prefix",
    "label",
];

/// Words before a final `key` that make it a lookup key (`SECRET_CONFIG_KEY`)
/// rather than a key (`SECRET_KEY`).
const LOOKUP_KEY_WORDS: &[&str] = &[
    "config", "setting", "settings", "storage", "store", "cache", "lookup", "map", "property",
    "metadata", "option",
];

/// A target or literal that names where a credential lives rather than
/// holding one: `API_KEY_ENV = "SERVICE_API_KEY"`, `TOKEN_HEADER = ".."`,
/// `SECRET_STORE_DIR = "secrets"`, `SECRET_BACKEND_CONFIG_KEY = ".."`, a
/// literal that is the target's own name (`api_key: "api_key"`), a property
/// key (`SOCKS_PASSWORD = "java.net.socks.password"`), or a character set
/// with no letter or digit (`PASSWORD_ENCODE_SET = " \"':;"`).
fn names_a_credential(target: &str, value: &str) -> bool {
    naming_target(target) || value.eq_ignore_ascii_case(target) || names_by_shape(value)
}

/// A target whose last word names something about a credential.
fn naming_target(target: &str) -> bool {
    match util::split_identifier(target).as_slice() {
        [.., before, last] if last == "key" => LOOKUP_KEY_WORDS.contains(&before.as_str()),
        [.., last] => NAMING_WORDS.contains(&last.as_str()),
        [] => false,
    }
}

/// An environment variable name, a property key, or symbols only.
fn names_by_shape(value: &str) -> bool {
    let environment_name = value.contains('_')
        && value.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        });
    let property_key = value.contains('.')
        && credential_context(value)
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || ".-_".contains(character)
        });
    environment_name || property_key || !value.chars().any(char::is_alphanumeric)
}
