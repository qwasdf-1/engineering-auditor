//! C and C++ API tables for L1 rules: findings-free site finders over
//! resolved callee identity. C functions are global, so a call's identity is
//! its callee name, with C++'s `std::` or `::` qualification dropped; a
//! function or macro of that name the file defines shadows the library one.
//! Semantics follow CERT C and the libraries' own guidance (OpenSSL, libcurl,
//! libxml2, libssh), with the Bandit and gosec rules as models.

use std::collections::BTreeSet;

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::bound_reads::reads_of_target;
use crate::java_api_rules::is_shell_invocation;

/// The functions and macros a C or C++ file defines, which shadow library
/// names.
pub(crate) struct CScope {
    defined: BTreeSet<String>,
}

impl CScope {
    pub(crate) fn new(root: AstNode<'_>) -> Self {
        let mut defined = BTreeSet::new();
        for definition in ast_scan::nodes_of_kind(root, &["function_definition"]) {
            if let Some(name) = function_name(definition) {
                defined.insert(name.to_string());
            }
        }
        for macro_definition in
            ast_scan::nodes_of_kind(root, &["preproc_def", "preproc_function_def"])
        {
            if let Some(name) = macro_definition.child_by_field_name("name") {
                defined.insert(name.text().to_string());
            }
        }
        Self { defined }
    }

    /// The library function a call names: its callee, without `std::` or
    /// `::`, unless the file defines that name itself.
    pub(crate) fn resolve_call<'a>(&self, call: AstNode<'a>) -> Option<&'a str> {
        if call.kind() != "call_expression" {
            return None;
        }
        let function = call.child_by_field_name("function")?;
        let name = match function.kind() {
            "identifier" => function.text(),
            "qualified_identifier" => {
                let scope = function
                    .named_children()
                    .find(|child| child.kind() == "namespace_identifier")
                    .map(|scope| scope.text());
                if !matches!(scope, None | Some("std")) {
                    return None;
                }
                function.child_by_field_name("name")?.text()
            }
            _ => return None,
        };
        (!self.defined.contains(name)).then_some(name)
    }

    /// Every resolved call in `root` that is compiled in, in source order.
    pub(crate) fn calls<'a>(&self, root: AstNode<'a>) -> Vec<(&'a str, AstNode<'a>)> {
        ast_scan::nodes_of_kind(root, &["call_expression"])
            .into_iter()
            .filter(|call| !is_compiled_out(*call))
            .filter_map(|call| Some((self.resolve_call(call)?, call)))
            .collect()
    }
}

/// Whether `node` sits in an `#if 0` block, which the preprocessor drops.
pub(crate) fn is_compiled_out(node: AstNode<'_>) -> bool {
    std::iter::successors(node.parent(), |parent| parent.parent()).any(|ancestor| {
        ancestor.kind() == "preproc_if"
            && ancestor
                .child_by_field_name("condition")
                .is_some_and(|condition| condition.text().trim() == "0")
    })
}

/// The declared name of a function definition.
pub(crate) fn function_name(definition: AstNode<'_>) -> Option<&str> {
    let mut declarator = definition.child_by_field_name("declarator")?;
    loop {
        match declarator.kind() {
            "identifier" | "field_identifier" => return Some(declarator.text()),
            "qualified_identifier" => {
                return declarator
                    .child_by_field_name("name")
                    .map(|name| name.text())
            }
            _ => declarator = declarator.child_by_field_name("declarator")?,
        }
    }
}

/// The argument expressions of a call.
pub(crate) fn c_arguments(call: AstNode<'_>) -> Vec<AstNode<'_>> {
    call.child_by_field_name("arguments")
        .map(|list| {
            list.named_children()
                .filter(|child| child.kind() != "comment")
                .collect()
        })
        .unwrap_or_default()
}

/// The value of a C integer literal (`0777`, `0x1ff`, `0L`).
pub(crate) fn c_integer(node: AstNode<'_>) -> Option<u64> {
    if node.kind() != "number_literal" {
        return None;
    }
    let text = node
        .text()
        .trim_end_matches(['u', 'U', 'l', 'L'])
        .replace('\'', "")
        .to_ascii_lowercase();
    if let Some(digits) = text.strip_prefix("0x") {
        u64::from_str_radix(digits, 16).ok()
    } else if let Some(digits) = text.strip_prefix("0b") {
        u64::from_str_radix(digits, 2).ok()
    } else if text.len() > 1 && text.starts_with('0') {
        u64::from_str_radix(&text[1..], 8).ok()
    } else {
        text.parse().ok()
    }
}

/// The function definition enclosing `node`.
pub(crate) fn enclosing_c_function(node: AstNode<'_>) -> Option<AstNode<'_>> {
    std::iter::successors(node.parent(), |parent| parent.parent())
        .find(|ancestor| ancestor.kind() == "function_definition")
}

/// Whether the file defines `main`: a program, whose standard streams and
/// exit status are its interface.
pub(crate) fn defines_c_main(root: AstNode<'_>) -> bool {
    ast_scan::nodes_of_kind(root, &["function_definition"])
        .into_iter()
        .any(|definition| {
            matches!(
                function_name(definition),
                Some("main" | "wmain" | "WinMain")
            )
        })
}

/// Calls in `root` to one of `functions`.
pub(crate) fn c_calls_to<'a>(root: AstNode<'a>, functions: &[&str]) -> Vec<AstNode<'a>> {
    CScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(name, _)| functions.contains(name))
        .map(|(_, call)| call)
        .collect()
}

