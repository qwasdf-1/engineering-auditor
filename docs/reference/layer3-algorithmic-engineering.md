# Layer 3 — Algorithmic Engineering

Profiles: 4 gating, 9 full-only
Detectors: 13 (all unconditional)

Algorithmic detectors identify asymptotically inefficient collection, sorting,
traversal, membership, query, recursive, and copy/allocation patterns. One
compact `AlgorithmicIndex` is derived from the scan-owned `StructureIndex`; it
retains typed loop, collection-shape, mutation, selection, callback-effect,
control-path, recursive-call, copy, and query-dependence facts without another
source or AST copy. Each detector-specific owner constructs only its own finding
ID.

Every fact maps exhaustively to a source-spanned cost expression. The term
catalog is constant, linear, logarithmic, sort, repeated call, copy/allocation,
or unknown. Expressions compose deterministic sequential sums, feasible branch
maxima, and loop products, and every finding records its observed expression,
alternative expression, flattened terms, composition kind, and confidence
boundary. Unknown call behavior does not create a standalone complexity claim.

All 11 owners have executable positive and semantic-negative repository cases.
The language capability matrix retargets those real cases across every claimed
grammar arm and requires silence elsewhere. Distinct member paths, loop-local
collections, unknown callback effects, direct mutable calls, alias mutation,
comparator ordering, bounded membership, loop-independent queries, write/batch
APIs, memoization, infeasible recursive branches, and intentional Cartesian
output fail closed where those facts govern the capability.

Go operates on slices through package functions rather than methods. Calls
whose resolved API identity is a known collection function are read as the
matching method on their first argument: `sort.Ints(xs)` and `slices.Sort` are
a default sort, `slices.Contains` a membership test, `slices.ContainsFunc` and
`slices.IndexFunc` traversals (a function-literal callback is judged by its
body), and `sort.Slice` a comparator sort. Such calls are classified only by
that table, while any other call that passes a slice may mutate its shared
elements. Index and slice expressions after a sort (`xs[0]`, `xs[len(xs)-1]`,
`xs[:k]`) are selections. Query methods are compared in snake case
(`FindByID`), and `*sql.DB`, `Tx`, and `Conn` receivers prove a query.

Java reads `Collections.sort`, `Arrays.sort`, `Collections.min`/`max`/
`reverse`/`shuffle` the same way (a comparator argument makes `sort_by`), and
`list.sort(null)` as a default sort. `items.stream()` is `items` for traversal
identity, `anyMatch`/`allMatch`/`noneMatch` are terminal traversals with `->`
lambdas judged by body, and `get(0)`, `get(xs.size() - 1)`, `getFirst`,
`getLast`, and `subList(0, k)` after a sort are selections. A Java local's
declared type (`List<String>` vs `Set<String>`) decides its collection kind,
and any call that passes a collection may mutate it. A loop that rebuilds its
accumulator with `Stream.concat(acc.stream(), Stream.of(item))` is a
loop-carried copy. An inner collection assigned inside the outer loop is per
element and is not a cross-collection traversal (all languages).
`invariant_collection_copy_in_loop` covers Rust and Python (`len(xs.copy())`
on a typed list): copying a collection to measure it has no Java or Go idiom.

Python runs the collection rules on annotated lists (`items: list[str]`).
Subscripts are selections: `xs.sort()` followed by `xs[0]`, `xs[-1]`, or
`xs[:k]`, and `sorted(xs)[0]`, `sorted(xs)[-1]`, or `sorted(xs)[:k]` directly.
`list.count(x)` and `list.index(x)` are terminal traversals, and
`needle in xs` (which calls `xs.__contains__`) is a membership test. A
collection rebound inside a loop (`values = list(group)`) is fresh on each pass
and is not a repeated sort (all languages).

JavaScript runs the TypeScript collection rules. Where TypeScript proves a
collection by annotation, JavaScript proves it by the JSDoc block before the
function (`@param {string[]} wanted`), so a string's `includes` is never read
as array membership.

## Operations Through Calls

