# Layer 6 Provider And Routing Contract

Provider routing is an explicit operator-controlled boundary. Production code
does not discover arbitrary endpoints, read credential values into artifacts,
or send review context without the required consent. What the review sends
and checks is in [Layer 6](./layer6-llm-review.md).

## Routes

`contracts/review-routes-v2.schema.json` (schema version 4) has:

- `providers`: each with an `id`, a `kind` (an adapter below),
  `endpoint_base`, the credential's environment variable (`api_key_env`),
  its `models`, cost and privacy classes, an optional
  `chat_completions_path`, `stream` (stream Chat Completions responses), and
  `min_request_interval_ms`;
- `single_reviewer`: the ordered chain of targets (provider and model) the
  review asks;
- `review_budget` and `question_budget`: requests, input bytes, output
  tokens, timeout, and cost. The question budget may tighten the review's
  but not widen it.

Requests, input bytes, and cost are cumulative; `max_output_tokens` is each
request's allowance and `timeout_ms` each call's. A request that returns no
model output counts against the cost budget only. A known-cost provider must
declare a positive per-request estimate in micro-USD and a zero-cost one
cannot; an unknown-cost route needs `allow_unknown_cost` and has no
predictive cost ceiling.

`review-prompt-bundle-v2` (schema 3) holds the core and reviewer
instructions under a content-derived ID.

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
or loopback HTTP. The review needs provider-native tool calls and tool-result
continuations. A provider whose strict tool mode rejects a JSON Schema
keyword receives its declared tool parameters without that keyword
(Mistral: `uniqueItems`); the host still validates every call against the
complete schema, so the lowering never widens what is accepted.

A conversation is an append-only prompt prefix: the system instruction and
the opening (pack, brief, question) stay fixed, and later instructions
arrive as trailing messages. Mistral requests carry a `prompt_cache_key`
from the review's shared pack and brief, so every question reuses one cached
prefix (billed at 10% of input). Provider records report
`cached_input_tokens` from each protocol's usage (`prompt_tokens_details`,
`cache_read_input_tokens`, or `input_tokens_details`).

HTTP status is authoritative for authentication, authorization, rate limiting,
timeouts, payload limits, and server availability even when the response also
contains a generic error envelope. In particular, every 5xx response is
retryable availability rather than invalid model output. For 400-class
configuration responses, the provider envelope still distinguishes an actual
unsupported request from a wrapped upstream availability failure.
Specific nested request, schema, grammar, or unsupported-capability evidence
takes precedence over generic outer `upstream` wording; only a genuine
availability failure is retryable. An unrecognized error body is classed by
its status.

## Consent And Egress

Hosted commercial and operator routes require `--allow-hosted-review`.
Anonymous routes also require `--allow-anonymous-provider`. A configured route
is not implicit consent. A model can call only the review's own tools (the
retrieval tools, `answer_question`, and with patch checks `check_patch`);
arbitrary provider tools and repository reads are unavailable. The host
validates each call's arguments against the tool's schema before it runs.
Tool results and rejections are Markdown, never JSON strings.

Provider manifests expose origin, protocols, auth mode, structured-output
strength, cost and privacy class, maturity, adapter version, and evidence
source. They expose credential variable names, never values.

## Discovery

```sh
cargo run -p auditor-cli -- review provider discover \
  --routes routes.json --allow-hosted-review
```

Discovery reports differences between configured and listed models. It is
not evidence that a model can do the review: a route qualifies only on the
recorded corpus ([Layer 6 evaluation](./layer6-evaluation.md)). Unit tests
with a scripted transport prove wire and schema behavior, not live quality.

## Budgets And Recovery

Targets are tried in order. A request that fails with no model output (a
transport failure, a retryable status, or a recognized rate-limit or
unavailable envelope) is retried four times on its target, waiting 15, 30,
60, and 120 s; the next target then takes over. The last target, with none
after it, waits out an outage with six resends five minutes apart. A gateway
timeout is retried twice, on any target, rather than waited out. A retry
waits at least what a failed response's `Retry-After` asks, in seconds, up
to ten minutes (a date is ignored). `min_request_interval_ms` is the least
time between the starts of two requests to a provider within one review
process, so for a requests-per-minute cap shared by `n` reviews running at
once it is about `60_000 * n / cap`.

A reply with no tool call, or one cut off at the token limit, is model
output: it is not retried, and the review tells the model to read and then
answer. Invalid tool arguments get a rejection result naming the error.
When transcript capture is enabled, request and response bodies are kept
complete after credential-header redaction.

## Retention And Failure Boundary

The review record keeps each call's question, request ID, target, retry
number, input bytes, usage, finish reason, and failure class with a short
detail. Provider call records keep request, provider, model, and stage
identity, timestamps, context and output digests, byte counts, finish
reason, usage, and failure class. Neither keeps raw prompts, hidden
reasoning, secret values, or failed response bodies; the transcript, when
asked for, keeps the redacted bodies.

The current transport is an in-process, redirect-disabled synchronous Rustls
client with a separate hard response-byte ceiling. Timeout limits the call, but
cancellation cannot interrupt a request already executing inside the blocking
client. This operational limitation is tracked in `docs/known-gaps.md`.