/// `exit` and `quick_exit` outside `main`: a library that ends the process
/// takes the decision from its caller. `_exit` is how a forked child that
/// failed to `exec` must end (POSIX), so it is not reported.
pub(crate) fn c_exits(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    c_calls_to(root, &["exit", "quick_exit"])
        .into_iter()
        .filter(|call| {
            enclosing_c_function(*call)
                .and_then(function_name)
                .is_none_or(|name| !matches!(name, "main" | "wmain" | "WinMain"))
        })
        .collect()
}

/// Writes to the process's standard streams (`printf`, `puts`,
/// `fprintf(stderr, ..)`, `std::cout <<`) outside a program that defines
/// `main`.
pub(crate) fn c_console_output(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    if defines_c_main(root) {
        return Vec::new();
    }
    const STANDARD_STREAM: &[&str] = &["stdout", "stderr"];
    let mut sites = CScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(name, call)| match *name {
            "printf" | "puts" | "putchar" | "vprintf" | "wprintf" => true,
            "fprintf" | "vfprintf" | "fwprintf" => c_arguments(*call)
                .first()
                .is_some_and(|stream| STANDARD_STREAM.contains(&stream.text())),
            "fputs" | "fputc" | "fwrite" => c_arguments(*call)
                .last()
                .is_some_and(|stream| STANDARD_STREAM.contains(&stream.text())),
            _ => false,
        })
        .map(|(_, call)| call)
        .collect::<Vec<_>>();
    sites.extend(
        ast_scan::nodes_of_kind(root, &["binary_expression"])
            .into_iter()
            .filter(|expression| !is_compiled_out(*expression))
            .filter(|expression| {
                expression.child_by_field_name("left").is_some_and(|left| {
                    matches!(
                        left.text(),
                        "std::cout" | "std::cerr" | "std::clog" | "std::wcout" | "std::wcerr"
                    )
                })
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// TODO comments.
pub(crate) fn c_todo_nodes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::todo_comments(root, &["comment"])
}

/// `system`, `popen`, and their wide/Windows forms run a command line through
/// the shell (CERT ENV33-C), as does `execl("/bin/sh", "sh", "-c", ..)`.
/// `system(NULL)` only asks whether a shell exists.
pub(crate) fn c_shell_commands(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    CScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(name, call)| {
            let arguments = c_arguments(*call);
            match *name {
                "system" | "_wsystem" | "popen" | "_popen" | "_wpopen" => arguments
                    .first()
                    .is_some_and(|command| !matches!(command.text(), "NULL" | "nullptr" | "0")),
                "execl" | "execlp" | "execle" => {
                    let values = arguments.iter().map(|argument| ast_scan::string_literal_content(*argument)).collect::<Vec<_>>();
                    matches!(
                        (values.get(1), values.get(2)),
                        (Some(Some(program)), Some(Some(flag))) if is_shell_invocation(program, flag)
                    )
                }
                _ => false,
            }
        })
        .map(|(_, call)| call)
        .collect()
}

/// A socket bound to every interface: `INADDR_ANY` stored or converted
/// (`addr.sin_addr.s_addr = htonl(INADDR_ANY)`), `in6addr_any` stored, or
/// `0.0.0.0` parsed into an address.
pub(crate) fn c_bind_all_interfaces(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = ast_scan::nodes_of_kind(root, &["identifier"])
        .into_iter()
        .filter(|identifier| matches!(identifier.text(), "INADDR_ANY" | "in6addr_any"))
        .filter(|identifier| {
            let parent = identifier.parent();
            let in_htonl = parent
                .filter(|list| list.kind() == "argument_list")
                .is_some_and(|list| {
                    list.parent()
                        .and_then(|call| call.child_by_field_name("function"))
                        .is_some_and(|function| function.text() == "htonl")
                });
            let stored = parent.is_some_and(|parent| {
                matches!(parent.kind(), "assignment_expression" | "init_declarator")
                    && parent
                        .child_by_field_name(if parent.kind() == "init_declarator" {
                            "value"
                        } else {
                            "right"
                        })
                        .is_some_and(|value| value.start_byte() == identifier.start_byte())
            });
            in_htonl || stored
        })
        .collect::<Vec<_>>();
    sites.extend(
        c_calls_to(root, &["inet_addr", "inet_aton", "inet_pton"])
            .into_iter()
            .filter(|call| {
                c_arguments(*call)
                    .iter()
                    .filter_map(|argument| ast_scan::string_literal_content(*argument))
                    .any(|address| matches!(address.as_str(), "0.0.0.0" | "::"))
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// `chmod`, `fchmod`, `mkdir`, `open`, and `creat` with an others-write
/// mode, and `umask(0)` (CERT FIO06-C).
pub(crate) fn c_world_writable_modes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const MODE_SETTERS: &[(&str, usize)] = &[
        ("chmod", 1),
        ("fchmod", 1),
        ("lchmod", 1),
        ("mkdir", 1),
        ("creat", 1),
        ("open", 2),
        ("fchmodat", 2),
        ("mkdirat", 2),
        ("openat", 3),
    ];
    CScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(name, call)| {
            let arguments = c_arguments(*call);
            if *name == "umask" {
                return arguments.first().and_then(|mask| c_integer(*mask)) == Some(0);
            }
            MODE_SETTERS
                .iter()
                .find(|(setter, _)| setter == name)
                .and_then(|(_, index)| arguments.get(*index))
                .is_some_and(|mode| {
                    c_integer(*mode).is_some_and(|mode| mode & 0o002 != 0)
                        || mode
                            .text()
                            .split(|c: char| !c.is_alphanumeric() && c != '_')
                            .any(|word| matches!(word, "S_IWOTH" | "S_IRWXO"))
                })
        })
        .map(|(_, call)| call)
        .collect()
}

/// `rand`, `random`, and the `rand48` family are predictable; secrets need
/// `getrandom`, `arc4random`, or a CSPRNG (CERT MSC30-C).
pub(crate) fn c_predictable_random(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    c_calls_to(
        root,
        &[
            "rand", "random", "rand_r", "drand48", "erand48", "lrand48", "nrand48", "mrand48",
            "jrand48",
        ],
    )
}

/// Peer verification switched off: libcurl's `CURLOPT_SSL_VERIFYPEER` or
/// `CURLOPT_SSL_VERIFYHOST` set to 0, or OpenSSL's `SSL_VERIFY_NONE`.
pub(crate) fn c_disabled_certificate_checks(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    CScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(name, call)| {
            let arguments = c_arguments(*call);
            match *name {
                "curl_easy_setopt" => {
                    arguments.get(1).is_some_and(|option| {
                        matches!(
                            option.text(),
                            "CURLOPT_SSL_VERIFYPEER"
                                | "CURLOPT_SSL_VERIFYHOST"
                                | "CURLOPT_PROXY_SSL_VERIFYPEER"
                                | "CURLOPT_PROXY_SSL_VERIFYHOST"
                        )
                    }) && arguments.get(2).is_some_and(|value| {
                        c_integer(*value) == Some(0) || matches!(value.text(), "false" | "FALSE")
                    })
                }
                "SSL_CTX_set_verify" | "SSL_set_verify" => arguments
                    .get(1)
                    .is_some_and(|mode| mode.text() == "SSL_VERIFY_NONE"),
                _ => false,
            }
        })
        .map(|(_, call)| call)
        .collect()
}

/// SSL 2/3 or TLS 1.0/1.1: OpenSSL's version-specific methods and minimum
/// versions, and libcurl's `CURLOPT_SSLVERSION`.
pub(crate) fn c_deprecated_tls_versions(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const WEAK_METHODS: &[&str] = &[
        "SSLv2_method",
        "SSLv2_client_method",
        "SSLv2_server_method",
        "SSLv3_method",
        "SSLv3_client_method",
        "SSLv3_server_method",
        "TLSv1_method",
        "TLSv1_client_method",
        "TLSv1_server_method",
        "TLSv1_1_method",
        "TLSv1_1_client_method",
        "TLSv1_1_server_method",
    ];
    const WEAK_VERSIONS: &[&str] = &[
        "SSL2_VERSION",
        "SSL3_VERSION",
        "TLS1_VERSION",
        "TLS1_1_VERSION",
        "CURL_SSLVERSION_SSLv2",
        "CURL_SSLVERSION_SSLv3",
        "CURL_SSLVERSION_TLSv1",
        "CURL_SSLVERSION_TLSv1_0",
        "CURL_SSLVERSION_TLSv1_1",
    ];
    CScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(name, call)| {
            let arguments = c_arguments(*call);
            WEAK_METHODS.contains(name)
                || (matches!(
                    *name,
                    "SSL_CTX_set_min_proto_version" | "SSL_set_min_proto_version"
                ) && arguments
                    .get(1)
                    .is_some_and(|version| WEAK_VERSIONS.contains(&version.text())))
                || (*name == "curl_easy_setopt"
                    && arguments
                        .get(1)
                        .is_some_and(|option| option.text() == "CURLOPT_SSLVERSION")
                    && arguments
                        .get(2)
                        .is_some_and(|version| WEAK_VERSIONS.contains(&version.text())))
        })
        .map(|(_, call)| call)
        .collect()
}

/// libssh's `SSH_OPTIONS_STRICTHOSTKEYCHECK` set to 0 through a local
/// initialized to 0.
pub(crate) fn c_ssh_insecure_host_keys(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    c_calls_to(root, &["ssh_options_set"])
        .into_iter()
        .filter(|call| {
            let arguments = c_arguments(*call);
            let Some(value) = arguments.get(2) else {
                return false;
            };
            arguments
                .get(1)
                .is_some_and(|option| option.text() == "SSH_OPTIONS_STRICTHOSTKEYCHECK")
                && value.text().strip_prefix('&').is_some_and(|variable| {
                    enclosing_c_function(*call).is_some_and(|function| {
                        ast_scan::nodes_of_kind(function, &["init_declarator"])
                            .into_iter()
                            .any(|declarator| {
                                declarator
                                    .child_by_field_name("declarator")
                                    .is_some_and(|name| name.text() == variable.trim())
                                    && declarator.child_by_field_name("value").and_then(c_integer)
                                        == Some(0)
                            })
                    })
                })
        })
        .collect()
}

/// Broken digests and ciphers through OpenSSL (`MD5`, `SHA1`, `EVP_md5`,
/// `EVP_des_*`, `EVP_rc4`, ECB modes), with the algorithm's display name.
pub(crate) fn c_weak_cryptography(root: AstNode<'_>) -> Vec<(AstNode<'_>, &'static str)> {
    CScope::new(root)
        .calls(root)
        .into_iter()
        .filter_map(|(name, call)| {
            let algorithm = match name {
                "MD4" | "MD4_Init" | "MD5" | "MD5_Init" | "EVP_md4" | "EVP_md5" => "MD5",
                "SHA1" | "SHA1_Init" | "EVP_sha1" => "SHA-1",
                "RC4" | "RC4_set_key" | "EVP_rc4" | "EVP_rc4_40" => "RC4",
                _ if name.starts_with("EVP_des_") || name.starts_with("DES_") => "DES",
                _ if name.starts_with("EVP_rc2_") => "RC2",
                _ if name.starts_with("EVP_bf_") => "Blowfish",
                _ if name.starts_with("EVP_") && name.ends_with("_ecb") => "ECB mode",
                _ => return None,
            };
            Some((call, algorithm))
        })
        .collect()
}

/// libxml2 parses that substitute entities or load external DTDs
/// (`XML_PARSE_NOENT`, `XML_PARSE_DTDLOAD`), and
/// `xmlSubstituteEntitiesDefault(1)`.
pub(crate) fn c_xxe_parsers(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    CScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(name, call)| {
            let arguments = c_arguments(*call);
            match *name {
                "xmlSubstituteEntitiesDefault" => {
                    arguments.first().and_then(|value| c_integer(*value)) == Some(1)
                }
                _ if name.starts_with("xmlRead")
                    || name.starts_with("xmlCtxtRead")
                    || *name == "xmlCtxtUseOptions" =>
                {
                    arguments.last().is_some_and(|options| {
                        options
                            .text()
                            .split(|c: char| !c.is_alphanumeric() && c != '_')
                            .any(|word| matches!(word, "XML_PARSE_NOENT" | "XML_PARSE_DTDLOAD"))
                    })
                }
                _ => false,
            }
        })
        .map(|(_, call)| call)
        .collect()
}