`repeated_sort_in_loop` and `n_plus_one_query_in_loop` follow calls into
repository functions, through up to four resolved calls
(`call_graph.rs`; see [Effects Through Calls](./layer4-systems-engineering.md#effects-through-calls)).
A function that sorts one of its parameters, or issues a read query whose
arguments name its parameters, passes that operation to its callers by
parameter position, and each call binds it to its arguments. A loop call is
then reported at the call site when the bound collection is unchanged by the
loop (a sort) or a bound query argument references the loop binding (a
query), with `via` evidence naming the helper chain and Medium confidence. A
named argument leaves the binding unknown, so no finding follows from it.
The other algorithmic owners read the function's own code only.

---

## `nested_quadratic_iteration` (full-only)

Detects nested iteration over the same scalable collection when the loops have
distinct bindings, establishing pairwise quadratic work rather than treating
all syntactic nesting as a performance defect. Inner collections rooted in the
outer binding, such as parent/child tree traversal, fail closed because their
aggregate cost is governed by the visited structure rather than an independent
pairwise product.

Evidence includes both bindings, exact loop spans, the shared collection,
dependence proof, and the `O(n^2)` cost model.

Language contract: see [`language-support.md`](./language-support.md).

---

## `repeated_sort_in_loop`

Detects sorting an invariant collection inside a loop. It rejects sorts of the
loop binding, the iterated collection, and collections mutated in the loop.

Evidence includes the collection kind, exact loop and sort spans,
`direct_mutation_count` (zero: no recognized write to the collection in the
loop), `calls_receiving_collection` (loop calls passed the collection itself,
such as a Rust helper given a `&mut` parameter by reborrow, whose effect on
it is unresolved), and the `O(m * n log n)` cost model. Algorithmic findings
that report `direct_mutation_count` count only directly recognized writes.

Language contract: see [`language-support.md`](./language-support.md).

---

## `sort_for_extremum`

Detects a default ascending sort followed by first/last or one-element
selection on the same unchanged receiver. Comparator sorts are excluded
because their ordering semantics may not match a direct extremum operation.

Evidence owns the `k = 1` boundary and the `O(n log n) -> O(n)` replacement
model.

Language contract: see [`language-support.md`](./language-support.md).

---

## `full_sort_for_top_k` (full-only)

Detects a default full sort followed by a statically bounded prefix selection
where `k > 1`. It is disjoint from `sort_for_extremum`, rejects intervening
mutation and comparator sorts, and records the `O(n log n) -> O(n log k)` cost
model.

Language contract: see [`language-support.md`](./language-support.md).

---

## `repeated_collection_traversal`

Detects at least three pure terminal traversals over the same unmodified
collection in one function. Direct call arguments and callbacks with unknown
calls or mutation are rejected; aliases participate in the mutation proof.
The retained control tree selects the maximum feasible path through
conditional and match/switch branches, so traversals in mutually exclusive
arms are not added together.

Language contract: see [`language-support.md`](./language-support.md).

---

## `hot_path_linear_membership`

Detects `contains`/`includes` membership on a proven linear collection
inside a loop when the lookup needle depends on that loop. Local bounded
collections, unknown collection shapes, invariant lookups, mutation, and
set/map inputs are excluded.

Evidence includes collection shape and cardinality, the loop-derived needle,
exact loop and lookup spans, zero mutation count, and the `O(m * n)` model.

Language contract: see [`language-support.md`](./language-support.md).

---

## `n_plus_one_query_in_loop` (full-only)

Detects loop-dependent read queries through known database or repository
receivers. The query arguments must reference the loop binding.

A query method is one of the fixed query names (`query`, `query_row`,
`fetch_one`, `find_one`, `find_unique`, ...) or a read of one record by key:
`get`, `find`, `load`, or `fetch`, then `by`, ending in `id`, `uuid`, or `key`
(`findById`, `getObjectByUuid`, `findUserById`), but not a lookup that writes
when the record is missing (`getOrCreateById`). A SQL runner (DB-API
`execute`, JDBC `executeQuery`) is a read query whatever its receiver is
called when its first argument is a string literal starting with `SELECT`
(`cur.execute("SELECT .. WHERE uid = ?", (user.id,))`): the SQL proves the
store. `executemany` and statements other than `SELECT` are not. A receiver
is proven when its name reads as a store (`repo`, `repository`, `database`,
`connection`, `session`) or contains the word `db` or `conn` (`db`, `userDb`,
`db_conn`, but not `feedback` or `dbg`); when it is declared with a store
type (repository, database, connection, session, query client or manager,
persistence or entity manager, Go `sql.DB`/`Tx`/`Conn`) as a parameter or
local of the function, including
a Java try-with-resources local or a `var` bound to `new` of such a type, or
as a Java field; or when it is a JavaScript module-level binding imported
from a module whose export queries directly (`redbean-node`, `knex`). Write methods,
batch APIs, preloading/eager-loading calls, and queries independent of the loop
are semantic negatives, and so is a query in a loop that walks batches, pages,
or windows (see `per_item_external_io`).

Evidence includes the proven receiver, query method and arguments, loop
binding, exact spans, database-read effect, and query cost model.

Language contract: see [`language-support.md`](./language-support.md).

---

## `loop_carried_copy_accumulation` (full-only)

Detects a proven linear accumulator initialized empty before a loop and rebuilt
once per iteration from its previous value plus the loop binding. The owner is
limited to grammar-specific copy forms and rejects accumulation into the
iterated collection, multiple writes, nonsequence receivers, and expressions
that do not carry both prior state and the loop value.

Evidence includes the accumulator and collection kind, binding, loop and
assignment spans, and a copy/allocation loop-product cost expression.

Language contract: see [`language-support.md`](./language-support.md).

---

## `invariant_collection_copy_in_loop` (full-only)

Detects a Rust primitive sequence cloned inside a loop only to read its length.
The collection must be scalable, invariant, distinct from the iterated
collection, and free of mutation in the loop. Other clone uses and unresolved
element ownership fail closed.

Evidence includes the collection kind, loop, copy, and terminal spans, and the
copy/allocation loop-product cost expression.

Language contract: see [`language-support.md`](./language-support.md).

---

## `repeated_recursive_subproblem` (full-only)

Detects at least two distinct decreasing self-calls for the same parameter on
one feasible control path. Memoization signals, mutually exclusive recursive
branches, nondecreasing arguments, and unresolved callees are excluded.

Evidence includes the parameter, function and call spans, normalized arguments,
branch count, and repeated-call composition with its memoization confidence
boundary.

Language contract: see [`language-support.md`](./language-support.md).

---

## `nested_cross_collection_traversal` (full-only)

Detects nested traversal over two distinct scalable linear inputs when work in
the inner region depends on both bindings. It rejects bounded, aliased,
loop-local, or unproved collections and suppresses loops whose output is
explicitly the Cartesian product required by the contract.

Evidence includes both collections, bindings, kinds, exact loop spans, the
non-Cartesian proof, and a product cost expression with a sum alternative.

Language contract: see [`language-support.md`](./language-support.md).

## `quadratic_front_removal` (full-only)

Reports removal of the first element of an array-backed sequence inside a
loop: Python `list.pop(0)`, JavaScript/TypeScript `array.shift()`, Java
`ArrayList.remove(0)`, and Rust `Vec::remove(0)`. Each removal shifts every
remaining element, so draining `n` items costs O(n^2); a deque
(`collections.deque`, `ArrayDeque`, `VecDeque`) or an advancing index makes it
O(n).

Algorithm: an `AlgorithmicIndex` fact. The receiver must be a local of the
function whose declaration or assignment shows an array: a list literal or
`list(..)`/`list[..]` annotation, an array literal or `T[]`, `new ArrayList`,
or `Vec`/`vec!`. A deque, linked list, or queue type, a field receiver, a
parameter typed only as `List<T>` (possibly a `LinkedList`), a receiver
declared inside the loop, and a loop over a small literal range (`range(3)`)
are excluded. Go is not applicable: `xs = xs[1:]` reslices without copying.

## `catastrophic_backtracking_regex` (full-only)

Reports a regex literal whose pattern alone admits exponential backtracking
(ReDoS): an unbounded repetition whose body can match the same text in more
than one way, such as `(a+)+`, `(\w+\s?)*`, or the string-literal pattern
`"(?:[^"\\]+|\\.)*"`.

Algorithm: patterns come from JavaScript/TypeScript regex literals and
`RegExp("..")`, Python `re`/`regex` calls (raw and escaped strings), and Java
`Pattern.compile`/`matches` and the `String` regex methods. A small parser
reads groups, classes, escapes, lookarounds, inline flags, and quantifiers. A
repetition is ambiguous when its body can match a variable length and at most
one of its consuming parts is mandatory, itself a variable-length repetition;
a body with a second mandatory part (`(?:,\s*\w+)*`) splits its input one
way only. Possessive quantifiers, atomic groups, and bounded repetitions are
not reported, and a pattern the parser cannot read fails closed. Go's
`regexp` and Rust's `regex` are linear-time automata, so those cells are not
applicable.

