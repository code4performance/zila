## Context

Plain Zila Lang on `web` (client), `json` and `reflect`. Reflection describes types (fields, variants, attributes with literal arguments) but not function signatures or comments, which shapes how tools are declared. `add-web` is planned and its `std/web.br` is not written; its client buffers whole bodies, so streaming needs one addition.

## Depends on

- `add-web`: `ai` calls model APIs through `web.request`. This change adds `web.stream(opts, method, url, headers, body, on_piece)`, which sends the request and hands each piece of a successful response's body (chunked, `Content-Length` or until close) to `on_piece`, which may return an error to stop. If `add-web` has landed, it is a small addition to `std/web.br`; if this change lands first, task 1.1 waits for it. `add-web` owns the rest of the client.
- Archived and available: `add-json`, `add-comptime-reflection`, `add-error-kind`, `remove-async-coloring`.

## Goals / Non-Goals

**Goals:** a typed, short API for chat, typed answers and tools; the same `Tool` value usable by a local agent and (in `add-mcp`) an MCP server; secrets never in errors or logs; every call cooperative and bounded in time and steps.

**Non-Goals:** the proposal's non-goals.

## Decisions

1. **Tools are record types, not functions.** `ai.tool(I, name, description, handler)` takes the input as a record `I`; the schema comes from `I`'s fields and attributes, descriptions from `#[desc("...")]` on fields (a plain attribute read through `attrs()`; no compiler change). A `#[tool]` attribute on a function would need reflection to expose parameters and comments; left for later because it only changes spelling. A `Tool` is a record `{name, description, schema: text, call: func(text) -> text or error}`; the typed wrapper decodes the JSON arguments with `json.parse(I, ...)` and calls the handler, so the agent loop, `add-mcp`'s server and its remote tools all deal with one untyped shape.
2. **JSON Schema by a `#[generated]` function.** `ai.schema(T)` is generated per type like `json.parse`: records become `object` with `properties` and `required`, optionals leave `required`, `[]T` is `array`, `Map<text, V>` an object with `additionalProperties`, enums without payloads `enum`, enums with payloads `oneOf` of one-key objects (the shape `json` decodes), integers `integer`, floats `number`. `#[json("x")]` and `#[json(skip)]` apply as in `json` so schema and decoder never disagree. A recursive type uses `$defs`/`$ref`. Types the schema cannot express (maps with non-text keys) are compile errors at the field through `reflect.fail`.
3. **Providers behind one record, two wire formats.** `ai.Client` holds `base_url`, headers, a `Wire` enum (`Anthropic`, `OpenAi`) and defaults; requests and replies convert between `ai`'s neutral records (`Message`, `Reply`, `ToolCall`) and private provider records via `json.encode`/`json.parse`, so no JSON is written by hand. Provider details (header names, the API version header, tool-use blocks, `stop_reason` values, streaming event names, embeddings endpoint shape) are checked against the provider documentation when implementing, not frozen here, and live in one block per provider. Unknown response fields are ignored (json ignores unknown keys).
4. **Streaming through `web.open`.** SSE over the chunked body: `event:`/`data:` lines, events split by a blank line, `data:` lines joined, comments ignored. Text deltas go to the callback; tool-call arguments arrive as partial JSON and are parsed only when the block ends. Retries stop once the first byte of a streamed answer was delivered, because the caller already saw output.
5. **Agent loop with concurrent tool calls.** One iteration is one model call; the calls in a reply run as `spawn`ed tasks joined in call order. A handler error is text for the model (`is_error`), not a loop failure; loop errors (network, step limit, repair failure) are Zila Lang errors. `max_steps` is the main guard against runaway cost; `Reply` carries cumulative `usage` and an optional `on_step` callback can return an error to stop (caller-side budgets).
6. **Typed output is a prompt plus a parse, with one repair.** `ask` puts the schema in the system prompt, asks for JSON only, strips a Markdown fence before parsing, and retries once with the parse error appended. Provider-native structured-output or forced-tool modes are an optimization not used here because they differ per provider; the repair loop is portable. A second failure returns the raw answer in the error.
7. **Credentials.** `key_from_env(name)` returns an error naming the variable, never the value. The key lives only in a header value inside the client; error construction goes through one function that cannot include request headers. `https` goes through `tls` with validation always on.
8. **Registration.** One portable file `std/ai.br` in the library tables; rejected in the bare-metal build with `net`.
9. **Tests without the world.** `tests/ai.rs` runs a fake model server written in Zila Lang on the library's own `net` (canned replies, scripted SSE with pauses, injected 429/503/401). Real calls run only when `ZILA_TEST_ANTHROPIC_KEY` or `ZILA_TEST_OPENAI_URL` is set, and print a skip otherwise.

## What changed while implementing

- `Request.temperature` is a plain `float` (below zero means "not set"): the language has optionals only for reference types. `ai.request(model, messages, max_tokens)` builds a request with the usual defaults.
- A `Tool` handler is a function `func(I) -> text or error`, and the generated `ai.tool`, `ai.ask` and `ai.schema` are free functions (a generated function cannot be a method). The program that uses `ask` or `tool` also writes `use json`.
- Recursive types in a schema are refused at build time (the schema generator stops at depth 8) instead of using `$defs`.
- Connection errors are retried but their kind is the transport's (for example `ConnectionRefused`); only HTTP statuses are mapped to the kinds of the spec.

## Risks / Trade-offs

- **Provider APIs and model names change.** → Wire details isolated per provider, unknown fields and events ignored, no default model, specs avoid provider field names.
- **Prompt injection through tool output and documents.** → The library never executes anything the model says except calling the tools the program registered; docs state that tool outputs are untrusted and tools should be least-privilege.
- **Runaway cost.** → `max_steps` default 10, `max_tokens` required, `on_step` hook, usage reported.
- **Streaming depends on `web.open`.** → Small, tested addition; if `add-web` is late, task 1.1 can ship a private minimal reader inside `ai` first and switch later.
- **Whole-body buffering for non-streamed calls** is bounded by the web client's `max_response` (default 16 MiB). → Streaming for long outputs.

## Open Questions

- Whether `on_step` should also receive tool results for logging (additive).
- Whether a `#[tool]` function attribute is worth a reflection extension once this is used.