/// `FILE *f = fopen(..)` (or `f = fopen(..)`) in a function that neither
/// closes `f` nor hands it on: passing it to anything but a stdio routine,
/// The identifier a C declarator names, under any pointer stars.
fn declared_identifier(mut declarator: AstNode<'_>) -> AstNode<'_> {
    while let Some(inner) = declarator.child_by_field_name("declarator") {
        declarator = inner;
    }
    declarator
}

/// returning it, or storing it transfers ownership (CERT FIO42-C).
pub(crate) fn c_unclosed_files(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const STDIO: &[&str] = &[
        "fread", "fwrite", "fgets", "fputs", "fprintf", "vfprintf", "fscanf", "vfscanf", "fgetc",
        "fputc", "getc", "putc", "fseek", "fseeko", "ftell", "ftello", "rewind", "fflush", "feof",
        "ferror", "clearerr", "setvbuf", "setbuf", "fileno", "ungetc", "fgetpos", "fsetpos",
        "getline", "getdelim",
    ];
    let scope = CScope::new(root);
    let mut sites = Vec::new();
    for call in scope
        .calls(root)
        .into_iter()
        .filter(|(name, _)| matches!(*name, "fopen" | "fdopen" | "freopen" | "tmpfile"))
        .map(|(_, call)| call)
    {
        let Some(holder) = call.parent() else {
            continue;
        };
        let variable = match holder.kind() {
            "init_declarator" => holder
                .child_by_field_name("declarator")
                .map(declared_identifier),
            "assignment_expression" => holder
                .child_by_field_name("left")
                .filter(|left| left.kind() == "identifier"),
            _ => None,
        };
        let Some(variable) = variable.filter(|_| enclosing_c_function(call).is_some()) else {
            continue;
        };
        let handled = reads_of_target(variable).into_iter().any(|usage| {
            let Some(parent) = usage.parent() else {
                return true;
            };
            match parent.kind() {
                "argument_list" => parent
                    .parent()
                    .and_then(|call| scope.resolve_call(call))
                    .is_none_or(|callee| !STDIO.contains(&callee)),
                // Tests of the handle (`if (!f)`, `f == NULL`) are not uses.
                "unary_expression"
                | "binary_expression"
                | "parenthesized_expression"
                | "condition_clause" => false,
                _ => true,
            }
        });
        if !handled {
            sites.push(holder);
        }
    }
    sites
}

/// Debugger traps left in code: `__debugbreak`, `__builtin_debugtrap`,
/// `DebugBreak`, and `raise(SIGTRAP)`. A trap the next statement follows with
/// `abort()` is a deliberate fatal-error breakpoint (libuv's `uv_fatal_error`),
/// not a leftover.
pub(crate) fn c_debugger_calls(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    CScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(name, call)| match *name {
            "__debugbreak" | "__builtin_debugtrap" | "DebugBreak" => true,
            "raise" => c_arguments(*call)
                .first()
                .is_some_and(|signal| signal.text() == "SIGTRAP"),
            _ => false,
        })
        .map(|(_, call)| call)
        .filter(|call| !precedes_abort(*call))
        .collect()
}

