# Layer 6 Provider And Routing Contract

Provider routing is an explicit operator-controlled boundary. Production code
does not discover arbitrary endpoints, read credential values into artifacts,
or send review context without the required consent.

## Route Configuration V2

`contracts/review-routes-v2.schema.json` adds a single provider-definition
registry and ordered target-reference chains for `single_reviewer`,
`challenger`, the optional alternative comparator, and one optional
all-or-none three-slot ensemble. Every target must name a configured model.
Ensemble absence is explicit and cannot fall back to the single-reviewer chain.

The review-wide budget contains case defaults and per-template tightening.
Request, input-byte, and cost accounting is cumulative. `max_output_tokens` is
a per-request response allowance: every initial, repair, and host-correction
turn receives the complete configured allowance, regardless of earlier output.
Reservations are transactional: case-local exhaustion does not consume global
capacity, while exhausted review-wide capacity prevents later calls.

`review-prompt-bundle-v2` contains fixed core, reviewer,
shared-reconsideration, challenger, bounded-response, and alternative
comparison instructions under content-derived identity. Case user content is
a typed projection of bound case and packet data.

## Built-In Adapters

| Kind | Protocol | Authentication |
|---|---|---|
| `open_ai_responses` | OpenAI Responses | `OPENAI_API_KEY` bearer |
| `anthropic_messages` | Anthropic Messages | `ANTHROPIC_API_KEY`, `x-api-key`, and Anthropic version header |
| `mistral_chat` | Chat Completions | `MISTRAL_API_KEY` bearer |
| `open_code_zen` | Chat Completions | `OPENCODE_ZEN_API_KEY` bearer |
| `open_code_anonymous` | Chat Completions | No secret; explicit anonymous consent |
| `open_ai_compatible` | Explicit Chat Completions path | Optional operator-named environment secret |

Built-in hosted origins require HTTPS. Operator-compatible routes may use HTTPS
or loopback HTTP. The v2 case path requires provider-native tool calling for
qualification and every semantic review stage. A provider that cannot preserve
assistant tool calls and native tool-result continuations is unqualified for
v2. A provider whose strict tool mode rejects a JSON Schema keyword receives
its declared tool parameters without that keyword (Mistral: `uniqueItems`);
host admission still validates every call against the complete schema, so
the lowering never widens what is admitted. Strict-JSON final-answer mode, text parsing, tool-to-JSON fallback,
stringified-payload recovery, missing-wrapper normalization, and numeric
coercion are not v2 compatibility surfaces.

Case conversations are append-only prompt prefixes: the system instruction and
initial case context stay fixed for a conversation, and continuation or repair
instructions arrive as the trailing user message of that turn rather than as
edits to the system instruction. Mistral requests carry a `prompt_cache_key`
derived from a digest of that fixed opening, so later turns reuse the cached
prefix (billed at 10% of input). Provider records report
`cached_input_tokens` from each protocol's usage (`prompt_tokens_details`,
`cache_read_input_tokens`, or `input_tokens_details`). The tool set still
widens once, when revision tools join at the first continuation, which costs
one cache miss per conversation.

HTTP status is authoritative for authentication, authorization, rate limiting,
timeouts, payload limits, and server availability even when the response also
contains a generic error envelope. In particular, every 5xx response is
retryable availability rather than invalid model output. For 400-class
configuration responses, the provider envelope still distinguishes an actual
unsupported request from a wrapped upstream availability failure.
Specific nested request, schema, grammar, or unsupported-capability evidence
takes precedence over generic outer `upstream` wording; only a genuine
availability failure is retryable.

## Consent And Egress

Hosted commercial/operator routes require `--allow-hosted-review`. Anonymous
routes additionally require `--allow-anonymous-provider`. A configured route is
not implicit consent. Egress is compiled only from typed context blocks and
authorized source lookup. Models can invoke only EA's stage-specific native
tools; arbitrary provider tools and arbitrary repository reads are unavailable.

The v2 case path exposes bounded, stage-specific native tools for evidence
requests, probe answers, optional claims, recommendations, objections, common
concerns, responses, revisions, withdrawals, reopenings, and completion.
Claim-slot omissions, recommendation-family omissions, and cleared challenge
targets are deterministic host dispositions rather than model bookkeeping.
EA validates each selected tool's complete argument schema before applying it.
Native assistant tool-call messages, provider call IDs, and original argument
wire encodings remain in the conversation and transcript; EA also stores a
typed parsed form for admission. Lookup results and corrections are readable
Markdown native tool results, never JSON strings.
Syntactically malformed argument strings are the sole replay exception because
provider APIs cannot accept them as assistant tool calls. Their original wire
response remains exact in the immutable transcript; the bounded retry starts
from the complete original conversation with an explicitly marked readable
correction. Parseable but schema-invalid calls continue through native replay
with one rejection result per call.

