# Layer 1 - Correctness

Profile: 16 gating (all scans) + 23 full-only (full scans only)

Detectors: 39. The generated per-language matrix is authoritative:
[`language-support.md`](./language-support.md).

## Execution Contract

Every L1 owner has one catalog spec, one dispatcher owner, positive and negative
capability repositories, and an explicit mode for every catalog language.
Semantic owners enter through `ast_scan.rs`, reuse the scan-owned source and AST
caches, and fail closed in token mode or when parsing reports errors. The
visitor permits production and tooling roles, excludes test paths, and follows
Rust `#[test]` and `#[cfg(test)]` attribute siblings through ancestor scopes.

Exact lexical owners remain lexical only where that is the declared end-state
contract. They still receive language and source-role gating before emission.
No L1 owner constructs a private parser or rereads source from disk.

`dead_code` is the one cross-file L1 owner. It may inspect test-evolution
context while emitting only on its production-role view. All other L1 owners
are file-local or bounded intraprocedural checks.

## Owner Inventory

| Detector | Profile | Active claim | Execution model |
|---|---|---|---|
| `todo_unimplemented_in_production` | gating | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | AST macro/comment nodes outside test ancestry; Go TODO comments and `panic` with an unfinished-work message; Java TODO comments and `UnsupportedOperationException` with an unfinished-work message; JS/TS TODO comments and a thrown `Error` with an unfinished-work message; Python `# TODO` comments |
| `assert_in_production` | gating | Python, Java, Kotlin, C# | AST `assert` statements, except type-narrowing asserts; Java `assert` on a parameter of a public method or constructor (disabled without `-ea`, so it cannot enforce an argument contract) |
| `eval_in_production` | gating | Python, JavaScript, TypeScript, Java, Kotlin, C# | exact `eval`/`exec` call nodes; Java `ScriptEngine.eval` and `GroovyShell.evaluate`; JS/TS `eval`, `Function`, `vm`, and string `setTimeout`/`setInterval` (ESLint `no-eval`, `no-new-func`, `no-implied-eval`) |
| `hardcoded_credential` | gating | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | assignment nodes, literal and entropy evidence; Rust `let`/`const`/`static`/assignment/field initializers; a literal shaped like an environment-variable name, or a target naming where a credential lives (`_ENV`, `_VAR`, `_NAME`, `_HEADER`, `_FIELD`, `_PARAM`), is not a credential, nor is a target naming text about one (`_ID`, `_HELP`, `_MESSAGE`, `_ERROR`, `_TEMPLATE`, `_QUERY`, `_CONDITION`, `_PATTERN`, `_REGEX`), a literal whose words are mostly the target's own (`CLIENT_CREDENTIALS = "clientCredentials"`), a Fetch `credentials` mode, a regular expression with alternation, or multi-line text other than a PEM block; JS/TS declarators, assignments, object keys, and class fields; Go declarations, assignments, and struct keys; Java declarators and plain assignments |
| `unsafe_deserialization` | gating | Python, JavaScript, TypeScript, Java, Kotlin, C# | unsafe deserializer call nodes; Java `new ObjectInputStream`/`new XMLDecoder`; JS/TS `node-serialize`/`serialize-to-js` revivers; `yaml.load` with `SafeLoader`, `CSafeLoader`, or `BaseLoader` is exempt |
| `bind_all_interfaces` | gating | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | bind/listen calls containing an all-interface literal; Go `net`/`net/http`/`crypto/tls` listeners on a host-less or wildcard address; Java `InetSocketAddress`/`InetAddress.getByName` on a wildcard host or a port alone; JS/TS `listen` on a literal wildcard host; Rust `bind` of a literal `0.0.0.0`/`[::]` address, and `SocketAddr` built from an unspecified address |
| `try_except_pass` | full | Python | exact lexical bare-except/pass contract |
| `world_writable_file` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | `chmod` call plus parsed mode bits; Go `os` mode setters with the world-write bit (`os.ModePerm` included); Java `PosixFilePermissions.fromString` with others-write, `OTHERS_WRITE`, `File.setWritable(true, false)`; JS/TS `fs` mode arguments or `{ mode }` options with the world-write bit; Rust `Permissions::from_mode`, `set_mode`, and `OpenOptionsExt`/`DirBuilderExt` `mode` octal literals with the world-write bit |
| `hardcoded_tmp_directory` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | AST string literals naming an unsafe temp directory or a path under it (`/tmp:rw,..` mount specs excluded); string literals in every Tier-1 language |
| `flask_debug_true` | full | Python | exact lexical Flask debug contract |
| `random_for_security` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | `random.*` call nodes except seeding; Go `math/rand` calls; Java `java.util.Random`, `Math.random`, `ThreadLocalRandom`, `RandomStringUtils`; JS/TS `Math.random()`; Rust `fastrand`, `oorandom`, `SmallRng`, xorshift and PCG generators, and `seed_from_u64` (`thread_rng` and `OsRng` are cryptographically secure); reported only when the value, followed through the function, reaches a name or key that names a security purpose (token, nonce, secret, password, salt, OTP, PIN, IV, CSRF, PKCE verifier, cipher, API or session key), a cookie, HMAC, signing, cipher, or key-derivation call, or a credential header, or the enclosing function's name names one; a helper returning the value is reported at each resolved call whose result is used so, with the helper in `via` evidence |
| `request_no_cert_verify` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | HTTP call nodes with `verify=False`; Go `tls.Config` with `InsecureSkipVerify: true`; Java hostname verifiers returning `true`, empty `checkServerTrusted`, Apache no-op verifier/trust-all strategy; JS/TS `rejectUnauthorized: false`, `strictSSL: false`, `NODE_TLS_REJECT_UNAUTHORIZED=0`; Rust `danger_accept_invalid_certs(true)`/`danger_accept_invalid_hostnames(true)`, OpenSSL `SslVerifyMode::NONE`, a rustls `verify_server_cert` that always asserts |
| `xml_xxe_vulnerable` | full | Python, JavaScript, TypeScript, Java, Kotlin, C#, C, C++ | vulnerable XML parser call families; Java parser factories in a method that parses without DOCTYPE or external-entity hardening; JS/TS `libxmljs` parsing with `noent: true` |
| `ssl_with_bad_defaults` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | `ssl.wrap_socket` with obsolete protocol constants; Go `tls.Config` minimum or maximum below TLS 1.2; Java `SSLContext.getInstance`/`setEnabledProtocols` naming SSL or TLS 1.0/1.1; JS/TS `secureProtocol`/`minVersion`/`maxVersion` naming SSL or TLS 1.0/1.1; Rust native-tls `Protocol` and OpenSSL `SslVersion` minimum or maximum naming SSL 3 or TLS 1.0/1.1 |
| `ssh_no_host_key_verification` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | permissive Paramiko host-key policy calls; Go `ssh.InsecureIgnoreHostKey()`; JSch `StrictHostKeyChecking=no`, sshj `PromiscuousVerifier`; JS/TS `ssh2` `hostVerifier` that always returns `true`; Rust russh `check_server_key` returning `Ok(true)` |
| `assignment_in_operand` | gating | Rust, JavaScript, TypeScript, Java, C#, C, C++ | assignment-expression nodes inside `if`/`while` conditions; JS/TS/Java `if`/`while`/`do` conditions that are an assignment and nothing else (ESLint `no-cond-assign` default; `(line = r.readLine()) != null` is allowed) |
| `resource_not_closed` | full | Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C | local `open` assignment and same-owner close evidence; Go `os.Open`/`Create`/`OpenFile` handle neither closed nor handed on; Java file streams outside try-with-resources, neither closed nor handed on; JS/TS `fs.openSync` descriptors and `fs.promises.open` handles neither closed nor handed on |
| `empty_conditional_branch` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | empty AST branch bodies; Python `if`/`elif`/`else`/`for`/`while` blocks that only `pass`, without a comment; Rust `if`/`else` blocks with nothing in them, not even a comment; Go and Java empty `if`/`else` blocks without a comment |
| `unreachable_code` | gating | ten grammar languages | bounded AST transfer/branch termination |
| `constant_condition` | full | ten grammar languages | literal AST predicates with loop sentinels |
| `cpp_raw_ptr_ownership` | full | C++ | exact lexical raw allocation/ownership contract |
| `kotlin_empty_when` | full | Kotlin | exact lexical empty-`when` contract |
| `exit_in_production` | gating | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | exact `std::process::exit` call nodes outside boundary paths; Go `os.Exit` outside `package main`; Java `System.exit`/`Runtime.exit`/`Runtime.halt` outside `main`; JS/TS `process.exit`/`abort` outside programs (`#!` scripts, private or bin-only packages, `bin` sources; ESLint `n/no-process-exit`); Python `sys.exit`/`os._exit`/`os.abort`, the `exit`/`quit` builtins, and `raise SystemExit` outside program code (`__main__.py`, `#!` scripts, `if __name__ == "__main__":` blocks, `pyproject.toml` script entry functions); Rust paths resolve through `use` (`process::exit`), and packages without a library target are programs |
| `unhandled_termination_in_production` | full | Rust | `panic!`/`unreachable!` macro nodes outside tests |
| `sql_injection` | gating | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C# | SQL sink call with AST string construction; Python DB-API `execute*`, Django `raw`, and SQLAlchemy `text` whose query argument is an f-string or `+`/`%`/`.format` building (Bandit B608); Rust sqlx/Diesel/rusqlite/postgres query methods given `format!` or `+` (checked `query!` macros are not calls); JS/TS `query`/`execute`/`raw`/Prisma unsafe raw calls and Java JDBC/JPA/Hibernate/`JdbcTemplate` calls whose query is a template with substitutions, a `String.format`, or a concatenation with a non-literal, non-`UPPER_SNAKE` value. A query passed by name is read through the values bound to it in the same function, including parts appended with `+=` (`query_built_at` evidence). Not reported when every runtime value shapes the query's structure rather than supplying a value: a table, schema, or column name (quoted as an identifier, joined to a name, or after `FROM`/`JOIN`/`INTO`/`INSERT`/`UPDATE`/`TABLE`/`TRUNCATE`/`EXISTS`) or a clause or list fragment (after `WHERE`/`AND`, inside `(..)` other than `IN (..)`), not taken from request input in the same function; values inside `'..'`, after a comparison, or after `LIKE`/`IN`/`LIMIT`/`OFFSET`/`BY` are values. A local every reaching value of which is a literal, a number converted to text, and a quoted identifier are constants |
| `shell_execution` | gating | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | `os.system` or subprocess call with `shell=True`; Go `exec.Command` of a shell with `-c`; Java `ProcessBuilder`/`Runtime.exec` of a shell with `-c`; JS/TS `child_process.exec`/`execSync`, and `spawn`/`execFile` with `shell: true` or a shell with `-c`; Rust `std`/`tokio`/`async-process` `Command::new` of a shell given its command flag through chained or later `.arg`/`.args` calls |
| `insecure_cryptography` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | weak crypto package call nodes; Java `MessageDigest`/`Cipher`/`KeyGenerator`/`Mac`/`Signature` with MD5, SHA-1, DES, RC4, Blowfish, or ECB, and Commons Codec MD5/SHA-1 helpers; JS/TS `crypto` hashes, HMACs, and ciphers with MD5, SHA-1, DES, RC4, Blowfish, or ECB, and the deprecated `createCipher`; Python `hashlib` MD5/SHA-1 unless `usedforsecurity=False`, PyCryptodome weak `Hash`/`Cipher` modules, `cryptography` MD5/SHA-1/ARC4/TripleDES/Blowfish/ECB (Bandit B303–B305, B324); Rust `md5`/`md4`/`sha1`/`des`/`rc4`/`blowfish`/`ecb` crates and OpenSSL `MessageDigest`/`Cipher` constructors for them |
| `console_output_in_production` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | console call nodes outside tooling scripts; Go `fmt.Print*`/`print` outside `package main`; Java `System.out`/`System.err` `print*` in files without `main`; TypeScript too; JS/TS programs and build configs (`*.config.*`) are excluded; Python `print` (unless it writes to a file), `sys.stdout`/`sys.stderr` `write`, and `pprint` outside program code; Rust `println!`/`print!`/`eprintln!`/`eprint!` outside programs (`main.rs`, `build.rs`, `src/bin`, packages without a library target) and inline tests |
| `debugger_in_production` | gating | Rust, Python, JavaScript, TypeScript, C#, C, C++ | debugger statement nodes; Python `breakpoint()`, `pdb`/`ipdb`/`pudb`/`debugpy`/`rdb` entry points (ruff `T100`); Rust `dbg!` (clippy `dbg_macro`) |
| `empty_catch_block` | gating | Python, JavaScript, TypeScript, Java, Kotlin, C#, C++ | catch clauses with empty AST bodies; Python `except Exception`/`BaseException` handlers that only `pass`, without a comment (ruff `S110` default; narrow handlers are the ask-forgiveness idiom, bare `except:` belongs to `try_except_pass`); Java empty `catch` bodies unless commented or the exception is named `ignored`/`expected` (PMD `EmptyCatchBlock`) |
| `no_await_in_loop` | full | Rust, Python, JavaScript, TypeScript, C# | `await` in the body of a collection `for` loop in the same function (ESLint `no-await-in-loop`), except stream loops (`for await`, `async for`), `while`/`loop` event and polling loops, loops that `break`, `return`, or sleep (`sleep`, `setTimeout`), and loops that await each element itself or a promise it already holds |
| `require_atomic_updates` | full | JavaScript, TypeScript | exact lexical compound-update/await contract |
| `dead_code` | full | Rust | private AST definitions plus semantic/attribute references |
| `go_log_fatal` | gating | Go | exact lexical `log.Fatal` contract |
| `go_errcheck` | gating | Go | AST call statements, `defer`, and `go` that discard a known error result: resolved standard-library functions (`os.Remove`/`Rename`/`WriteFile`/`Mkdir*`/`Chmod`/`Setenv`/.., `io.Copy*`/`WriteString`/`ReadFull`, `filepath.Walk*`, `json.Unmarshal`, `http.ListenAndServe*`), `Run`/`Start`/`Wait` on an `exec.Command` value (chained or through a local, via the data-flow layer), and `Close()`/`Write(..)`/`Remove(..)` methods; assigned, returned, or tested results are used |
| `discarded_fallible_result` | full | Rust, Python, JavaScript, TypeScript, Go, Java | AST discard sites: Rust `let _ =`, `_ =`, and `.ok();` on `write_all`/`flush`/`sync_all`/`sync_data`/`commit`/`persist` and `fs::write`/`rename`/`copy`; Go blank assignments of `Sync`/`Commit` and `os.WriteFile`/`os.Rename` (bare calls are `go_errcheck`'s); Java statements dropping the boolean of `File` `mkdir(s)`/`renameTo`/`createNewFile`/`set*` and of `delete()` on a `File`; JS/TS non-top-level statements calling a same-file `async` function or `this` method, or `.then(onFulfilled)` alone; Python statements calling a same-file `async def` (or `self`/`cls` method) or `asyncio.sleep`, and dropped `asyncio.create_task`/`ensure_future` results |
| `float_equality_comparison` | full | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | `==`/`!=` (`===`/`!==`) between a float literal inexact in binary (`0.1`, not `0.5`; Kotlin and C# reals and C/C++ number literals with their `f`/`L` suffixes, not a C# `m` `decimal`) and float arithmetic: an arithmetic expression, or a local whose reaching value (through the data-flow layer) is one or a `+=` accumulation; production code, outside Rust `#[cfg(test)]` modules |
| `mutable_default_argument` | full | Python | a parameter default that is a mutable literal, comprehension, or constructor (`[]`, `{}`, `set()`, `defaultdict(..)`), reached through the data-flow layer by a mutating method call, an item store or delete, an in-place `+=`/`\|=`, a store into an attribute or item, or a `return`/`yield` |
| `unused_assignment` | gating | Rust, Python, JavaScript, TypeScript, Go, Java, Kotlin, C#, C, C++ | statement-list local def-use over AST assignments; Python blocks and Rust `let`/assignment statements, with Rust format-string captures (`{name}`) counted as reads and attributed (`#[cfg]`) statements skipped; JS/TS and Java declarations and plain `=` assignments; only function locals are judged (a field, package variable, or captured variable may be read by a call) |
| `kt_non_null_assertion` | gating | Kotlin | exact lexical non-null assertion contract |
| `unsafe_type_cast` | full | Kotlin | exact lexical unsafe cast contract |

## Conventions

- `assert_in_production`: Python strips `assert` under `python -O`, so an
  assert that validates input or state silently stops checking; Bandit B101
  and ruff `S101` flag every assert for that reason. EA applies Google's
  Python style guide test, "the assert could be removed without breaking the
  code": an assert that only narrows a type for the type checker
  (`assert isinstance(x, T)`, `assert x is not None`, or several joined by
  `and`) passes it and is exempt. Rust is not claimed: `assert!` always runs,
  and `debug_assert!` is designed to be compiled out. Known residual: a
  bare-name narrowing assert (`assert lexer  # for mypy`) looks like a
  precondition (`assert values`) without type information and is still
  reported.
- `go_errcheck`: errcheck's contract without type information. Only a call
  statement, `defer`, or `go` discards a result; one that is passed on,
  assigned (`_ =` included, as errcheck's default), returned, or tested is
  used. A call counts only when its error is known from the API table, an
  `exec.Command` origin, or the method name; errcheck's default exclusions
  apply to `Write` into a local `bytes.Buffer`, `strings.Builder`, or hash
  (`sha256.New()`, `hmac.New(..)`), and a method the file declares only
  without results (chi's `LogEntry.Write`) is not reported. Directories with a
  leading `_`, such as `_examples/`, take the role of their name (Go's
  toolchain skips them). Known residuals: `fmt.Fprint*` (errcheck excludes it
  only for some writers, which needs the writer's type), and methods on values
  whose type is unknown other than `Close`, `Write`, and `Remove`.
- `discarded_fallible_result`: the result must be the only failure signal
  and the discard explicit, so it extends `go_errcheck` (which accepts
  `_ =`) and the Rust compiler's `unused_must_use` (which accepts `let _`)
  rather than overlapping them. Not reported: a `Drop` implementation (it
  cannot return an error), test code, writes into an in-memory buffer the
  function binds (`Vec`, `String`, `BytesMut::writer`, `Cursor`), console
  and logger flushes or syncs, a discard whose line or the line above
  carries a justifying comment (`ignore`, `best effort`, `intentional`,
  `fire and forget`), an `async` function whose body is one `try` with a
  `catch`, `Promise.resolve().then(..)` microtasks, a script's top-level
  call, a `void`-marked promise, and a Python `async def` behind a decorator
  that may wrap it.

- `float_equality_comparison`: Clippy's `float_cmp` judges operands by
  their type; without types, EA reports only
  what is certain from syntax and data flow: the literal is inexact in
  binary (decided exactly, by whether its denominator is a power of two
  once the factors of 5 cancel, within the 53-bit or `f32` 24-bit
  significand), and the other side is computed by arithmetic, which rounds
  at each step. A value parsed or assigned from the same literal text
  (`float(config["ratio"]) == 0.1`, a parameter) compares equal by
  construction and is not reported, nor are literals exact in binary
  (`0.0`, `0.5`), deliberate sentinels.
- `mutable_default_argument`: Python evaluates a default once, when the
  `def` runs, so a mutable default is one object shared by every call that
  omits the argument. Ruff `B006` and Pylint `W0102` flag every such
  default; EA reports one only when it leaves the call, through a mutation,
  a store, or a return, since a default that is only read cannot carry one
  call's changes into the next. A parameter rebound before the use
  (`items = items or []`, `rows = list(rows)`) is not reported, nor is a
  private or cache-named parameter (`_cache={}`, `memo={}`), the memoizing
  idiom that shares the default on purpose. Every other parsed language
  evaluates defaults per call, allows only constants, or has none, so the
  cell is not applicable there.

## Bounded Semantic Behavior

`unreachable_code` emits once for the first executable statement in each
contiguous segment after an unconditional `return`, `throw`, `break`, or
`continue`. It also recognizes an `if` whose consequence and alternative both
terminate. Comment trivia, Rust nested function items, and JavaScript hoisted
declarations are not executable statements. It does not claim a general CFG.

`constant_condition` unwraps grammar-specific condition nodes and recognizes
literal boolean, null-like, and numeric predicates. Intentional `while true`,
`while True`, and `while (1)` loops are allowed. It does not perform general
constant folding.

`dead_code` indexes private Rust functions and type definitions, then counts
semantic identifier references across the context view. Rust test definitions,
trait declarations and implementations, public symbols, and names referenced
through attributes such as Serde defaults are silent. Reflection and unresolved
external entry points are not claimed.

`unused_assignment` walks Go statement lists and reports a simple local value
overwritten before an intervening read. Compound and multi-target assignments,
aliases, and control-flow joins fail closed. `resource_not_closed` similarly
matches a simple Python `open` assignment and a later `.close()` in the same
function/module owner; general alias and CFG analysis are not claimed.

The Go forms match resolved API identity: each callee and composite-literal
type is resolved through the file's import bindings (`x "os/exec"` makes
`x.Command` `os/exec.Command`; a value named like a package is not the
package), then read against a per-rule API table with literal arguments and
keyed fields. Go `resource_not_closed` treats any use of the handle other
than a non-`Close` method call (returning it, storing it, passing it on) as
an ownership transfer. Go `panic` is not claimed by
`unhandled_termination_in_production`: on the corpus every one guarded an
API contract (`chi: all middlewares must be defined before routes`), the
convention that already demoted the Rust form.

The Java forms match the same way through `java_api_identity.rs`: type names
resolve through single-type, static, and wildcard imports and `java.lang`
(a class the file declares shadows implicit bindings), and receivers resolve
through declared variable types (`var` through its initializer) and JDK
return types (`Runtime.getRuntime().exec` is `java.lang.Runtime.exec`).
Semantics follow PMD `DoNotTerminateVM`, `SystemPrintln`, and `CloseResource`
where they exist. A bare `UnsupportedOperationException` is the JDK's
optional-operation contract and is not a TODO. A transformer that serializes
an in-memory DOM reads no external entities and is not an XXE site.

The JavaScript and TypeScript forms resolve through `js_api_identity.rs`:
ES imports (default, namespace, named with aliases) and CommonJS `require`
(whole module, member, or destructured member), with the `node:` scheme
dropped; `process`, `Math`, `eval`, and `Function` resolve as globals only
when the file declares no variable or parameter of that name. A JS/TS program
owns its process and output: a `#!` script, a file in a `"private": true`
package or in a package with a `bin` and no root entry, or the source file of
a `bin` entry. The JavaScript `console`, `debugger`, empty-catch, and
empty-branch rules run unchanged on the TypeScript grammar.

## Tier-2 Forms

Kotlin, C#, C, and C++ resolve API identity per file, then read the same
per-rule tables as the Tier-1 forms:

- **Kotlin** (`kotlin_api_identity.rs`): imports, aliases, and wildcards
  over the Kotlin standard library and the JDK tables the Java forms use, with
  file-declared names shadowing them. Receiver types come from declared or
  initialized `val`/`var` bindings. A file that declares `fun main` owns its
  process and output. `use { }` closes a resource. A Ktor `embeddedServer`
  host is a bind site.
- **C#** (`csharp_api_identity.rs`): `using` directives, aliases, `using
  static`, and the SDK's implicit global usings. Receivers resolve through
  declared locals and member types. A file declaring `Main` or top-level
  statements is a program. A `[Conditional]` method's argument asserts are
  compiled out by design and not reported. `ServerCertificateCustomValidationCallback`
  lambdas that return `true`, `TypeNameHandling` other than `None`, and
  `DtdProcessing.Parse` are the security sinks.
- **C and C++** (`c_api_rules.rs`): callees resolve by name with `std::`
  stripped. A function or macro the file defines shadows the library name,
  and `#if 0` regions are compiled out. `fopen` handles that are neither
  closed nor handed on are unclosed (stdio streams are not transfers). C++
  releases resources by RAII, so `resource_not_closed` does not claim it.
  `DebugBreak` and `__builtin_trap` are debugger traps.
- **Dead stores** (`unused_assignment`): only locals are judged. The checks
  skip `volatile`, address-taken, and reference-bound C/C++ locals; C#
  `out`/`ref` arguments; and constant initializers. A `goto` or label clears
  pending stores.
- **SQL** (`tier_two_sql.rs`): JDBC-style Kotlin sinks, plus ADO.NET
  (`SqlCommand`, `CommandText`), Entity Framework raw SQL, and Dapper sinks
  given an interpolated, concatenated, or formatted query. EF Core's
  `*Interpolated` methods parameterize their holes and are not sinks.

Source roles: .NET `*.Tests` projects, `testing-support`/`test-utils`
modules, Kotlin `src/<name>Test` source sets, and `*Test`/`*Tests` class
files are test code. C and C++ files carrying a Bison, yacc, or flex banner
are generated. A `.h` header is C++ when its content uses C++ syntax.

## Donor Evidence

Selected atomic behavior is checked live against Bandit, Clippy,
GolangCI-Lint, and Detekt. Donor agreement is capability-specific, not a claim
of whole-tool parity. Exact mappings and documented semantic differences live
in [`donor-parity-spec.md`](./donor-parity-spec.md).
