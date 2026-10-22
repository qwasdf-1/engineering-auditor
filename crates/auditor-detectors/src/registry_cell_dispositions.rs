//! Reviewed dispositions for Tier-1 and Tier-2 detector-language cells that no support
//! policy covers: why the cell is not applicable, or the specific frontend
//! fact it still lacks. A disposition applies only where the policy leaves the
//! cell unsupported, and a test rejects one that names a supported cell.

use crate::LanguageSupportMode;

pub(crate) struct CellDisposition {
    pub(crate) ids: &'static [&'static str],
    pub(crate) languages: &'static [&'static str],
    pub(crate) mode: LanguageSupportMode,
    pub(crate) reason: &'static str,
}

const fn not_applicable(
    ids: &'static [&'static str],
    languages: &'static [&'static str],
    reason: &'static str,
) -> CellDisposition {
    CellDisposition {
        ids,
        languages,
        mode: LanguageSupportMode::NotApplicable,
        reason,
    }
}

const fn deferred(
    ids: &'static [&'static str],
    languages: &'static [&'static str],
    reason: &'static str,
) -> CellDisposition {
    CellDisposition {
        ids,
        languages,
        mode: LanguageSupportMode::Unsupported,
        reason,
    }
}

const TIERED: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "kotlin",
    "csharp",
    "c",
    "cpp",
];
const TIER_TWO: &[&str] = &["kotlin", "csharp", "c", "cpp"];
const KOTLIN_CSHARP: &[&str] = &["kotlin", "csharp"];
const C_CPP: &[&str] = &["c", "cpp"];
const NON_PYTHON: &[&str] = &[
    "rust",
    "javascript",
    "typescript",
    "go",
    "java",
    "kotlin",
    "csharp",
    "c",
    "cpp",
];
/// Algorithmic capabilities no Tier-2 frontend resolves yet.
const ALGORITHMIC_IDS: &[&str] = &[
    "repeated_sort_in_loop",
    "sort_for_extremum",
    "full_sort_for_top_k",
    "repeated_collection_traversal",
    "hot_path_linear_membership",
    "n_plus_one_query_in_loop",
    "loop_carried_copy_accumulation",
    "repeated_recursive_subproblem",
    "nested_cross_collection_traversal",
];
/// Systems capabilities no Tier-2 frontend resolves yet.
const SYSTEMS_IDS: &[&str] = &[
    "architectural_layering",
    "package_coupling_metrics",
    "loop_artifact_rewrite",
    "latest_only_unbounded_cache",
    "repeated_parse_scan_serialize",
    "per_item_external_io",
    "repeated_expensive_initialization",
    "request_path_repository_scan",
];
/// Optimizations that apply to every Tier-2 language but are not resolved.
const OPTIMIZATION_IDS: &[&str] = &[
    "cache_miss_opportunity",
    "repeated_format_allocation",
    "loop_invariant_regex_compilation",
    "manual_collection_copy_loop",
    "loop_invariant_computation",
];

