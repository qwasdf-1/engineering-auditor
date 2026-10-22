# Layer 2 — Structural Engineering

Profile: 22 gating (all scans) + 15 full-only (full scans only)
Detectors: 37 default plus 3 gated behind `graph-analysis`.
Full per-language coverage matrix in
[`language-support.md`](./language-support.md).

Structural detectors inspect file structure, semantic declarations, naming,
duplication, and cross-module patterns. Syntax-sensitive owners share
scan-owned AST, complexity, and structure indexes.

> Architectural compliance is enforced by `tooling/detector_gate_lint.py`.

---

## `large_file` (gating)

Flags files that exceed line-count thresholds. Thresholds are configurable via
`auditor.toml` (`[detectors.large_file] threshold`); defaults are advisory (500 lines) and strict (1000 lines).

Algorithm: counts nonblank lines. Applies per-file with category-based tiering
(source, generated, documentation). Config thresholds are looked up per
detector ID.

Language contract: see [`language-support.md`](./language-support.md).

---

## `generated_vs_source` (gating)

Detects transient or generated artifacts in the audited tree: merge conflict
files, Python `__pycache__` directories, and diagnostic evidence stored under
`docs/`.

Algorithm: scans file paths for known transient patterns during source
discovery. Fires on artifact discovery rather than per-line analysis.

Language contract: see [`language-support.md`](./language-support.md).

---

## `stale_path_reference` (gating)

Backtick-delimited path references in Markdown files that do not resolve to
existing files. Detects stale documentation references after file moves or
deletions.

Algorithm: extracts backtick-delimited text from `.md` files, normalizes
(removes qualified refs like `File::method`, strips line numbers from
`path:line` patterns), and checks existence against the filesystem. Only
checks paths under resolvable prefixes (`docs/`, `tooling/`).

Language contract: see [`language-support.md`](./language-support.md).

---

## `stale_link_reference` (gating)

Markdown link targets that do not resolve to existing files.

Algorithm: scans each line for markdown link targets, filters out external
URLs, absolute paths, and anchors-only refs, then resolves against the
repository root.

Language contract: see [`language-support.md`](./language-support.md).

---

## `stale_section_anchor` (gating)

Markdown section-anchor link targets where the section anchor does not
exist in the target file.

Algorithm: collects all `##`-level header slugs from each `.md` file (GitHub
slugification: lowercase, replace spaces with hyphens, strip non-alphanumeric).
For each `section-anchor` link target, checks whether the anchor exists in the target's
section set.

Language contract: see [`language-support.md`](./language-support.md).

---

## `function_complexity` (gating)

Counts grammar-backed branch decisions per function using the shared
`ComplexityIndex`. Fires when branch count exceeds the configured budget.

Algorithm: one AST traversal counts conditional, loop, catch, ternary, and
switch/match arm nodes while excluding nested named functions and closures.
The same fact pass supplies cognitive, NPath, Halstead, and churn inputs.

Language contract: see [`language-support.md`](./language-support.md).

---

## `too_many_parameters` (gating)

Flags functions with parameter lists exceeding threshold (default 6 effective
params).

Algorithm: counts typed parameter symbols in the shared structure index and
excludes receiver parameters such as `self`, `cls`, `this`, and a C#
extension method's `this T value`. Python
`*args` and `**kwargs`, typed or not, do not count, as in pylint and ruff
`R0913`. A Python `@overload` stub restates its implementation's signature and is skipped; the
implementation is measured once.

Language contract: see [`language-support.md`](./language-support.md).

---

## `section_divider_comment` (gating)

Flags functions that use decorative comment dividers (`// ----`, `# ====`,
`/* ****`) excessively (default threshold: 3 divider lines per function).

Algorithm: strips comments and counts lines matching divider patterns (>=3
repeated `-`, `=`, `*`, `_` characters in a comment line). A border framed by
`+` or `|` (`+---+---+`) draws a table or byte-layout diagram and is not a
divider.

Language contract: see [`language-support.md`](./language-support.md).

---

## `magic_literal_cluster` (full-only)

Flags functions with a high count of distinct non-trivial numeric literals
(default threshold: 3 distinct literals).

Algorithm: extracts numeric literals from each function body using
`util::numeric_literals()`. Filters out trivial values (0, 1, -1, 100, etc.)
via `is_trivial_numeric_literal()`. Counts distinct literals per function.
Block strings are masked first (Python triple quotes, Rust raw strings, Java
and Kotlin text blocks, JavaScript/TypeScript/Go backticks), so prose such as
"4xx or 5xx" is not counted.

