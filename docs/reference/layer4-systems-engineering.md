# Layer 4 - Systems Engineering

Default catalog: 13 detectors (6 gating, 7 full-only). The `graph-analysis`
feature adds four Layer 4 gating detectors. Language modes are authoritative in
[`language-support.md`](./language-support.md).

## Shared Systems Index

Default Layer 4 source owners consume one lazy, scan-scoped `SystemsIndex`
derived from the resolution-aware structure index. It retains compact typed
facts, spans, and provenance, not source text or syntax trees.

Effects classify file I/O, network I/O, external reads, parse, serialization,
cache mutation/access, initialization, state construction, repository scans,
async functions, and hot/request paths. Collection and value lineage connects
reads, transforms, and writes without keyword windows. A Go package-qualified
call is classified by its resolved API identity alone (`os.ReadFile`,
`json.Unmarshal`, `regexp.MustCompile`, `os.ReadDir`, `http.Get`), so
`url.Parse` is not a parse effect; `Unmarshal` decodes into its pointer
argument, which is the parsed value. Java calls are classified by resolved
identity first (`Files.readString`/`writeString`, Jackson `ObjectMapper` and
Gson read/write, `Pattern.compile`, `Files.list`/`walk`, `HttpClient.send`)
and fall back to the name rules otherwise, so `cache.add` remains a cache
insert. Java caches read their latest entry through `getLast`, `peekLast`, or
`get(cache.size() - 1)` and are bounded by `remove`/`removeFirst`/`poll`/
`clear`; Go grows one with `cache = append(cache, e)`, reads
`cache[len(cache)-1]`, and bounds it by reslicing it into itself. A lambda
passed to `CompletableFuture.supplyAsync`/`runAsync` without an executor runs
on the shared fork-join pool, so blocking I/O in it is in an async context. So is a lambda passed to an operator of a Reactor (`Mono`, `Flux`) or
RxJava chain, which runs on the emitting thread (an event loop, BlockHound's
concern), when the chain's root resolves to a library type (a declared
`Mono<T>` or a `Mono.just(..)` factory) and no `subscribeOn`, `publishOn`, or
`observeOn` appears anywhere along it. Static factories such as
`Mono.fromCallable` and chains rooted in calls the resolver cannot type are
not claimed, since the caller may schedule them.

Local topology resolves imports to files and imported symbols, records
re-exports and inheritance edges, groups packages, and canonicalizes strongly
connected components deterministically. A Go import under the module path of
the file's nearest `go.mod` resolves to its package directory, and a Java
import resolves to the file its package path names under any source root
(`com.acme.Model` to `.../com/acme/Model.java`; a static import to its class). A supplied authoritative graph remains
authoritative. Unresolved local imports or symbols reduce completeness and fail
closed for findings that require that edge.

## Gating Owners

### `architectural_layering`

Applies explicit `auditor.toml` layer collectors and deny rules to resolved
local import, inheritance, and re-export edges. It can suppress accepted edges
through a baseline. No layer is inferred from path vocabulary. A Java type
import named nowhere in code but its own declaration, such as one kept for a
javadoc `{@link}`, is no edge.

### `package_coupling_metrics`

Computes package afferent/efferent coupling, instability, abstractness, and
distance from the main sequence over resolved local edges and classifiable
types. Incomplete local topology and packages without classifiable types fail
closed for the affected metric, and so does a package with fewer than two
coupling edges: with one edge, instability is 0 or 1 by construction and the
distance only restates abstractness.

### `loop_artifact_rewrite`

Reports a loop that performs a whole-artifact read followed by a write of the
derived value. Both effects must share the function, loop region, and value
lineage, and the output artifact must be loop invariant. Per-item copies whose
destination depends on the loop binding fail closed.

### `latest_only_unbounded_cache`

Reports cache/history growth whose only observed consumption is the latest
value and whose lifecycle has no capacity, eviction, truncation, retention, or
replacement bound.

### `repeated_parse_scan_serialize`

Reports parse, scan/query, and serialization effects chained through the same
value lineage on a repeated or hot/request path. Unrelated calls in one function
do not satisfy the contract.

### `churn_complexity_hotspot`

Combines Git positive line growth with shared AST complexity facts. Eligible
repositories require at least eight files; candidates must be in the upper
quartile for both dimensions, have grown by at least 50 lines, and have at
least two commits. The floor keeps an active window, where the growth
quartile can sit at a few lines or none, from ranking small edits as
hotspots. Churn covers the
commits reachable from `HEAD` within 14 days of `HEAD`'s commit time, so one
commit scans identically on any day and in any clone. Growth is net of moves:
in each commit, the lines a file gains are offset by lines that commit removed
from other files with its extension (largest growth first), so splitting a
file adds none, and a file's initial size is not growth: only commits after
the one that creates it grow it. Because the window and the quartile cutoffs move with every
new commit, a file can enter or leave the hotspot set without being edited.