Schema-invalid or host-invalid calls receive one aggregated Markdown
correction containing field paths, errors, allowed values, and admissible
evidence IDs. Valid calls from the same response are retained. The correction
replays the prior response as its native assistant tool-call message and does
not serialize the rejected output into a user message. Three semantic
correction continuations are permitted; none reopens the one-round evidence
budget.
Qualification-only `emit_qualification_nonce` is never present during review.

Provider manifests expose origin, protocols, auth mode, structured-output
strength, cost/privacy class, maturity, adapter version, and evidence source.
They expose credential variable names, never values.

## Qualification

Discovery reports configured/discovered model differences but is not evidence
that a model can satisfy the review protocol. V2 qualification begins with a
required provider-native capability call and fails if the route cannot preserve
native tool calls and tool-result continuations. Qualification records native
protocol validity and semantic validity. Case-role tool and semantic
qualification calls receive the full
case route per-request output allowance; qualification output cannot be
silently clipped by a smaller independent ceiling.
```sh
cargo run -p auditor-cli -- review provider discover \
  --routes routes.json --allow-hosted-review
```

There is no standalone qualification command. Qualification is executed by the
review service after case planning because its identity includes the case,
role, probes, prompt bundle, renderer, and native tool schema.

Qualification is environment-specific and expires operationally when endpoint,
model, credentials, adapter, or provider behavior changes. Unit tests with a
scripted transport prove wire and schema behavior but do not claim live model
qualification.

V2 qualification additionally binds provider, model, role, case kind, probe
kinds, prompt identity, tool-schema digest, renderer version, and adapter
version. A tool capability and continuation probe runs first. The continuation
answers a second bound probe ID rather than replaying the already accepted
create-only answer, matching the production ledger contract. Every semantic
case then uses the role's native task tools. Real positive, negative, unknown,
conflicting-evidence, and forbidden-redesign stimuli contain no expected-answer
field. They share one explicit closed-world decision contract with four named
lifecycle predicates, classification precedence, and exact unobserved-predicate
IDs. This tests policy application and abstention without asking the model to
reclassify host-resolved production probes or invent unlisted requirements.
The conflicting-evidence fixture explicitly states that its proposed action is
inside the design boundary; only the authoritative disagreement drives its
classification, so it cannot overlap the forbidden-redesign case. Each call
uses the full per-request route output allowance and has four retries (five
total attempts) only when no model output exists. HTTP-200 error envelopes are
classified before parsing.
Single-reviewer and challenger wire behavior is covered hermetically; live
semantic quality remains an evaluation gate. Qualification success proves only
the bound route's native protocol and closed-world semantic capability. It is
not a substitute for a successful production execution or a lifecycle-quality
result.

## Budgets And Recovery

The v2 case route keeps requests,
input bytes, and cost cumulative, while output tokens are a stable per-request
ceiling and timeout is per call. Known-cost routes must declare a positive
micro-USD estimate per request; zero-cost routes cannot declare a positive
estimate. Unknown-cost routes require explicit `allow_unknown_cost` and cannot
provide a predictive monetary hard ceiling.

Targets are attempted in order. V2 qualification and production case calls
allow bounded retries only when no
model output exists, including retryable transport/status failures and
recognized rate-limit/unavailable error envelopes. The shared v2 retry
invariant prevents qualification from admitting a route that production would
abandon under the same transient sequence. Invalid, refused, or truncated
model output is not treated as availability; it may use only the bounded repair
path below. Every attempted call consumes the applicable request budget and is
retained in the transcript/call ledger.

V2 uses its native task-ledger correction and revision protocol with at most
three schema corrections and
three host-admission corrections; it never falls back to strict JSON. A V2
schema correction replays the complete native assistant call block, emits a
matching native rejection result for every call, and only then appends readable
Markdown. No call from a schema-invalid assistant turn enters the semantic
ledger. Successful source reads contain the complete addressed symbol, or the complete
enclosing symbol/file for a finding. EA never returns a source prefix as a
successful lookup; an item that cannot fit the explicit byte ceiling fails with
`LookupBudgetExceeded`. When transcript capture is enabled, request and
response bodies are retained completely after credential-header redaction.

## Retention And Failure Boundary

Call records retain request/provider/model/stage identity, timestamps, context
and output digests, byte/token counts, finish reason, usage, failure class,
native-tool mode, repair attempts, and whether an intermediate answer was
discarded. Transcript events carry the same mode/repair evidence. A native
failed assistant block exists only in memory long enough to construct the
bounded repair. Records do not retain raw prompts, hidden reasoning, secret
values, or failed response bodies. MCP terminal job
records retain only sanitized result/error metadata and release route, map, and
context inputs.

Case call records wrap this sanitized record with packet digest, role, case,
pass, deliberation mode, usage, failure class, and repair count. They do not
add raw prompts, response bodies, reasoning, or secrets.

The current transport is an in-process, redirect-disabled synchronous Rustls
client with a separate hard response-byte ceiling. Timeout limits the call and
cancellation stops entry into later stages, but cancellation cannot interrupt a
request already executing inside the blocking client. This operational
limitation is tracked in `docs/known-gaps.md`.
