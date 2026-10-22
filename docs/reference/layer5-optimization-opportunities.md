# Layer 5 - Optimization Opportunities

Profiles: 5 gating, 9 full-only
Detectors: 14 (all unconditional)

Layer 5 reports only transformations with an explicit semantics-preservation
argument. Twelve owners consume one scan-scoped `OptimizationIndex` derived
from structure and algorithmic facts; the Java-specific buffer owner remains a
bounded ecosystem rule. The shared proof records invariant operands, purity,
intervening mutation, evaluation order, failure behavior, receiver/container
type, known size, and result use. An unresolved obligation fails closed.
`eager_log_formatting` reads log calls from the syntax tree.

## `cache_miss_opportunity` (gating)

Detects at least three identical known-pure calls in one control region when
their typed operands are unchanged between calls. Supported APIs are deliberately
small: typed Rust string transforms, typed Python `abs`/`len`, typed
TypeScript numeric `Math` operations, and typed Go `math.Abs`/`Floor`/`Ceil`/
`Round` and `strings.ToLower`/`ToUpper`, JavaScript numeric `Math` operations
typed by JSDoc, and Java `Math.abs`/`floor`/`ceil`/
`round` on numeric locals and `String.toLowerCase`/`toUpperCase`/`trim`,
matched by resolved API identity.

## `repeated_format_allocation` (gating)

Detects a formatting call inside a loop only when every dynamic operand is a
loop-invariant primitive with known formatting behavior. Enclosing calls that
merely contain a format expression, custom debug/display values, and
loop-dependent operands are excluded. Go `fmt.Sprintf`, Java
`String.format`, and Node `util.format` qualify by resolved identity;
JavaScript template literals are not calls and are not claimed. A call guarded by a switch on, or an equality test of, the key of a
loop over a map runs at most once per loop (map keys are unique) and is not
reported.

## `loop_invariant_regex_compilation` (gating)

Detects a recognized, unshadowed regex compiler inside a loop when the pattern
is invariant. Rust, Python, JavaScript, TypeScript, and Java arms require the
expected import or constructor identity; Go matches `regexp.Compile` and
`regexp.MustCompile` by resolved identity. The recommendation uses lazy
first-use caching so zero-iteration and first-failure behavior are preserved.

It also follows calls into repository functions, through up to four resolved
calls (`call_graph.rs`): a loop calling a helper that compiles a fixed
expression is reported at the call site, with `via` evidence naming the
helper chain and Medium confidence. A helper compiles a fixed expression when
its uncached compile reads only plain values: its parameters, literals, and
names it never assigns, with no nested call and no member of `self`, `this`,
or a parameter. A call carries the compilation when its own arguments meet
the same rule, and the loop call when they are also loop-invariant. A Python
helper decorated with `@functools.cache` or `@lru_cache` compiles once, and a
caller that caches the helper's result on first use is excluded as in the
direct case.

## `immutable_string_concatenation_in_loop` (gating)

Detects ordered concatenation into a pre-loop empty string over a typed string
collection. Nested control, non-string element coercion, and unproved mutable
builder behavior are excluded. Semantic arms cover Python, JavaScript,
TypeScript, Go, Java, and Kotlin, and Rust `output = format!("{output}..")`
over a pre-loop empty `String` (`push_str` or `write!` appends in place).

## `eager_log_formatting` (gating)

Detects a debug or trace log call whose message is formatted before the
call: a Python f-string, `%` or `.format`, a JavaScript/TypeScript template
literal with a substitution, Java `String.format` or `.formatted`, Go
`fmt.Sprintf`, or concatenation of a string with a value. The logger checks
the level inside the call, so with debug logging off the string is built and
discarded on every call; passing the values as arguments (`%s` arguments,
SLF4J `{}` placeholders, `Debugf`, structured fields) formats only enabled
messages (Pylint `W1201`-`W1203`, SonarJava `S2629`). The receiver must name
a logger (`logger`, `log`, `LOG`, `self._log`, `logging`, `slog`); `console`
formats whatever the level. Calls inside an enabled-level guard
(`isDebugEnabled()`, `isEnabledFor(DEBUG)`) are lazy already, and Rust is not
applicable: the `log` and `tracing` macros check the level before evaluating
their arguments. Python, JavaScript, TypeScript, Go, and Java.
Gating since its corpus labels met the precision bar: all 40 sampled
findings (Python and Java, on the application slice) are corroborated true
positives.