## Full-Only Owners

## Effects Through Calls

`per_item_external_io` and `blocking_io_in_async_context` also follow calls
into repository functions. A call inherits the external-I/O and blocking-I/O
effects of the function it invokes, through up to four resolved calls, so a
loop calling a helper that makes a network request is reported at the call
site, with `via` evidence naming the helper chain and Medium confidence.
Calls resolve only when the target is certain without types
(`call_graph.rs`): same-file functions, imported functions, Go functions in
the same package, `self`/`this` and implicit-`this` methods, and
`Type.method` or `module.function` calls. A call on any other value stays
unresolved.

`loop_artifact_rewrite` follows calls the same way. A helper's artifact read
or write carries its artifact and value relative to the helper's parameters,
and each call binds them to its arguments by position. A loop that reads an
artifact and passes the value to a helper writing a fixed artifact is
reported, and so is a loop calling a helper that reads and rewrites one
artifact itself. A write whose path depends on an argument that varies with
the loop is per-item output, not a rewrite. A named argument leaves the
binding unknown, so no finding follows from it. The other systems owners
still consume direct effects only.

### `blocking_io_in_async_context`

Reports a proven blocking file or process I/O effect directly inside an async
function. Async-native APIs and work delegated through a blocking boundary are
semantic negatives.
Tooling files (a `scripts/`, `tools/`, or `tooling/` directory at any depth,
such as a docs site's `www/scripts/`) are excluded: a one-shot script runs nothing concurrently, so blocking there
stalls nothing.

### `per_item_external_io`

Reports external network or repository I/O whose arguments depend on the active
loop binding. Batched calls, writes, invariant calls, and local computation are
excluded. Database/repository read queries owned by
`n_plus_one_query_in_loop` remain disjoint.
A loop that already walks groups of items is excluded too: its binding's
last word is `batch`, `chunk`, `page`, `window`, `partition`, or `shard`
(`purlBatch`, `page`), or its iterable calls a batching function (`chunks`,
`chunked`, `batched`, `partition`, `windows`, `paginate`, ..). One operation
per group is the batched form.

### `repeated_expensive_initialization`

Reports construction of a proven expensive client, parser, regex, runtime, or
repository state inside a repeated control region. Loop-invariant reused
initialization is excluded, and so is a constructor whose arguments come
from the loop variable or a counter assigned in the loop's header
(`new RegExp(pattern)` per pattern key, or per `args[i]`): each iteration
builds different state, so nothing could be hoisted.

### `network_call_without_timeout`

Reports an HTTP call or client whose library default never times out, so a
stalled peer blocks the caller indefinitely (CWE-400). Production files only,
and not inline Rust `#[cfg(test)]` modules.

Algorithm: calls resolved through each language's API identity are matched
against a per-language client table; only clients with an unbounded default
are listed:

- Python (Bandit B113, the donor oracle): `requests` calls without
  `timeout` or with `timeout=None`, `httpx` calls and clients with
  `timeout=None` (httpx defaults to 5 s), and `urllib.request.urlopen`
  without a timeout. A `**kwargs` splat may carry the timeout and is skipped.
- Go: `http.Get`/`Post`/`PostForm`/`Head` and `http.DefaultClient` calls,
  which use the timeout-free default client, and an `http.Client{}` literal
  without `Timeout` or a custom `Transport`.
- Java: `java.net.http.HttpClient.newHttpClient()` and `newBuilder()`
  clients (no connect timeout by default). OkHttp and Apache HttpClient have
  bounded defaults.
- JavaScript/TypeScript: axios calls and `axios.create` whose config, by
  position, sets no `timeout` (skipped when any file sets
  `axios.defaults.timeout`, or the config is not an object literal; a bare
  `axios(x)` takes `x` as the config unless it is a URL string), and
  Node `http`/`https` `get`/`request` without a `timeout` option or a
  `setTimeout` in the enclosing function. `fetch` is not listed: Node's
  undici bounds headers and bodies at 300 s.
- Rust: `reqwest::get` and `reqwest::Client::new`/`default`/`builder`.
  The `reqwest::blocking` client defaults to 30 s.

A client construction is skipped in a file that mentions a timeout anywhere,
where it is set on the client or its requests.

### `retry_without_backoff`

Reports a loop that retries a failed operation at once, with no wait between
attempts. Immediate retries add load to a peer that is already failing and
fail again for the same reason; a bounded loop spends its attempts in
microseconds, before a transient fault can clear. Production files only, and
not inline Rust `#[cfg(test)]` modules. Rust, Python, JavaScript,
TypeScript, Go, and Java.

- Retry loop: a counted loop (`range(n)`, a C-style `for` with a counter,
  a Rust `0..n` range) whose operation does not read the counter or a local
  assigned from it (`url = urls[i]` tries a different target each pass), or
  a `while`/`loop`/`do` whose condition calls nothing and which names its
  attempts (an identifier with the word `attempt`, `retry`, `retries`, or
  `tries`): an open loop without one reads, polls, or iterates.
- Retry: a `try` whose success leaves the loop (`return`/`break` in the
  `try` or its `else`, or right after it) and whose handler does not end in
  `raise`/`throw`/`return`/`break`; a Go `if err == nil { break }` after the
  call assigning `err`, or `if err != nil { continue }` before the exit; a
  Rust `match` on a call whose `Ok` arm leaves the loop and whose `Err` arm
  falls through, or `if let Ok(..) = call { break }`.
- Backoff: any call in the loop whose name has the word `sleep`, `wait`,
  `delay`, `backoff`, `pause`, `throttle`, `timeout`, `park`, `jitter`, or
  `select` (`time.sleep`, `Thread.sleep`, `setTimeout`, `select.select`), Go
  `time.After` or a `select` statement, or a call to a function of the same
  file that waits, directly or through another.

The corpus has no such loop in production code: its retries go through
retry policies or wait between attempts, so the cells rest on their
capability boundaries.

### `unused_declared_dependency`

Reports a production dependency that a package manifest declares and no
source file under the manifest's directory imports: every unused entry costs
each build a fetch, a compile or install, and a supply-chain audit.

- Manifests: `Cargo.toml` `[dependencies]` and target tables (the key is the
  import name, so renamed packages count), `package.json` `dependencies` of
  a published package, `go.mod` direct `require`s (`// indirect` excluded),
  `pyproject.toml` `[project] dependencies` and
  `[tool.poetry.dependencies]`, and `pom.xml` compile-scope `jar`
  dependencies outside `dependencyManagement`, plugins, and profiles.
  Cargo dev and build tables, npm dev, peer, and optional dependencies,
  Python optional-dependency groups, and Maven test, provided, and runtime
  scopes are not read. A manifest is read when it sits above a production source
  file of its ecosystem; Cargo virtual workspaces, Maven aggregators
  (`<packaging>pom</packaging>`), and `"private": true` npm packages
  (workspace roots and sites, where frameworks load packages by convention)
  declare nothing.
- Uses: any source file under the manifest's directory, tests, examples,
  and nested packages included. Rust `name::` paths, `use name`, `use` groups,
  and `extern crate`; npm quoted specifiers (`'name'`, `"name/sub"`) in
  JS/TS and JSON/YAML configuration, and names in the manifest's scripts and
  tool configuration (with the `typescript` -> `tsc` style binary names);
  Go imports of the module path or a package under it, and `tool`
  directives; Python `import`/`from` of the distribution's normalized name,
  a known module mapping (`PyYAML` -> `yaml`, `Pillow` -> `PIL`), or the
  name without common affixes, or the module named as a string; Maven
  packages under the group's first two segments, or an import of the
  artifact's root (`lombok.`).
- Not reported: `-sys` link crates and names a `cargo-machete` or
  `cargo-udeps` ignore list holds; npm type packages, compiler helpers
  (`tslib`, `@babel/runtime`), tool plugins and presets by naming convention,
  and React when the package has JSX/TSX files; Python servers and database
  drivers loaded by command line or URL; Maven starters, BOMs, logging
  bindings, JDBC drivers, and runtime implementations.

The finding sits on the manifest line and names the package's first
production source file as a second location, which carries its language.

### `request_path_repository_scan`

Reports a whole-repository discovery or graph scan on a typed request/hot path.
Lookup in already-built state and startup-time scans are excluded.

## Graph Feature Owners

`repo_graph_isolated_source_file`,
`repo_graph_high_fanout_source_file`, `repo_graph_import_cycle`, and
`shotgun_surgery_cluster` consume authoritative repository/co-edit inputs.
Omitted inputs are valid and emit nothing; malformed specified inputs fail the
scan. The graph feature also adds the Layer 2 split/consolidation candidates.