Conventions: a number that carries its own name or is data is not magic.
JavaScript and TypeScript object values and JSX attribute values are named by
their keys (ESLint `no-magic-numbers` defaults to `detectObjects: false`), and
regex literals are patterns. A literal-only list of three or more numbers is a
lookup table. `hashCode`/`__hash__` mixing constants are the algorithm
(Checkstyle `ignoreHashCodeMethod`), and files under `locales/`, `i18n/`, or
`l10n/` encode each locale's plural rules.

Tier-2 languages are deferred: on the corpus sample Kotlin scored 0 of 12 and
C# 5 of 12, and a C/C++ probe looked the same. Their numeric literals are mostly
byte layouts and encoding or hash specification constants (UTF-8, Base64, SHA,
IEEE-754), which naming would not clarify.

Language contract: see [`language-support.md`](./language-support.md).



## `identifier_verbosity` (gating)

Flags functions where identifiers average more than 3 words per name.

Algorithm: collects all identifiers in a function via `identifier_occurrences()`.
Splits each identifier on camelCase/snake_case boundaries. Computes mean words
per identifier. Fires when mean > 3.0 and at least 5 identifiers exist.

Language contract: see [`language-support.md`](./language-support.md).

---

## `identifier_tersity` (full-only)

Flags functions where more than 50% of identifiers are 1-2 characters (excluding
common loop variables `i`, `j`, `k`, `x`, `y`, `z`, `ok`, `n`).

Algorithm: collects identifiers per function, counts those with length <= 2
(not in allowlist). Fires when terse density > 0.50 and at least 5 identifiers
exist.

Language contract: see [`language-support.md`](./language-support.md).

---

## `stringly_typed_parameter` (gating)

Flags functions with parameters named like sentinel values (`mode`, `kind`,
`type`, `status`, `state`, `action`, `variant`) typed as a string (`str`,
`&str`, `String`, TS `string`, JSDoc `{string}`) and used as real sentinel
selectors, in Python, JavaScript, TypeScript, and Java.

Algorithm: checks semantic parameter symbols and their bounded declarations
for sentinel names and string types, then requires the parameter reference and
at least two distinct string literals to occur in the same conditional or match
control regions. Passive boundary-conversion parameters remain silent.

Language contract: see [`language-support.md`](./language-support.md).

---

## `function_local_import` (gating)

Flags functions containing local import statements that could be hoisted to
module scope.

Algorithm: groups semantic import facts by owning function. Dynamic imports
stay explicitly unresolved.

Conventions: PEP 8 and Google's Python style guide put imports at the top of
the file, and pylint/ruff `PLC0415` flags every exception. EA follows them and
exempts only the deferrals those communities accept when the reason is
visible in the code:

- an optional dependency, inside a `try` whose handler catches `ImportError`
  or `ModuleNotFoundError`;
- a typing-only import inside `if TYPE_CHECKING:`;
- an import with a comment on its line or the three lines above stating the
  reason (`lazy`, `lazily`, `deferred`, `circular`, `cycle`, `optional
  dependency`).

An undocumented deferral of a standard-library or package-internal module,
even one that avoids an import cycle, is a true positive: the fix is to hoist
it or to say why it cannot be.

Language contract: see [`language-support.md`](./language-support.md).

---

## `sibling_call_redundancy` (full-only)

Detects pairs of functions in the same file with the same set of at least three
non-trivial callees.

Algorithm: builds resolved call sets per semantic function owner, removes
ubiquitous collection and error-handling calls, and groups exact signatures. The
evidence records the shared set and score. Agent-slop's broader overlap-score
rule is covered by a live atomic parity case, but repository-scale candidate
sets may differ.

Language contract: see [`language-support.md`](./language-support.md).

---

## `source_duplicate_detection` (gating)

Reports one canonical finding per normalized clone class.

