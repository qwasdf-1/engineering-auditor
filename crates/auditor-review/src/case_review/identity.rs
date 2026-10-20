use std::fmt;

use auditor_core::sha256_digest_bytes;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Map, Value};

const SHA256_HEX_LENGTH: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityError {
    InvalidContentDigest,
    InvalidStableId,
    InvalidTypedPrefix,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidContentDigest => {
                "content digest must use sha256:<64 lowercase hexadecimal>"
            }
            Self::InvalidStableId => "stable id must use <typed-prefix>:<64 lowercase hexadecimal>",
            Self::InvalidTypedPrefix => {
                "stable id prefix must contain lowercase ASCII letters, digits, or hyphens"
            }
        })
    }
}

impl std::error::Error for IdentityError {}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContentDigest(String);

impl ContentDigest {
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        let valid = value.strip_prefix("sha256:").is_some_and(valid_lower_hex);
        if !valid {
            return Err(IdentityError::InvalidContentDigest);
        }
        Ok(Self(value))
    }

    pub fn from_unprefixed(value: &str) -> Result<Self, IdentityError> {
        Self::parse(format!("sha256:{value}"))
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(format!("sha256:{}", sha256_digest_bytes(bytes)))
    }

    pub fn from_canonical_value(value: &Value) -> Self {
        Self::from_bytes(canonical_json(value).as_bytes())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ContentDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ContentDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct StableId(String);

impl StableId {
    pub fn from_identity(
        typed_prefix: &str,
        schema_version: u32,
        identity_fields: &Value,
    ) -> Result<Self, IdentityError> {
        if !valid_typed_prefix(typed_prefix) {
            return Err(IdentityError::InvalidTypedPrefix);
        }
        let identity = Value::Object(Map::from_iter([
            ("identity_fields".to_string(), canonicalize(identity_fields)),
            ("schema_version".to_string(), Value::from(schema_version)),
            ("typed_prefix".to_string(), Value::from(typed_prefix)),
        ]));
        Ok(Self(format!(
            "{typed_prefix}:{}",
            sha256_digest_bytes(canonical_json(&identity).as_bytes())
        )))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        let valid = value
            .split_once(':')
            .is_some_and(|(prefix, digest)| valid_typed_prefix(prefix) && valid_lower_hex(digest));
        if !valid {
            return Err(IdentityError::InvalidStableId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for StableId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for StableId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

pub trait CanonicalIdentity {
    fn identity_fields(&self) -> Value;
}

pub(crate) fn case_draft_id<T: Serialize>(
    typed_prefix: &str,
    schema_version: u32,
    case_id: &StableId,
    draft: &T,
) -> Result<StableId, IdentityError> {
    StableId::from_identity(
        typed_prefix,
        schema_version,
        &json!({"case_id": case_id, "draft": draft}),
    )
}

pub(crate) fn canonical_json(value: &Value) -> String {
    serde_json::to_string(&canonicalize(value)).expect("JSON value is serializable")
}

pub(crate) fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(left, _)| *left);
            let mut canonical = Map::new();
            for (key, value) in entries {
                canonical.insert(key.clone(), canonicalize(value));
            }
            Value::Object(canonical)
        }
        scalar => scalar.clone(),
    }
}

fn valid_typed_prefix(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_lower_hex(value: &str) -> bool {
    value.len() == SHA256_HEX_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
