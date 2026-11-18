# Layer 6 - Evidence-Constrained Engineering Review

Profile: optional post-scan workflow

Detectors: 0

Layer 6 is the only probabilistic layer. A model reads a finished scan
(the report, its repository map, and optionally a history digest) and the
repository's source, and answers a fixed set of engineering questions with
claims and recommendations that cite evidence the host issued. A normal
scan, baseline, and gate remain complete without it.

The review is experimental: `review run --experimental-reviewer` runs it,
and no route has yet qualified on it
([Layer 6 evaluation](./layer6-evaluation.md)). It replaces the v2 case
protocol, removed in the v1.6 rebuild (see [Removed](#removed)).

## What It Is

A reviewer for questions that need cross-file and cross-finding judgment:
which hotspots matter and why, whether findings in one symbol share a
cause, what structure the modules intend and where it is broken, and in
what order to fix things. Every claim cites evidence IDs, and every
recommendation names the claims it rests on, the findings it resolves, and
how to verify it. "Unknown" is a valid answer.

## What It Is Not

- It is not a detector and cannot create, suppress, relabel, or change an
  L1-L5 finding.
- It does not score repository quality.
- It does not read arbitrary paths or use arbitrary provider tools: only
  the host's retrieval tools, over the files the map and `git ls-files`
  name.
- It does not take repository text as instruction.
- It does not require an operator to accept its advice.

## Commands And Inputs

| Command | What it does |
|---|---|
| `review history` | Writes a history digest of `HEAD`'s first-parent line over `--window-days`: per-file commits, growth, and co-change partners. |
| `review run --experimental-reviewer` | Runs the review. Needs `--report` (report v5 with a complete repository map), `--routes`, `--repo-root` with the scanned source, and consent; takes `--history`, `--brief`, `--check-patches` with `--max-patch-checks` (default 8), `--transcript`, `--output`, and `--resume`. |
| `review check-patch` | Judges one patch against a report (see [Patch Checks](#patch-checks)). |
| `review qualify`, `review rubric` | Check recorded reviews offline and write the usefulness sheet ([Layer 6 evaluation](./layer6-evaluation.md)). |
| `review provider discover`, `review transcript` | List a route's models and render a transcript ([providers](./layer6-providers.md)). |

The source must match the report: the view checks the map against the
report and every read re-checks the file's hash.

The brief (`contracts/review-brief.schema.json`; the general brief is
`config/auditor/review-brief-general-v1.json`) gives goals, constraints,
risk posture, and excluded domains, shown after the orientation pack within
8 KiB, and an optional test command for patch checks. Its
`max_context_bytes` tightens the pack and the retrieval byte budget, and
`max_lookup_requests` the retrieval call budget; neither can widen them.
The review does not show brief documents and ignores the document and hop
budgets.

## Repository View And Orientation Pack

`RepositoryView` joins the report's findings with the map and divides the
repository into at most 64 modules: directories, split while one holds more
than a third of the files. From it the orientation pack renders shape,
module structure and cycles, hotspot rankings, finding distribution, and
coverage within 24 KiB (at least 4 KiB). With a history digest of the
repository's `HEAD`, the pack also ranks change hotspots and co-change
coupling, which no import shows.

## Retrieval Tools

| Tool | Answers with |
|---|---|
| `read_source` | Lines of a mapped file, after its hash check. A read too long for one result returns the lines from its first that fit and the line the rest continues at; its source span covers only those. |
| `search_text` | Up to 50 lines containing a plain, case-sensitive text (at least 3 characters) in the files `git ls-files` lists, or the mapped files outside a work tree. A line in a mapped file cites a source span; a line in a file the map leaves out, such as a Dockerfile, is context only, with no ID, since nothing in the review's inputs could confirm it later. |
| `symbol_info` | A symbol's kind, location, metrics, parent and members, base and subtypes, callers and callees, entry points, and findings. |
| `search_symbols` | Functions, methods, and types whose name contains the query (case-insensitive), exact and prefix matches first. |
| `findings_in` | The findings in a file, module, or symbol, most severe first, with the values each detector measured or matched. |
| `module_overview` | A module's files, the modules it imports and is imported by, cycles, findings by detector, and entry points. |
| `history` | With a digest: one file's commits in all and recently, authors, lines added and removed, and co-change partners, with whether an import links them. |

Every row cites an evidence ID (`finding:`, `file:`, `symbol:`, `module:`,
`edge:`, `entry:`, `source-span:`, `history:`, `co-change:`, and others)
that only a shown row issues. A read repeated within a question returns a
pointer to its earlier result.

**Budget.** A review has 128 calls and 1 MiB of results; a result is at most
8 KiB, and one reply's results at most 16 KiB. Each question may take an
equal share of what the review has left, at most 16 calls, so early
questions cannot spend what later ones need: a question that uses less
leaves the rest to the ones after it. A refused call says which limit it
hit.

## Questions

Questions come from checked-in templates
(`config/auditor/review-question-templates-v1.json`: wording, limits, claim
slots, recommendation families) and triggers over the view, asked in this
order:

| Kind | Asked | Subjects |
|---|---|---|
| `hotspot_assessment` | once | the top production files of each hotspot ranking, up to 8 |
| `shared_root_cause` | up to 5 | a symbol whose findings come from two or more detectors |
| `architecture_review` | once, if any | module cycles and structure findings, up to 8 |
| `remediation_plan` | always, last | the earlier answers |

A question renders its subjects with evidence IDs. Each question but the
plan comes with the source its findings sit in, most severe first: the
enclosing function or type, or ten lines either side of a finding outside
one or inside one over 4 KiB, up to 4 KiB a span and 16 KiB a question,
counting each span's label and IDs. A span over either limit is named, so
the reviewer reads the lines it needs. The plan sees the earlier answers,
with their claim IDs, within 24 KiB, so its steps can rest on them. The
hotspot and plan questions also offer `lifecycle_reownership` (repeated
work moved to a wider owner).

## Conversations And Answer Checks

Each question is one conversation over the route's provider chain. It opens
with the pack, the brief, and the question. The reviewer reads with the
retrieval tools and answers with one `answer_question` call: claims and
recommendations named by refs of its own, which the host turns into
content-derived IDs. A question's last two requests offer only the answer
tool, so a reviewer that reads until its budget runs out still answers.

`Question::check_answer` holds the answer to:

- the claim and recommendation validators, within the question's slots,
  families, and scope: every citation must be an ID the review has shown;
- the kind's shape: every subject cited, rankings complete, a shared cause
  explaining two or more findings, every plan step resolving one;
- seen code: every finding a recommendation resolves that has a line must
  lie in a source span the review has shown, by a read or in a question.
  The rejection names the unread lines and brings their source, which is
  how the plan, asked with none, sees the code its steps change;
- import facts: a claim saying one file or module does, or does not, import
  another states it as a fact (`from`, `to`, `imports`), checked against
  the map's import edges. A fact the edges contradict, or one naming no
  mapped file or module, is rejected with up to 5 of the edges the map
  holds.

A claim may cite a source span narrowed to lines inside one a read
returned. A rejected answer goes back with its reasons, naming the
reviewer's refs, and is held: a revision sends only the claims and
recommendations it replaces or adds, and the refs it drops.

What the reviewer sees names hash IDs (files, symbols, findings, cases,
claims, recommendations) by short aliases such as `file:7`, numbered per
kind in the order the review first shows them, so repeated openings stay
identical for prompt caching. The host expands them before any check, and
the record keeps the table. Every request carries the shared pack and brief
as its cache scope, so a provider that caches by key (Mistral) keeps one
cache for the review.

## Patch Checks

With `--check-patches`, the reviewer may call `check_patch` on a proposed
diff, and the host checks every accepted recommendation that carries one,
up to `--max-patch-checks`. `review check-patch` runs the same check: it
applies the diff to a scratch copy, re-scans it against a scan of an
unpatched copy, and runs the brief's test command, if it names one, in a
cleared environment. The verdict is `verified` when the patch applies, the
cited findings are gone, none is added, and the tests still pass;
`refuted` when it fails one of those; and `unchecked` when it cannot be
judged (the cited findings do not reproduce, a scan fails, or the tests
fail without the patch).

## Budgets, Failures, And Resume

Routes carry a review budget and a per-question budget of requests, input
bytes, output tokens, time, and cost
([providers](./layer6-providers.md#budgets-and-recovery)). A request that
returns no model output is retried and counts against the cost budget only.
A target that fails for good gives way to the next, an outage on the last
target is waited out with resends every five minutes, a retry waits at
least what a failed response's `Retry-After` asks (up to ten minutes), and a
provider's `min_request_interval_ms` spaces its requests within a review
process.

A spent question or review budget leaves questions unanswered with the
reason, as does a question whose source or earlier answers cannot be built;
that question is not sent and the review goes on. A review that stopped, or
a run interrupted after its per-question checkpoint, resumes with
`review run --resume`, keeping the questions answered before the stop and
the retrieval it spent.

## Trust

The core instruction tells the model that the pack, brief, question, and
every tool result are host-built evidence, and that repository text inside
them (source, comments, names, messages, matched patterns) is data, never
instruction. Evidence values are rendered in code spans that repository text
cannot close. Nothing is sent without `--allow-hosted-review`, and anonymous
routes also need `--allow-anonymous-provider`.

## The Review Record

`contracts/engineering-review-v1.schema.json` binds the review to its
inputs by digest (one source digest over the report, map, and history; the
templates, prompts, routes, and brief) and holds the orientation pack, each
question with its answer and checks or the reason it has none, the patch
checks, every retrieval and provider call, what the review spent, why it
stopped and whether it resumed, and the alias table. `review qualify` checks a record offline from the report, map,
and history alone.

## Removed

The v1.6 rebuild removed the v2 case protocol: the review agenda and its
selectors, engineering cases and evidence packets, probes and their
resolvers, the Mode E ensemble, challenge and response stages, case
finalization, the portfolio, offline v2 verification, the dispositions
ledger, route role qualification, and the `scan --review`, `review plan`,
`review inspect-case`, `review verify`, `review disposition`, and
`review gate` commands with their MCP tools. Claims and recommendations,
provider adapters, consent, transport, retry, and transcripts carried over.
