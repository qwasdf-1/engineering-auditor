use super::systems_facts::EffectKind;
use crate::structure_index::StructureCall;
use crate::Language;

pub(super) struct CallSignature {
    pub(super) qualified: String,
    method: String,
    receiver: String,
    /// A resolved package-qualified identity (`os.ReadFile`,
    /// `java.nio.file.Files.readString`), when the language resolves imports.
    package_api: Option<String>,
    language: Language,
    /// The call selects the last element by index (`xs.get(xs.size() - 1)`).
    selects_last: bool,
}

impl CallSignature {
    pub(super) fn from_call(call: &StructureCall, language: &Language) -> Self {
        let receiver = call
            .receiver
            .clone()
            .or_else(|| {
                call.callee
                    .rfind(['.', ':'])
                    .map(|index| call.callee[..index].trim_end_matches(':').to_string())
            })
            .unwrap_or_default();
        Self {
            qualified: call.receiver.as_ref().map_or_else(
                || call.callee.clone(),
                |value| format!("{value}.{}", call.callee),
            ),
            method: call
                .callee
                .rsplit([':', '.'])
                .find(|part| !part.is_empty())
                .unwrap_or(&call.callee)
                .to_ascii_lowercase(),
            receiver: receiver.to_ascii_lowercase(),
            package_api: call.api.clone().filter(|api| api.contains('.')),
            language: language.clone(),
            selects_last: call
                .arguments
                .as_deref()
                .is_some_and(|arguments| arguments.replace(' ', "").ends_with(".size()-1)")),
        }
    }
}

/// Go effects by resolved API identity. A package-qualified Go call is
/// classified by this table alone, so `url.Parse` is not a parse effect and
/// `os.ReadDir` is a repository scan whatever the package is imported as.
const GO_EFFECTS: &[(&str, EffectKind)] = &[
    ("os.ReadFile", EffectKind::ArtifactRead),
    ("io/ioutil.ReadFile", EffectKind::ArtifactRead),
    ("os.WriteFile", EffectKind::ArtifactWrite),
    ("io/ioutil.WriteFile", EffectKind::ArtifactWrite),
    ("encoding/json.Unmarshal", EffectKind::Parse),
    ("encoding/xml.Unmarshal", EffectKind::Parse),
    ("gopkg.in/yaml.v3.Unmarshal", EffectKind::Parse),
    ("gopkg.in/yaml.v2.Unmarshal", EffectKind::Parse),
    ("github.com/BurntSushi/toml.Unmarshal", EffectKind::Parse),
    ("encoding/json.Marshal", EffectKind::Serialize),
    ("encoding/json.MarshalIndent", EffectKind::Serialize),
    ("encoding/xml.Marshal", EffectKind::Serialize),
    ("gopkg.in/yaml.v3.Marshal", EffectKind::Serialize),
    ("gopkg.in/yaml.v2.Marshal", EffectKind::Serialize),
    ("regexp.Compile", EffectKind::ExpensiveInitialization),
    ("regexp.MustCompile", EffectKind::ExpensiveInitialization),
    ("text/template.New", EffectKind::ExpensiveInitialization),
    ("html/template.New", EffectKind::ExpensiveInitialization),
    ("os.ReadDir", EffectKind::RepositoryScan),
    ("io/ioutil.ReadDir", EffectKind::RepositoryScan),
    ("path/filepath.Walk", EffectKind::RepositoryScan),
    ("path/filepath.WalkDir", EffectKind::RepositoryScan),
    ("io/fs.WalkDir", EffectKind::RepositoryScan),
    ("net/http.Get", EffectKind::ExternalIo),
    ("net/http.Head", EffectKind::ExternalIo),
    ("net/http.Post", EffectKind::ExternalIo),
    ("net/http.PostForm", EffectKind::ExternalIo),
];