pub(crate) const CELL_DISPOSITIONS: &[CellDisposition] = &[
    not_applicable(
        &["stale_path_reference", "stale_link_reference", "stale_section_anchor", "generated_vs_source"],
        TIERED,
        "A documentation and path capability: it reads Markdown links, anchors, and generated-file markers, not source code.",
    ),
    not_applicable(
        &["flask_debug_true", "try_except_pass"],
        NON_PYTHON,
        "A Python construct (Flask's debug server, `except: pass`); the language has no counterpart.",
    ),
    not_applicable(
        &["assert_in_production"],
        &["rust", "javascript", "typescript", "go"],
        "The language has no assertion that release builds strip: Rust's `assert!` and Node's `assert` always run, and Go has no assert.",
    ),
    not_applicable(
        &["assignment_in_operand"],
        &["python", "go"],
        "Assignment is not an expression in Go, and Python's walrus operator is the deliberate form of it.",
    ),
    not_applicable(
        &["debugger_in_production"],
        &["go", "java"],
        "The language has no debugger statement or debug-leftover macro.",
    ),
    not_applicable(
        &["empty_catch_block"],
        &["rust", "go"],
        "The language has no `catch`: errors are values.",
    ),
    not_applicable(
        &["eval_in_production"],
        &["rust", "go"],
        "The language has no runtime evaluation of source text.",
    ),
    not_applicable(
        &["unsafe_deserialization"],
        &["rust", "go"],
        "Standard and common deserializers (serde, `encoding/*`) construct plain data and run no code.",
    ),
    not_applicable(
        &["xml_xxe_vulnerable"],
        &["rust", "go"],
        "The XML parsers in use (`encoding/xml`, quick-xml, roxmltree, xml-rs) resolve no external entities.",
    ),
    not_applicable(
        &["resource_not_closed"],
        &["rust"],
        "Rust releases resources by RAII when the owner goes out of scope.",
    ),
    not_applicable(
        &["no_await_in_loop"],
        &["go", "java"],
        "The language has no `await`; goroutines and futures are composed explicitly.",
    ),
    not_applicable(
        &["require_atomic_updates"],
        &["rust", "python", "go", "java", "kotlin", "csharp", "c", "cpp"],
        "A JavaScript event-loop race (a read-modify-write split by `await`); concurrency here is governed by threads, locks, or ownership instead.",
    ),
    not_applicable(
        &["unhandled_termination_in_production"],
        &["python", "javascript", "typescript", "java"],
        "Exceptions are the language's normal error channel, so a raise or throw is not an unhandled termination.",
    ),
    not_applicable(
        &["blocking_io_in_async_context"],
        &["go"],
        "Goroutines are not async functions: the runtime parks a goroutine blocked on I/O.",
    ),
    not_applicable(
        &["identifier_stutter"],
        &["python", "javascript", "typescript", "java", "kotlin", "csharp", "c", "cpp"],
        "The language's naming convention has no package-prefix stutter rule (Rust RFC 430 and Effective Go do).",
    ),
    not_applicable(
        &["duplicate_import"],
        &["go"],
        "The Go compiler rejects duplicate imports.",
    ),
    not_applicable(
        &["function_local_import"],
        &["go", "java"],
        "The language has no function-local imports.",
    ),
    not_applicable(
        &["duplicate_import"],
        &["rust"],
        "Importing one name twice is a Rust compile error (E0252); separate `use` items of one module are rustfmt's default granularity.",
    ),
    not_applicable(
        &["function_local_import", "collection_parameter_mutation"],
        &["rust"],
        "Idiomatic Rust: a function-scoped `use` limits an import to where it is needed, and `&mut` declares a collection mutation in the signature.",
    ),
    not_applicable(
        &["god_module"],
        &["java"],
        "A Java file's top-level unit is its class, whose size `class_structure_metrics` judges; methods are not module definitions.",
    ),
    not_applicable(
        &["out_parameter"],
        &["rust", "go"],
        "The signature declares every write to a caller's value (`&mut T`, `*T`), so an output argument cannot surprise the reader.",
    ),
    not_applicable(
        &["invariant_collection_copy_in_loop"],
        &["javascript", "typescript", "go", "java", "csharp", "cpp"],
        "Copying a collection only to measure it has no idiom in the language.",
    ),
    not_applicable(
        &["loop_carried_copy_accumulation"],
        &["go"],
        "Go's `append` grows a slice in amortized constant time; Go has no rebuild-by-concatenation idiom for slices.",
    ),
    not_applicable(
        &["eager_fallback_computation"],
        &["python", "javascript", "typescript", "go"],
        "Fallbacks are already lazy (`??`, `||`, `or`, conditional expressions), Go has no optional type, and Python's default-taking APIs have no lazy form.",
    ),
    not_applicable(
        &["intermediate_collection_materialization"],
        &["go"],
        "Go has no lazy iterator chain whose materialization could be skipped.",
    ),
    not_applicable(
        &["known_size_preallocation"],
        &["python", "javascript", "typescript"],
        "The language has no capacity reservation for its growable sequence (`new Array(n)` makes a holey array).",
    ),
    not_applicable(
        &["map_entry_opportunity"],
        &["javascript", "typescript", "go"],
        "The language's map has no entry or compute-if-absent API.",
    ),
    not_applicable(
        &["range_for_each_iteration"],
        &["python", "javascript", "typescript", "go"],
        "The language has no range object consumed by a `forEach` callback; a counting loop is already the idiom.",
    ),
    not_applicable(
        &["function_local_import"],
        &["javascript", "typescript"],
        "ES imports cannot be function-local; a function-local `require()` or `import()` is CommonJS or ESM lazy loading, the exception the rule allows.",
    ),
    not_applicable(
        &["identifier_tersity"],
        &["go"],
        "Effective Go asks for short local names (`r`, `w`, `buf`), so terse locals are the convention.",
    ),
    deferred(
        &["collection_parameter_mutation"],
        &["javascript"],
        "Untyped parameters: a collection cannot be told from an output sink or builder (a `StringBuilder`-like accumulator), which dominated the typed-language probe.",
    ),
    deferred(
        &["collection_parameter_mutation"],
        &["go"],
        "Go mutates slices and maps through indexing and `append`, not method calls; the rule needs index-write facts for slice and map parameters.",
    ),
    deferred(
        &["source_duplicate_detection"],
        &["java"],
        "Below the precision bar on the corpus sample (15 of 24): primitive specializations (int/long, float/double), mirror-image algorithms, and deliberately monomorphic copies repeat by necessity.",
    ),
    deferred(
        &["class_structure_metrics"],
        &["rust", "go"],
        "Needs type-member ownership: methods attached to a type through `impl` blocks or receivers.",
    ),
    deferred(
        &["out_parameter"],
        &["python", "javascript", "typescript"],
        "Reviewed and rejected: writing a parameter object's fields is how these languages fill, normalize, and configure objects; the corpus probe (347 findings) was dominated by such designed writes.",
    ),
    deferred(
        &["sibling_call_redundancy"],
        &["javascript", "typescript", "go"],
        "Needs the receiver-method call shape: shared callees here are mostly methods on different receivers.",
    ),
    deferred(
        &["sibling_call_redundancy"],
        &["rust", "java"],
        "Below the precision bar on the corpus sample (2 of 10): trait implementations, builder setters, and API facades (`get`/`post`) share callees by design.",
    ),
    deferred(
        &["stringly_typed_parameter"],
        &["go"],
        "Go structure facts record no string literals and model `switch` cases as plain branches, so a sentinel comparison has no evidence to read.",
    ),
    deferred(
        &["stringly_typed_parameter"],
        &["rust"],
        "Below the precision bar on our own Rust (0 of 2): parsers receive string kinds from external APIs (tree-sitter node kinds), which an enum cannot replace.",
    ),
    deferred(
        &["identifier_verbosity"],
        &["rust", "javascript", "typescript", "go", "java"],
        "Lexical occurrences include type and enum-variant names, which carry domain vocabulary (a `match` over long variant names reads as verbose); the rule needs binding-level identifiers before it extends.",
    ),
    deferred(
        &["identifier_tersity"],
        &["rust", "javascript", "typescript", "java"],
        "Below the precision bar on the corpus sample (1 of 16): one-line predicates and callbacks use conventional short names (`fn`, `xs`, `m`), and Rust inline tests are not excluded from the lexical spans.",
    ),
    deferred(
        &["escape_hatch_type_density"],
        &["rust", "javascript", "go", "java"],
        "Needs a per-language escape-hatch table (`unsafe`/`transmute`, untyped JavaScript, `interface{}`, raw casts and `@SuppressWarnings`).",
    ),
    deferred(
        &["dead_code", "orphan_symbol"],
        &["python", "javascript", "typescript", "go", "java", "kotlin", "csharp", "c", "cpp"],
        "Needs cross-file reference resolution and the language's export model (`__all__`, ES exports, Go capitalization, Java and Kotlin visibility, C# assemblies, C/C++ linkage and headers); only Rust has it.",
    ),
    deferred(
        &["churn_complexity_hotspot"],
        &["javascript", "typescript", "go", "java", "kotlin", "csharp", "c", "cpp"],
        "History-dependent: the depth-1 ecosystem corpus has no commit history, so the cell has no precision sample yet.",
    ),
    deferred(
        &["unhandled_termination_in_production"],
        &["go"],
        "Reviewed and rejected: every corpus `panic` was an API-contract guard, the convention that demoted the Rust cell to the full profile.",
    ),
    deferred(
        &[
            "test_without_assertion",
            "tautological_assertion",
            "skipped_test_without_reason",
        ],
        TIER_TWO,
        "The test-scope facts model the Tier-1 frameworks; the Tier-2 forms (Kotlin JUnit and MockK, C# xUnit, NUnit, and MSTest, GoogleTest and Catch2 macros) are not modelled yet.",
    ),
    not_applicable(
        &["quadratic_front_removal"],
        &["go"],
        "Go drops a slice's front by reslicing (`xs = xs[1:]`), which copies nothing.",
    ),
    not_applicable(
        &["catastrophic_backtracking_regex"],
        &["rust", "go"],
        "The standard engines (Rust's `regex`, Go's `regexp`) are linear-time automata that never backtrack.",
    ),
    deferred(
        &["quadratic_front_removal"],
        &["kotlin", "c"],
        "The front removals (Kotlin `removeAt(0)`, C `memmove` over an array) and their array-backed declarations are not modelled yet.",
    ),
    deferred(
        &["catastrophic_backtracking_regex"],
        &["kotlin", "c"],
        "The backtracking engines (Kotlin `Regex`, POSIX `regcomp`) take patterns through APIs the regex literal extraction does not model yet.",
    ),
    deferred(
        &["float_equality_comparison"],
        TIER_TWO,
        "The Tier-2 float literal and comparison forms (Kotlin `Double`/`Float`, C# `double`/`float`/`decimal`, C and C++ suffixes) are not modelled yet.",
    ),
    deferred(
        &["discarded_fallible_result"],
        TIER_TWO,
        "The Tier-2 discards (Kotlin `runCatching` results, C# unobserved `Task`s, C and C++ `warn_unused_result` returns) are not modelled yet.",
    ),
    deferred(
        &["unused_declared_dependency"],
        KOTLIN_CSHARP,
        "Gradle build scripts and MSBuild project files are not parsed yet.",
    ),
    deferred(
        &["unused_declared_dependency"],
        &["c", "cpp"],
        "C and C++ have no standard manifest; vcpkg and Conan manifests are not parsed yet.",
    ),
    not_applicable(
        &["eager_log_formatting"],
        &["rust"],
        "The `log` and `tracing` macros check the level before evaluating their arguments, so a message is never formatted for a disabled level.",
    ),
    deferred(
        &["eager_log_formatting"],
        TIER_TWO,
        "The Tier-2 logger APIs (Kotlin lambda messages, C# `ILogger` templates and interpolated handlers, C and C++ logging macros) are not modelled yet.",
    ),
    deferred(
        &["retry_without_backoff"],
        TIER_TWO,
        "The Tier-2 retry shapes (Kotlin `runCatching` and `repeat`, C# `catch` filters, C and C++ return-code loops) and their wait calls are not modelled yet.",
    ),
    deferred(
        &["network_call_without_timeout"],
        KOTLIN_CSHARP,
        "Kotlin's common clients (OkHttp, Ktor engines) and .NET's HttpClient bound requests by default; the unbounded forms (Ktor engines without HttpTimeout, a `Timeout` of `InfiniteTimeSpan`) are not in the client API table yet.",
    ),
    deferred(
        &["network_call_without_timeout"],
        C_CPP,
        "libcurl's default is no timeout (`CURLOPT_TIMEOUT` 0), but the client API table does not model curl handles and their option calls yet.",
    ),
    // Tier-2 L1.
    not_applicable(
        &["assignment_in_operand"],
        &["kotlin"],
        "Assignment is not an expression in Kotlin.",
    ),
    not_applicable(
        &["debugger_in_production"],
        &["kotlin"],
        "Kotlin has no debugger statement or debug-trap call in its standard library.",
    ),
    not_applicable(
        &["empty_catch_block"],
        &["c"],
        "C has no `catch`: errors are return codes.",
    ),
    not_applicable(
        &["eval_in_production", "unsafe_deserialization"],
        C_CPP,
        "Compiled languages with no runtime evaluation of source text, and no standard deserializer that constructs arbitrary types.",
    ),
    not_applicable(
        &["resource_not_closed"],
        &["cpp"],
        "C++ releases resources by RAII (`std::ifstream`, smart pointers) when the owner goes out of scope.",
    ),
    not_applicable(
        &["unhandled_termination_in_production"],
        KOTLIN_CSHARP,
        "Exceptions are the language's normal error channel, so a throw is not an unhandled termination.",
    ),
    not_applicable(
        &["no_await_in_loop"],
        &["c"],
        "C has no `await`.",
    ),
    deferred(
        &["assert_in_production"],
        C_CPP,
        "An `assert` compiles out under `NDEBUG`, but whether a build defines it lives in the build system; the rule needs the build configuration to tell a stripped check from a live one.",
    ),
    deferred(
        &["sql_injection"],
        C_CPP,
        "SQL text is assembled into buffers (`snprintf`, `strcat`, `std::string` appends) before `sqlite3_exec` or `PQexec`; the rule needs buffer dataflow to connect the build to the sink.",
    ),
    deferred(
        &["unhandled_termination_in_production"],
        C_CPP,
        "Calls to `abort` and `std::terminate` guard broken invariants in libraries much as Go's `panic` does; the Go cell was reviewed and rejected for the same convention.",
    ),
    deferred(
        &["no_await_in_loop"],
        &["kotlin"],
        "Kotlin suspends implicitly: a suspend call in a loop has no `await` marker, so the rule needs suspend-function resolution to find sequential awaits.",
    ),
    deferred(
        &["no_await_in_loop"],
        &["cpp"],
        "C++20 coroutines (`co_await`) appear in none of the corpus repositories, so the cell has no capability evidence or precision sample.",
    ),
    // Tier-2 L2.
    deferred(
        &["magic_literal_cluster"],
        TIER_TWO,
        "Below the precision bar on the corpus sample (Kotlin 0 of 12, C# 5 of 12; C and C++ probed alike): literals are mostly byte layouts, encoding and hash specification constants (UTF-8, Base64, SHA, IEEE-754), and value-keyed caches, which naming would not clarify.",
    ),
    deferred(
        &["identifier_verbosity", "identifier_tersity", "sibling_call_redundancy", "function_local_import"],
        TIER_TWO,
        "Qualified on Python only; the Tier-1 extensions were reviewed and deferred (type names in lexical spans, conventional short names, shared receiver callees), and the Tier-2 grammars share those blockers.",
    ),
    deferred(
        &["stringly_typed_parameter"],
        KOTLIN_CSHARP,
        "No corpus evidence: the probe over the Kotlin and C# repositories produced no finding, so the cell has no precision sample.",
    ),
    deferred(
        &["stringly_typed_parameter"],
        C_CPP,
        "C strings are `char *` buffers whose comparison is a `strcmp` call, not an equality the structure facts record; C++ adds `std::string` overloads. The rule needs string-comparison facts for these grammars.",
    ),
    deferred(
        &["escape_hatch_type_density"],
        TIER_TWO,
        "Needs a per-language escape-hatch table (Kotlin `!!` and `as`, C# `dynamic` and `unsafe`, C `void *` casts, C++ `reinterpret_cast`).",
    ),
    deferred(
        &["source_duplicate_detection"],
        &["kotlin", "csharp", "cpp"],
        "Below the precision bar on the corpus sample (Kotlin 6 of 12, C# 7 of 12, C++ 2 of 9), for the reasons that deferred Java: per-type overloads and primitive specializations (`Varint32`/`Varint64`, `DateTime`/`DateTimeOffset`), mirror pairs (`Next`/`Prev`), and DSL families (`single`/`factory`/`scoped`) repeat by design.",
    ),
    deferred(
        &["collection_parameter_mutation"],
        C_CPP,
        "Collections are arrays and pointers (or STL containers passed by reference): a mutation through a non-`const` pointer or reference is declared in the signature, and the rule needs `const`-qualification facts to tell a surprising write from a designed output buffer.",
    ),
    deferred(
        &["out_parameter"],
        KOTLIN_CSHARP,
        "Below the precision bar on the corpus probe: writes to a parameter object's fields were designed output buffers and builders (okio segments, Newtonsoft readers), and C# `out`/`ref` parameters declare the write in the signature.",
    ),
    not_applicable(
        &["out_parameter"],
        C_CPP,
        "A pointer or reference parameter declares the write in the signature, and output pointers are the C convention for multiple results.",
    ),
    deferred(
        &["class_structure_metrics"],
        &["c"],
        "C has no classes; a struct-plus-functions module would need ownership by naming convention or first parameter, which the structure facts do not model.",
    ),
    deferred(
        &["god_module"],
        &["csharp"],
        "A C# file's top-level unit is its type; methods belong to classes that `class_structure_metrics` judges, and file-scoped namespaces need type-member ownership before module size means anything.",
    ),
    deferred(
        &["naming_convention"],
        C_CPP,
        "C and C++ have no single naming convention (K&R snake_case, Google C++ PascalCase types, Win32 Hungarian); the rule needs a per-project convention to compare against.",
    ),
    // Tier-2 L3-L5.
    deferred(
        ALGORITHMIC_IDS,
        &["kotlin"],
        "Needs Kotlin collection and cardinality facts: stdlib extension functions (`sorted`, `contains`, `first`) resolve by receiver type, and `Sequence` chains are lazy, so the Java tables do not transfer.",
    ),
    deferred(
        &["nested_quadratic_iteration", "invariant_collection_copy_in_loop"],
        &["c"],
        "Needs C loop and collection facts: index loops over arrays and hand-rolled lists.",
    ),
    deferred(
        &["invariant_collection_copy_in_loop"],
        &["kotlin"],
        "Needs Kotlin collection facts: `toList()` and `toMutableList()` copies resolve by receiver type.",
    ),
    deferred(
        &[
            "full_sort_for_top_k",
            "repeated_collection_traversal",
            "hot_path_linear_membership",
            "n_plus_one_query_in_loop",
            "loop_carried_copy_accumulation",
            "nested_cross_collection_traversal",
        ],
        &["c"],
        "C has no collection library to identify: arrays and hand-rolled lists need cardinality and call-cost facts the frontend does not derive.",
    ),
    not_applicable(
        &["loop_carried_copy_accumulation"],
        &["cpp"],
        "C++ grows a container in place (`push_back`, `insert`); rebuilding one by concatenation in a loop has no idiom.",
    ),
    deferred(
        &["full_sort_for_top_k"],
        &["cpp"],
        "C++ keeps a sorted prefix by resizing or copying an iterator range (`resize(k)`, `vector(v.begin(), v.begin() + k)`), which the selection facts do not read yet.",
    ),
    deferred(
        &[
            "latest_only_unbounded_cache",
            "loop_artifact_rewrite",
            "architectural_layering",
            "package_coupling_metrics",
        ],
        &["c"],
        "C keeps caches in hand-rolled arrays and reads and writes files through `FILE *` streams (`fread`, `fwrite`) the artifact facts do not follow; its topology needs include paths resolved through the build's include directories.",
    ),
    deferred(
        SYSTEMS_IDS,
        &["kotlin"],
        "Needs the language's effect, lifecycle, and topology classifiers (I/O, network, and database APIs per ecosystem; module and package graphs from Gradle or CMake).",
    ),
    deferred(
        &["loop_artifact_rewrite"],
        &["cpp"],
        "C++ streams read and write an artifact through `>>` and `<<` on stream objects (`std::ifstream in(path)`), which the artifact facts do not follow from read to write.",
    ),
    deferred(
        &["architectural_layering", "package_coupling_metrics"],
        &["cpp"],
        "C++ topology needs include paths resolved through the build's include directories (CMake or compiler flags), which the scan does not read.",
    ),
    deferred(
        &["architectural_layering", "package_coupling_metrics"],
        &["csharp"],
        "C# `using` directives name namespaces, not files: layering and coupling need a namespace-to-file index and project (`.csproj`) boundaries.",
    ),
    deferred(
        &["blocking_io_in_async_context"],
        &["kotlin", "cpp"],
        "Needs async-context resolution (Kotlin `suspend` and dispatchers, C++ coroutines) plus blocking-I/O API identity.",
    ),
    not_applicable(
        &["blocking_io_in_async_context"],
        &["c"],
        "C has no async functions.",
    ),
    deferred(
        OPTIMIZATION_IDS,
        &["kotlin"],
        "Needs receiver-type, alias, and evaluation-order facts for the language's string, regex, and collection APIs.",
    ),
    not_applicable(
        &["repeated_format_allocation"],
        &["c"],
        "C formats into a caller's buffer (`snprintf`) rather than allocating a string per call.",
    ),
    deferred(
        &["manual_collection_copy_loop"],
        &["c"],
        "A C copy loop is bounded by a separate length (`i < n`) the facts do not tie to the arrays it copies.",
    ),
    deferred(
        &["intermediate_collection_materialization", "range_for_each_iteration"],
        &["cpp"],
        "Needs C++20 ranges facts: views materialized with `ranges::to` or iterated with `ranges::for_each` over `views::iota`.",
    ),
    deferred(
        &["eager_fallback_computation", "known_size_preallocation", "bulk_collection_extension", "map_entry_opportunity"],
        &["kotlin"],
        "Needs receiver-type facts for Kotlin's optional, sequence, and map APIs (`getOrDefault`, `ArrayList` capacity, `addAll`, `getOrPut`).",
    ),
    deferred(
        &["known_size_preallocation"],
        &["cpp"],
        "A default-constructed `std::vector` local is a declaration with no value, and the facts do not record C++ local declarations' types yet.",
    ),
    not_applicable(
        &[
            "immutable_string_concatenation_in_loop",
            "intermediate_collection_materialization",
            "range_for_each_iteration",
            "eager_fallback_computation",
            "known_size_preallocation",
            "bulk_collection_extension",
            "map_entry_opportunity",
        ],
        &["c"],
        "C has no string objects, lazy sequences, optional type, growable collection, or map in its standard library.",
    ),
];

/// The reviewed disposition of `(detector, language)`, if any.
pub(crate) fn cell_disposition(detector: &str, language: &str) -> Option<&'static CellDisposition> {
    CELL_DISPOSITIONS.iter().find(|disposition| {
        disposition.ids.contains(&detector) && disposition.languages.contains(&language)
    })
}