fn precedes_abort(call: AstNode<'_>) -> bool {
    let Some(statement) = call
        .parent()
        .filter(|parent| parent.kind() == "expression_statement")
    else {
        return false;
    };
    statement.parent().is_some_and(|block| {
        block
            .named_children()
            .skip_while(|child| child.start_byte() != statement.start_byte())
            .nth(1)
            .is_some_and(|next| {
                let text = next.text().split_whitespace().collect::<String>();
                matches!(text.as_str(), "abort();" | "std::abort();")
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    #[test]
    fn library_calls_resolve_unless_the_file_defines_the_name() {
        let source = "#define popen my_popen\nint rand(void) { return 4; }\nvoid f(const char *c) { system(c); popen(c, \"r\"); rand(); std::exit(1); obj.system(c); }\n";
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::Cpp, source)
            .unwrap();
        let calls = CScope::new(tree.root_node())
            .calls(tree.root_node())
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        assert_eq!(calls, ["system", "exit"]);
        let integer = |text: &str| {
            let tree = AstParser::new()
                .unwrap()
                .parse(&AstLang::C, &format!("int x = {text};"))
                .unwrap();
            c_integer(ast_scan::nodes_of_kind(tree.root_node(), &["number_literal"])[0])
        };
        assert_eq!(integer("0777"), Some(0o777));
        assert_eq!(integer("0x1ffL"), Some(0x1ff));
        assert_eq!(integer("0"), Some(0));
    }
}