/// Java effects by resolved API identity. A Java call missing from this table
/// falls back to the name heuristics below (a `cache.add` is still a cache
/// insert), unlike Go, whose package calls are classified by table alone.
const JAVA_EFFECTS: &[(&str, &[EffectKind])] = &[
    (
        "java.nio.file.Files.readString",
        &[EffectKind::ArtifactRead, EffectKind::BlockingIo],
    ),
    (
        "java.nio.file.Files.readAllBytes",
        &[EffectKind::ArtifactRead, EffectKind::BlockingIo],
    ),
    (
        "java.nio.file.Files.readAllLines",
        &[EffectKind::ArtifactRead, EffectKind::BlockingIo],
    ),
    (
        "java.nio.file.Files.writeString",
        &[EffectKind::ArtifactWrite, EffectKind::BlockingIo],
    ),
    (
        "java.nio.file.Files.write",
        &[EffectKind::ArtifactWrite, EffectKind::BlockingIo],
    ),
    ("java.lang.Thread.sleep", &[EffectKind::BlockingIo]),
    (
        "com.fasterxml.jackson.databind.ObjectMapper.readValue",
        &[EffectKind::Parse],
    ),
    (
        "com.fasterxml.jackson.databind.ObjectMapper.readTree",
        &[EffectKind::Parse],
    ),
    (
        "com.fasterxml.jackson.databind.ObjectMapper.writeValueAsString",
        &[EffectKind::Serialize],
    ),
    (
        "com.fasterxml.jackson.databind.ObjectMapper.writeValueAsBytes",
        &[EffectKind::Serialize],
    ),
    ("com.google.gson.Gson.fromJson", &[EffectKind::Parse]),
    ("com.google.gson.Gson.toJson", &[EffectKind::Serialize]),
    (
        "java.util.regex.Pattern.compile",
        &[EffectKind::ExpensiveInitialization],
    ),
    ("java.nio.file.Files.list", &[EffectKind::RepositoryScan]),
    ("java.nio.file.Files.walk", &[EffectKind::RepositoryScan]),
    ("java.nio.file.Files.find", &[EffectKind::RepositoryScan]),
    (
        "java.nio.file.Files.newDirectoryStream",
        &[EffectKind::RepositoryScan],
    ),
    ("java.io.File.listFiles", &[EffectKind::RepositoryScan]),
    ("java.net.http.HttpClient.send", &[EffectKind::ExternalIo]),
    ("java.net.URL.openStream", &[EffectKind::ExternalIo]),
];

pub(super) fn classify_call(call: &CallSignature) -> Vec<EffectKind> {
    if let Some(api) = &call.package_api {
        match call.language {
            Language::Go => {
                return GO_EFFECTS
                    .iter()
                    .filter(|(name, _)| name == api)
                    .map(|(_, kind)| *kind)
                    .collect();
            }
            Language::Java => {
                if let Some((_, kinds)) = JAVA_EFFECTS.iter().find(|(name, _)| name == api) {
                    return kinds.to_vec();
                }
            }
            _ => {}
        }
    }
    [
        (EffectKind::ArtifactRead, is_artifact_read(call)),
        (EffectKind::ArtifactWrite, is_artifact_write(call)),
        (EffectKind::BlockingIo, is_blocking_io(call)),
        (EffectKind::ExternalIo, is_external_io(call)),
        (EffectKind::Parse, is_parse(call)),
        (EffectKind::Scan, is_scan(call)),
        (EffectKind::Serialize, is_serialize(call)),
        (EffectKind::CacheInsert, is_cache_insert(call)),
        (EffectKind::CacheLatestRead, is_cache_latest_read(call)),
        (EffectKind::CacheBound, is_cache_bound(call)),
        (
            EffectKind::ExpensiveInitialization,
            is_expensive_initialization(call),
        ),
        (EffectKind::RepositoryScan, is_repository_scan(call)),
    ]
    .into_iter()
    .filter_map(|(kind, present)| present.then_some(kind))
    .collect()
}

fn is_artifact_read(call: &CallSignature) -> bool {
    matches!(
        call.method.as_str(),
        "read_to_string" | "read_text" | "read_bytes" | "read_file" | "readfilesync"
    ) || call.qualified.eq_ignore_ascii_case("std::fs::read")
        || call.qualified.to_ascii_lowercase().contains("fs.readfile")
}

fn is_artifact_write(call: &CallSignature) -> bool {
    matches!(
        call.method.as_str(),
        "write_all" | "write_text" | "write_bytes" | "write_file" | "writefilesync"
    ) || call.qualified.eq_ignore_ascii_case("std::fs::write")
        || call.qualified.to_ascii_lowercase().contains("fs.writefile")
}