## `manual_collection_copy_loop` (full-only)

Detects an indexed copy whose loop contains exactly one same-index assignment
and no other calls or work: Rust over `0..source.len()`, Go over a slice ranged
by index (`for i := range source`), which `copy` replaces, and Java counting
`i < source.length` over an array (PMD `AvoidArrayLoops`; `System.arraycopy`
or `Arrays.copyOf`), and JavaScript/TypeScript counting the same way over a
typed array (`slice` or `Array.from`), and Python over
`range(len(source))` on a typed list (`target[:] = source`). Transforms, side effects, alias-uncertain targets, and
extra statements are excluded.

## `eager_fallback_computation` (full-only)

Detects Rust eager fallback APIs, and Java `Optional.orElse`, when the
fallback is a known-pure allocation with an equivalent lazy form (an empty JDK
collection or `StringBuilder` for `orElseGet`). Unknown calls, mutation, and failure-sensitive
fallbacks are excluded.

## `intermediate_collection_materialization` (full-only)

Detects three proven materialization shapes: Rust iterator collection into a
`Vec` used only for `len`, Python typed identity comprehensions passed directly
to a generator-capable terminal, and Kotlin typed collection spread through
`toTypedArray`, a Java stream collected to a list used only for `size()`
(`count()`), and a JS/TS `filter` with a call-free arrow predicate whose
result is only tested for emptiness (`some`) or read at index 0 (`find`). Rust `is_empty`, Python transforms, nested calls, and untyped
Kotlin sources are excluded because evaluation behavior is not equivalent.

## `range_for_each_iteration` (full-only)

Detects Kotlin range expressions consumed through `forEach`, matching Detekt's
`ForEachOnRange` capability, Rust `(a..b).for_each(..)` (clippy
`needless_for_each`), and Java `IntStream`/`LongStream` `range(..).forEach(..)`. Ordinary collections and conventional `for` loops
are excluded.

## `loop_invariant_computation` (full-only)

Detects a known-pure typed computation inside a loop when its operands are
invariant and it is not already owned by the regex, formatting, or Layer 3
contracts. Semantic arms cover Rust, Python, JavaScript, TypeScript, Go, and
Java.

## `known_size_preallocation` (full-only)

Detects a Go zero-capacity slice that receives exactly one unconditional append
per iteration over a known collection, and a Java `new ArrayList<>()` that
receives one unconditional transformed `add` per iteration. Nested control,
extra work, and existing capacity are excluded; the finding carries
`len(collection)` or `collection.size()` as known-size evidence. A Java loop
that adds each element unchanged is `bulk_collection_extension`'s. Rust
`Vec::new()` or `vec![]` receiving one unconditional transformed `push` per
element of a slice or `Vec` qualifies the same way (`Vec::with_capacity`).

## `bulk_collection_extension` (full-only)

Detects an identity-only append/push loop over proven sequence types when the
source and target differ and the loop has no extra work. Semantic arms cover
Rust, Python, JavaScript, TypeScript, Go, and Java (`addAll` between declared
collections; a Java `clone()` per element is not identity). The Go arm requires the target to
be a slice parameter (`append(target, source...)`); a fresh empty slice filled
the same way is a copy owned by `known_size_preallocation`, matching
golangci-lint `prealloc`.

## `map_entry_opportunity` (full-only)

Detects a Rust `HashMap`/`BTreeMap` `contains_key` then `insert` sequence, or
a Java `Map` `containsKey` then `put` (`putIfAbsent`), with the same map and
key, a known-pure value, and no intervening map access or mutation, and a
Python dict guarded by `key not in d` and filled with `d[key] = []` (or
another literal or empty constructor, which `setdefault` may evaluate eagerly). Unknown value construction and control-flow variants fail closed.

## `java_inefficient_string_buffer` (full-only)

Detects local Java `StringBuffer` construction where thread-safe synchronization
is not required and `StringBuilder` is the appropriate local replacement.

The exact detector-language modes are generated from the executable catalog in
[`language-support.md`](./language-support.md).