Algorithm: tree-sitter leaves of function bodies (not signatures, generics,
`where` clauses, or Python docstrings) form function-bounded rolling windows. A window
is extended leftward only into tokens of the same function, so an `export`
keyword, decorator, or attribute before the function no longer hides a clone
that starts at it. Candidate
matches require consistent one-to-one identifier and literal renaming, span at
least 50 tokens and eight lines, and contain at least two repeated nonreceiver
semantic identities. Matches extend to the maximal shared suffix and are
greedily overlap-suppressed. Homogeneous unique-call and data lists remain
silent. Rust JSON macro mappings and struct field-initializer lists are treated
as declarative projection rather than behavioral clones. Evidence records every
exact line/column occurrence without expanding all pairs.

Conventions: a clone is a true positive when the repeated code can reasonably
be extracted into shared code. Code that states an interface rather than
behavior is not tokenized: function parameter lists (explicit keyword APIs
such as httpx's `get`/`head`/`options` repeat them by design), keyword
arguments that forward a parameter of the same name (`params=params`), and
argument lists of literals only (help-table rows). A clone whose async
occurrences are all twins of sync ones (`send` and `async send`, `close` and
`aclose`, `handle_request` and `handle_async_request`, `__enter__` and
`__aenter__`) is not reported: Python and JavaScript cannot share a body across
the two without code generation. Sync code repeated inside such twins (a
synchronous decode-and-flush tail) still counts. A clone whose occurrences
differ in more distinct identifier or literal bindings than the
`too_many_parameters` budget (5) is not reported either: its extraction would
need that many parameters (parallel match arms, attribute-assignment rows).
Names declared inside the clone are locals of the extraction and do not count.
The count is reported as `differing_bindings` evidence.

Not reported either: a clone across two build packages (the nearest
directories with `Cargo.toml`, `package.json`, `go.mod`, `pom.xml`,
`build.gradle`, or a Python manifest), which can be shared only by adding
public API to one of them; a row table, where the occurrence is mostly one row
shape repeated at least three times (also across two abutting occurrences, the
halves of one match); handlers of different states of one Java enum state
machine, which transcribe a specification's repeated steps; and clones among
locale files.

Profile: `full` since v1.0 Task 8. Corpus precision after the v1.0 Task 15
fixes is 92% or better per language for Rust, Python, Go, and TypeScript on
the labelled sample; Java stays deferred (15/24: primitive specializations,
mirror-image algorithms, and deliberately monomorphic copies). Among the
Tier-2 languages, C qualifies (11/12: libuv's per-platform backends copy code
the project already shares elsewhere). Kotlin (6/12), C# (7/12), and C++ (2/9)
are deferred for Java's reasons: per-type overloads, primitive
specializations, mirror pairs, and DSL families (`single`/`factory`/`scoped`).

Language contract: see [`language-support.md`](./language-support.md).

---

## `naming_convention` (gating)

Enforces per-language naming conventions: PascalCase for Java classes/interfaces,
camelCase for Java methods, UPPER_SNAKE for Java/Kotlin constants, PascalCase
for Kotlin classes/objects, camelCase for Kotlin functions, PascalCase for
exported Go functions, camelCase for unexported Go functions, PascalCase JS/TS
classes and no snake_case JS/TS functions (React-style `unstable_`
prefixes, PascalCase components, and `_`/`$` private markers allowed;
interfaces may describe callable hooks and are not judged), RFC 430 for
Rust (UpperCamelCase types and traits, snake_case functions and modules,
SCREAMING_SNAKE_CASE constants and statics; trait-impl items take the trait's
names), and PEP 8 for Python (CapWords classes, lowercase functions and
methods; pep8-naming N801/N802). Python methods of a class with a base may
implement an external protocol (`setUp`, `visit_Name`, `do_GET`) and are not
judged, nor are `ctypes`/`cffi` binding modules, whose names mirror the C API.
Kotlin functions are judged only for underscores (detekt `FunctionNaming`),
and `external` functions keep their native names. A Kotlin file named after
its class is checked only when that class is the file's single top-level
declaration. C# follows the .NET design guidelines: `I`-prefixed PascalCase
interfaces and PascalCase types and methods.

Algorithm: uses semantic declaration symbols for Java, Kotlin, Go, Rust, and
Python type and function names, with bounded lexical supplements for constants and Kotlin
property/enum rules.

Conventions: Google Java Style §5.2.4 defines a constant as a `static final`
field whose contents are deeply immutable, so a mutable collection, array,
builder, atomic, or logger held in a `static final` field keeps
lowerCamelCase (gson's `mapOfMapOfMethods` cache). PMD's
`FieldNamingConventions` flags these; the published style guide takes
precedence. `serialVersionUID` is exempt as a required name.

Language contract: see [`language-support.md`](./language-support.md).

---

## `go_repeated_string_literal` (full-only)

Flags string literals that appear 3+ times in the same file, indicating a
missing named constant.

Algorithm: scans each source file, extracts string-literal content (between
double quotes) per line (after comment stripping). Counts each distinct string.
Fires when count >= 3.

Language contract: see [`language-support.md`](./language-support.md).

---

## `too_many_locals` (full-only)

Flags functions with >15 local variable declarations (let/const/var/val).

Algorithm: counts semantic local-variable symbols owned by the nearest named
function, excluding locals owned by nested named functions; a closure's or
lambda's locals count toward its enclosing function. Python declares no
locals, so its count is the distinct names a function assigns or binds as
loop targets (pylint `R0914` without the parameters).

Language contract: see [`language-support.md`](./language-support.md).

---

## `too_many_returns` (gating)

Flags functions with >5 `return` statements.

Algorithm: counts semantic return facts owned by the nearest named function.

Language contract: see [`language-support.md`](./language-support.md).

---

## `too_many_statements` (gating)

Flags functions with >50 non-blank, non-comment statements.

Algorithm: counts grammar-backed statement facts owned by the nearest named
function.

Language contract: see [`language-support.md`](./language-support.md).

---

## `duplicate_import` (gating)

Reports repeated module-level imports with the same canonical source
(Python, Java, JavaScript, TypeScript, Kotlin, C#, and C/C++ `#include`).

Algorithm: canonical import facts are grouped by source path after alias
parsing; dynamic and function-local imports are excluded.

Conventions: ESLint `no-duplicate-imports` and `import/no-duplicates` cover ES
`import` declarations, which merge into one statement. CommonJS member
requires (`var methods = require('./utils').methods;`) bind one member per
line by design, have no runtime cost (`require` is cached), and no lint
convention asks to merge them, so they are not duplicates. A Python import
under `if TYPE_CHECKING:` binds nothing at runtime, so it is grouped apart from
runtime imports; a runtime re-import in a `__main__` block of a name the
module already imports is a duplicate (Pylint `reimported`). A C/C++
`#include` repeats another only when its preprocessor branch encloses the
other's: the same header under `#ifdef`/`#else` arms is not a duplicate, but a
header re-included inside a branch after an unconditional include is.

Language contract: see [`language-support.md`](./language-support.md).

---

## `out_parameter` (gating)

Reports Java methods that write through an input parameter: a field or element
the caller can observe (`target[i] = x`, `request.boundary = x`).

Algorithm: assignment targets that start with a semantic parameter symbol of
the same method followed by `.` or `[` (and, for C/C++, `*p` or `p->`). One
finding per method and parameter, at the first write.

Conventions: this is Clean Code's "output argument": a caller cannot tell from
the call that an argument is a result channel, and the advice is to "have it
change the state of its owning object" instead. Rebinding the parameter itself
(`value = 2`) is not a write: Java passes every argument by value (JLS
§8.4.1), so the caller never sees it. PMD `AvoidReassigningParameters` and
Checkstyle `ParameterAssignment` cover that as a readability rule. Two
patterns are the owning object at work and are exempt:

- the parameter's type is declared in the same file, so this code owns its
  invariants (a map rotating its own tree nodes, a node detaching a child);
- the method belongs to an enum: a GoF State handler acting on the context it
  is a state of.

Known residual: an owner adopting an object whose class lives in another file
(`attribute.parent = this` in jsoup `Attributes.put`) is still reported.

Language contract: see [`language-support.md`](./language-support.md).

---

## `excessive_nesting_depth` (gating)

Reports functions whose semantic control-region depth exceeds five.

Algorithm: follows parent links between conditional, loop, match, try, and
catch facts and attributes the maximum depth to the nearest named function.
Direct `else if` branches retain the surrounding conditional depth rather than
introducing artificial nesting.

Language contract: see [`language-support.md`](./language-support.md).

---

## `identifier_stutter` (full-only)

Flags public items whose name repeats the module callers qualify them with:
Go `chubby.ChubbyFile`, Rust `config::ConfigError`.

Algorithm: compares the words of a public module-level item (function, type,
trait, constant) with the words of its module name: the Go package, or the
Rust module's file or directory name. It fires when the name starts or ends
with all the module's words and has more; a name equal to its module
(`error::Error`) is idiomatic. Rust items count only when callers outside the
crate can name the module: the item and every module from the crate root down
are declared `pub`, not `pub(crate)`, and `impl` members are excluded.
`overlap_count` is the number of repeated words, which the `.slop.toml`
`min_overlap_tokens` threshold compares.

Conventions: Go Code Review Comments ("all references to names in your
package will be done using the package name, so you can omit that name from
the identifiers") and clippy `module_name_repetitions` define stutter for
qualified public names. No style guide treats a local that repeats its
function's words (`header_value` in `_get_header_value`) as a smell, so
locals are out of scope. Python is not claimed: PEP 8 is silent, the standard
library itself names `json.JSONDecoder` and `http.HTTPStatus`, and a private
Rust module's items are re-exported under another path, so neither stutters at
the call site.

Language contract: see [`language-support.md`](./language-support.md).

---

## `escape_hatch_type_density` (full-only)

Flags Python and TypeScript files where `Any` or `any` exceeds 30% of real
type annotations.

Algorithm: strips strings and comments, counts annotation syntax and
escape-hatch tokens, and fires only when at least five annotations exist.
Rust `dyn` trait objects are not escape hatches, and neither is a TypeScript
`any` inside generic brackets (`Map<any, any>`, a default
`R extends HandlerResponse<any> = any`) or as a constraint
(`T extends any[]`): there it is a variadic generic's placeholder.

Language contract: see [`language-support.md`](./language-support.md).

---

## `collection_parameter_mutation` (full-only)

Flags functions that mutate collection parameters (`.append(`, `.push(`,
`.insert(`, `.extend(`) in place, indicating unintended side effects on
caller state.

Algorithm: resolves call receivers to semantic parameter symbols and matches a
bounded mutator set. Implicit object receivers named `self`, `cls`, or `this`
are not collection parameters; other parameter naming does not affect the
result. In Python a consumed result of a `None`-returning mutator is a copying
API. In Java and TypeScript the parameter must be declared as a collection (a
`StringBuilder` or tree builder is an output sink), the function must not be
private, and a recursive call must not pass the parameter on (a walk
threading a `visited` set). Kotlin requires a mutable collection type
(`MutableList`, `ArrayList`, ...) on a function that is not `private` or
`internal`. C# requires a BCL collection type that is not passed by `ref` or
`out` and is not an extension method's `this` receiver; it uses the .NET
mutator names (`Add`, `Insert`, `RemoveAt`, ...). JavaScript is deferred
because untyped parameters cannot be told from output sinks. Go is deferred
because it mutates slices by index, not by method call.

Language contract: see [`language-support.md`](./language-support.md).

---

## `halstead_information_density` (gating)

Estimates Halstead volume per function. Fires when volume exceeds 1500 unless
repository configuration raises the threshold.

Algorithm: walks only the function body, stops at nested functions and
closures, and classifies tree-sitter leaf tokens with the donor's
language-specific operator and operand tables. Volume is `length *
log2(vocabulary)`. The default capability case exercises the AST path.

Language contract: see [`language-support.md`](./language-support.md).

---

## `class_structure_metrics` (gating)

Reports classes exceeding donor-calibrated WMC, CBO, DIT, or NOC budgets.

Algorithm: combines semantic method control counts, reference/call coupling,
resolved base classes, and child relationships in the shared structure index.
Depth alone does not flag a method-less leaf (an exception taxonomy), and NOC
alone does not flag a class built for extension (abstract, an `ABC` or
`Protocol` base, or a mixin). C++ classes count when they have a body (a
forward declaration names a class without declaring one). Kotlin interfaces
and `expect` classes declare an API surface and are not judged.

Language contract: see [`language-support.md`](./language-support.md).

---

## `god_module` (full-only)

Flags files with >20 top-level function-like definitions.

Algorithm: counts semantic function symbols, not methods, whose parent is
the file or a module. Nested functions do not inflate the count, and neither
do methods however the language attaches them: class members, Rust `impl` and
`trait` functions, Go receivers, C++ qualified definitions
(`int Block::Compare() {}`), and members of a JavaScript class expression.

Language contract: see [`language-support.md`](./language-support.md).

---

## `orphan_symbol` (full-only)

Reports private Rust constants, statics, type aliases, and unions with no
resolved source reference.

Algorithm: same-name candidates are disambiguated with local references,
explicit imports, aliases, and re-exports. Tests contribute reference context;
ambiguous or dynamic resolution remains `unresolved` and does not emit.

Language contract: see [`language-support.md`](./language-support.md).

---

## `cognitive_complexity` (gating)

Cognitive-complexity detector with AST control-flow nesting and typed evidence.

Algorithm: the SonarSource specification (G. Ann Campbell, "Cognitive
Complexity", 2017):

- `1 + nesting` for `if`, a ternary, a whole `switch`/`match` (once, not per
  arm), each loop, and each `catch`/`except`; each also nests its contents;
- `+1` with no nesting penalty for `else if`/`elif` and `else`, so an
  `else if` chain stays at its first `if`'s level;
- `+1` per run of like boolean operators (`a && b && c` adds 1, `a && b || c`
  adds 2), per labeled `break`/`continue`, and once for direct recursion
  (the whitepaper counts each method in a recursion cycle once, not each
  call). `return value` is not a jump and adds nothing.

It fires above 15, Sonar S3776's default. `npath_complexity` owns
multiplicative path counting separately.

Conventions: the published specification is the reference; where it is
silent, EA follows the SonarQube family. One deliberate deviation: the
whitepaper folds nested functions and lambdas into the enclosing function,
while EA scores every closure, arrow function, and JavaScript `function`
expression as its own function, as SonarJS does, so a callback's complexity is
reported at the callback. Known oracle divergences, where EA matches the
specification: complexipy scores Python ternaries lower, and
rust-code-analysis undercounts `if let` nested inside `match`.

---

## `npath_complexity` (gating)

Reports functions with more than 200 acyclic execution paths.

Algorithm: Nejmeh's NPath (1988), with PMD's treatment of the constructs the
paper predates:

- sequential statements multiply, but paths that `return`, `throw`, or
  `raise` leave the function, so guard clauses add (`if (a) return; if (b)
  return;` has 3 paths, not 4); `break` and `continue` rejoin after the
  enclosing loop or switch, and a labeled `break outer` after the statement
  labeled `outer`;
- a condition has true and false outcomes, and each `&&`/`||` short-circuits:
  `a || b` is true two ways and false one way. An `if` consequence runs once
  per true outcome and each later `else if`/`else` once per false outcome of
  the conditions before it; without a final `else` the false outcomes fall
  through. Ternaries split the same way, and a boolean value outside a
  condition (`x = a || b`) has one path per operator plus one;
- a loop runs its body once per true outcome of its condition and is
  skipped once per false outcome; `while (true)`, Rust `loop`, and Go
  `for {}` cannot be skipped;
- a `switch` sums its cases. For Java, JavaScript, and C each case label is
  a path into its group (`case 1, 2 ->` is two), statements that fall
  through enter the next group, and a switch without `default` adds one path
  for matching no case. Go, Python, and Rust arms add one path each. A `try`
  counts only its body, since exception handlers are not normal-flow paths.

Conventions: for Java the reference is PMD `NPathComplexity`; capability tests
pin EA's count to PMD 7.16 on each construct above, and the corpus functions
PMD previously disagreed on (gson `JsonReader.nextUnquotedValue`, jsoup
`SimpleStreamReader.read`, `HtmlTreeBuilder.resetInsertionMode`,
`Tokeniser.consumeCharacterReference`) now match exactly. Labels are not
matched by name: a jump to an outer label also stops at an inner labeled
statement, which nested labels make rare.

Language contract: see [`language-support.md`](./language-support.md).

---

## `test_without_assertion` (full-only)

Reports a test that runs code but checks no result: no assertion, no
failure call, no compile-time check, no use of the test context, and no call
to a helper that asserts. Such a test passes whatever the code under test
returns.

Algorithm: a shared test-scope fact finds each framework's tests: Rust
`#[test]`/`#[tokio::test]`/`#[rstest]` functions (inline test modules
included), Python `test*` functions and `Test*`/`TestCase` methods in the
files pytest collects (`test_*.py`, `*_test.py`), JavaScript/TypeScript
`it`/`test`/`specify` callbacks in test files, Go `TestXxx(t *testing.T)`,
and Java `@Test`/`@ParameterizedTest`/`@RepeatedTest` and JUnit 3 `test*`
methods. Assertion evidence is:

- a call whose callee names an assertion verb (`assert`, `expect`, `verify`,
  `should`, `raises`, `fail`, `check`, `ensure`), or a throwing wait or query
  (`waitFor*`, `getBy*`, `findBy*`);
- Python and Java `assert` statements, a `raise` or `throw`, and Rust `?`,
  `unwrap`, a non-formatting macro (`assert!`, `panic!`, a crate's own
  checking macro), or a macro argument that unwraps;
- a compile-time check: a typed discarded binding (`let _: T = ..`,
  `var _ T = ..`) or a TypeScript type alias, `satisfies`, explicit type
  arguments, typed binding, or `@ts-expect-error`;
- any use of the per-test context (Go `t`, an AVA `t`, a Mocha `done`) other
  than logging or configuration;
- a call to, or a passed value of, a repository function that asserts
  directly or transitively, and an undeclared method inherited from a
  framework base class (`this.runCase()`, `self.run_case()`).

Skipped tests, expected-failure tests (`#[should_panic]`,
`@Test(expected = ..)`), and value-returning Rust tests are exempt, as are
tests whose name declares another check: smoke tests (`does_not_raise`,
`renders without crashing`, `contextLoads`), constructor-exists tests, and
placeholders, and in TypeScript, type tests (a test or file name mentioning
types or inference).

Language contract: Tier 1; the Tier-2 frameworks are deferred (see
[`language-support.md`](./language-support.md)).

---

## `tautological_assertion` (full-only)

Reports an assertion that cannot fail: it compares a literal with itself, or
an operand with itself under an equality the operand cannot redefine
(identity, structural, or reflective: Python `is`, Java `assertSame`, Jest
`toBe`/`toEqual`, Node `assert.equal`, testify `Equal`); asserts an
always-true literal (`assertTrue(true)`, `assert True`, Python's
always-truthy `assert (x, "message")`); or asserts that a mock returns the
value it was just stubbed with
(`when(m.get(1)).thenReturn(5); assertEquals(5, m.get(1))`).

Algorithm: the equality and truth assertions of each framework (Rust
`assert!`/`assert_eq!`, pytest and `unittest`, Jest/Vitest `expect`, Node and
Chai `assert`, AVA `t`, testify `assert`/`require`, JUnit, AssertJ, and
Hamcrest) yield an expected/actual pair or a literal. Comparing an object
with itself under its own `equals`, `__eq__`, or `PartialEq`
(`assertEquals(token, token)`) is a reflexivity test and is not reported.
Self-comparisons count only for operands without calls, mutation, fresh
allocations, or NaN. Mock stubs are Mockito `when(..).thenReturn`,
`unittest.mock` `return_value`, Jest/Vitest `mockReturnValue` (not a
`spyOn` of a real object, which checks the spy intercepts it), testify
`On(..).Return`, and mockall `return_const`, read in the same test before the
assertion.

Language contract: Tier 1; the Tier-2 frameworks are deferred (see
[`language-support.md`](./language-support.md)).

---

## `skipped_test_without_reason` (full-only)

Reports a test, or a group of tests, disabled with no reason written down:
Rust `#[ignore]`, pytest `@pytest.mark.skip` and `@unittest.skip` (on a
test or a test class), Jest/Vitest/Mocha `it.skip`, `xit`, `test.skip`,
`describe.skip`, and `xdescribe`, Go `t.Skip()` and `t.SkipNow()`, and JUnit
`@Disabled`/`@Ignore` (on a test or a test class, in Java or Kotlin) or
TestNG `@Test(enabled = false)`, and C# xUnit `[Fact(Skip = "")]`/
`[Theory(Skip = "")]` and NUnit or MSTest `[Ignore]` (on a test or a
fixture class). In C and C++ test files: GoogleTest `TEST`/`TEST_F`/
`TEST_P`/`TYPED_TEST` with a `DISABLED_` suite or test name, `GTEST_SKIP()`,
CppUTest and Unity fixture `IGNORE_TEST`, Unity `TEST_IGNORE()`, Catch2
`TEST_CASE` (and its variants) hidden by a `[.]` or `[!hide]` tag, Catch2
`SKIP()`, and doctest `* doctest::skip()` on a test case or a
`TEST_SUITE`. GoogleTest tests are named `Suite.Name`. C and C++ test files
are read as parsed, since error recovery would quarantine every Catch2 and
doctest test.

A skip states its reason when the mark carries one (`#[ignore = ".."]`,
`skip(reason=..)`, `@Disabled("..")`, `t.Skip("..")`, `SKIP("..")`,
`TEST_IGNORE_MESSAGE("..")`, `GTEST_SKIP() << ".."`, a doctest
`doctest::description("..")`), when a Go, GoogleTest, Catch2, or Unity skip
runs under a condition (`if testing.Short() { t.Skip() }`, an `#ifdef`, a
computed `doctest::skip(cond)`), when a hidden Catch2 test carries another
tag (`[.slow]` is shorthand for `[.][slow]`), or when a comment gives it. A plain comment beside the mark counts: after it on the same line,
inside the attribute or annotation block, or ending on the line above. A doc
comment above the test, a comment opening the test body, or a Python
docstring describes the test, so it counts only when it speaks of the skip
or a failure (skip, disabled, ignore, fails, flaky, broken, TODO, FIXME,
manual) or points to an issue (`#123`, `LANG-1655`, a URL); a comment
deeper in the body counts only when it names the skip (skip, disabled,
ignored, manual: "test ignored for now. Run manually."); a comment above
the enclosing class counts only when it speaks of the skip. Lint directives
(`eslint-disable`, `noqa`) are not reasons. Conditional skips (`skipif`,
`skipIf`, `#[cfg_attr(.., ignore)]`), expected failures (`xfail`), and
placeholders (`it.todo`) are not reported.

Language contract: Tier 1 and Tier 2 (see
[`language-support.md`](./language-support.md)).

---

## `pass_through_wrapper` (full-only)

Reports a private function whose whole body calls one other function with
its own parameters, unchanged and in order (`return f(a, b)`, a tail
expression, through an `await`; spread `*args`/`**kwargs`, `...rest`, and
`args...` count as passed on). It adds a name and a call and nothing else,
and its callers, all in the same module, could call the target directly.

Read: Rust free and associated functions without `pub` (or with
`pub(crate)`/`pub(super)`), module-level undecorated Python functions named
`_x`, top-level JavaScript/TypeScript functions and `const f = (..) => ..`
arrows that are not exported (by `export`, `module.exports`, or
`exports.x`), Go functions with a lowercase name, and Java
static methods that are not `public` or `protected`. The target is a plain
function name or one qualified by an imported module or package, a Rust
path, a Java type name, or a JavaScript global namespace (`Array`, `JSON`,
`Math`, `console`, ..).

Not reported:

- a public or exported function: API, such as a facade over internals, a
  convenience name, or an overload, which callers outside the module cannot
  replace;
- a function that adds, drops, changes, or reorders an argument, or takes
  no parameters (an accessor or hook);
- a method with a receiver (delegation, often required by an interface), a
  Rust trait implementation or default method, and a Java overload set;
- a data object's method (`PATTERN.test(s)`, `MAP.get(k)`,
  `utils.LINEBREAK.test(s)`): the object is bound by the wrapper, not
  supplied by the caller; and a constructor (`Self(x)`, a capitalized
  Python call);
- a Python decorator or a Java annotation, a Rust `#[cfg(..)]`,
  `#[no_mangle]`, or `extern` function, a Go build-constrained file or
  `//go:linkname`/`//export` function, and a conditional Python definition
  (under `try` or `if`);
- explicit type arguments (`parse::<T>(s)`, `decode<T>(s)`) and a TypeScript
  type predicate (`value is T`), which add a type;
- a function whose comment or docstring, or its class's or impl's, gives the
  indirection a purpose: deprecated, an alias, compatibility, legacy, a test
  seam (bridge, mock, testing, seam), a deferred lookup (lazy, runtime), or
  a hook.

Language contract: Tier 1 (see
[`language-support.md`](./language-support.md)).

## Graph Analysis Support Detectors

### `tooling_overlap` (gating, `--features graph-analysis`)

Detects multiple executable shell entry points in the same `tooling/`
directory that use the `run-<role>-<variant>.sh` convention for the same role.

## Graph-Guided Refactoring Candidates (`graph-analysis`)

`large_file_split_candidate` reports files at the 750-line advisory or
1,000-line strict threshold only when the authoritative symbol-reference graph
contains at least two disconnected clusters of three or more symbols.

`small_file_consolidation_candidate` reports non-entry source files with one
importer at the 100-line strict or 150-line advisory threshold. Both detectors
are input-gated and silent without a supplied repo graph.