fn is_blocking_io(call: &CallSignature) -> bool {
    let qualified = call.qualified.to_ascii_lowercase();
    (is_artifact_read(call) || is_artifact_write(call))
        && (qualified.contains("std::fs")
            || qualified.contains("sync")
            || matches!(
                call.method.as_str(),
                "read_to_string" | "read_text" | "read_bytes" | "write_text" | "write_all" | "open"
            ))
}

fn is_parse(call: &CallSignature) -> bool {
    matches!(
        call.method.as_str(),
        "from_str" | "parse" | "loads" | "parse_str"
    ) && (names_data_format(call) || call.method == "parse")
}

/// A call qualified by a data-format library (`serde_json`, `yaml`).
fn names_data_format(call: &CallSignature) -> bool {
    let qualified = call.qualified.to_ascii_lowercase();
    ["json", "serde", "yaml", "toml"]
        .iter()
        .any(|marker| qualified.contains(marker))
}

fn is_scan(call: &CallSignature) -> bool {
    matches!(call.method.as_str(), "scan" | "visit" | "inspect")
}

fn is_serialize(call: &CallSignature) -> bool {
    match call.method.as_str() {
        "dumps" | "stringify" | "serialize" => true,
        "to_string" => names_data_format(call),
        _ => false,
    }
}

fn is_cache(call: &CallSignature) -> bool {
    call.receiver.contains("cache") || call.receiver.contains("history")
}

fn is_cache_insert(call: &CallSignature) -> bool {
    is_cache(call) && matches!(call.method.as_str(), "push" | "insert" | "append" | "add")
}

fn is_cache_latest_read(call: &CallSignature) -> bool {
    is_cache(call)
        && (matches!(
            call.method.as_str(),
            "last" | "back" | "latest" | "peek" | "getlast" | "peeklast"
        ) || (call.method == "get" && call.selects_last))
}

fn is_cache_bound(call: &CallSignature) -> bool {
    is_cache(call)
        && matches!(
            call.method.as_str(),
            "truncate"
                | "retain"
                | "pop_front"
                | "drain"
                | "set_max_size"
                | "remove"
                | "removefirst"
                | "pollfirst"
                | "poll"
                | "clear"
        )
}

fn is_repository_scan(call: &CallSignature) -> bool {
    let qualified = call.qualified.to_ascii_lowercase();
    ["discover_sources", "scan_repository", "walkdir"]
        .iter()
        .any(|marker| qualified.contains(marker))
        || matches!(
            call.method.as_str(),
            "read_dir"
                | "readdirsync"
                | "scandir"
                | "rglob"
                | "walk_repo"
                | "list_repository_files"
        )
}

fn is_external_io(call: &CallSignature) -> bool {
    let external_method = matches!(
        call.method.as_str(),
        "get" | "post" | "put" | "patch" | "delete" | "send" | "request" | "connect"
    );
    let known_module = matches!(
        call.receiver.as_str(),
        "reqwest" | "requests" | "http" | "https" | "urllib.request"
    ) || call.receiver.starts_with("requests.")
        || call.receiver.starts_with("reqwest::")
        || call.receiver.starts_with("urllib.request.");
    let network_client = call
        .receiver
        .rsplit(['.', ':'])
        .find(|part| !part.is_empty())
        .is_some_and(|part| {
            part == "client"
                || part == "socket"
                || part.ends_with("_client")
                || part.ends_with("_socket")
        });
    !is_database(call)
        && (call.method == "fetch"
            || call.method == "urlopen"
            || (external_method && (known_module || network_client)))
}

fn is_database(call: &CallSignature) -> bool {
    let qualified = call.qualified.to_ascii_lowercase();
    call.receiver.contains("repo")
        || call.receiver.contains("repository")
        || call.receiver.contains("database")
        || call.receiver == "db"
        || qualified.contains("query")
}

fn is_expensive_initialization(call: &CallSignature) -> bool {
    let qualified = call.qualified.to_ascii_lowercase();
    qualified.contains("regex::new")
        || qualified == "regexp"
        || qualified == "re.compile"
        || ((call.receiver.contains("client")
            || call.receiver.contains("parser")
            || call.receiver.contains("runtime")
            || call.receiver.contains("compiler"))
            && matches!(
                call.method.as_str(),
                "new" | "build" | "compile" | "connect"
            ))
}
