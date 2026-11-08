## Why

Brasa has TLS, JSON with comptime reflection and cheap tasks, which is what talking to a language model needs, but there is no client for an LLM API. With one, a program can use a model as a typed function (`ask(Invoice, ...)`), stream answers and give the model Brasa functions as tools. This is the base of `add-mcp` (the same tools served over MCP) and `add-rag` (retrieval and grounded answers). Database tools for agents come from `add-db-drivers`.

## What Changes

- **`use ai`**: a client for the Anthropic Messages API and for OpenAI-compatible servers (including local ones such as Ollama over plain `http`): `ai.chat`, streaming through server-sent events, retries with back-off, token accounting, `key_from_env`.
- **Typed output**: `ask(T, req)` returns a value of the user's own type; the JSON Schema is derived from `T` by comptime reflection (`#[desc("...")]`, `#[json(...)]`), with one repair round.
- **Tools and the agent loop**: `ai.tool(I, name, description, handler)` defines a tool whose input is a record type; `client.run(req, tools)` runs the loop with concurrent tool calls, a step limit and errors passed back to the model.
- **Embeddings** for OpenAI-compatible servers, and `client.embedder(model)` for `add-rag`.
- **Streaming body in the web client**: `web.open(request)` in `std/web.br` (head, then chunks of an unbounded body), needed by streaming.
- **Environment access**: `sys.env(name)` and a task-friendly standard-input reader if either is missing.
- **Example** `examples/ai.br`; documentation page `docs/ai.html` (chat, typed output, tools, embeddings, safety notes) and stdlib/navigation updates.

Non-goals: fine-tuning or hosting models; image and audio inputs; tokenizers (counts come from the provider's `usage`); agent frameworks beyond the loop (planning, memory, multi-agent); caching of model calls; provider-native structured-output modes (the portable schema-plus-repair is used).

## Capabilities

### New Capabilities

- `ai-client`: model API client with streaming, typed output, tools, agent loop and embeddings.

### Modified Capabilities

None. (`web.open` is an addition to the library of the pending `add-web`; it is specified there as part of that change's client, see design.md.)

## Impact

New `std/ai.br`; `std/web.br` (`web.open`, once `add-web` has landed); `std/sys*.br` and `std/bufio.br` if environment or stdin helpers are missing; `src/codegen_x64.rs` (library table, kernel rejection); `examples/ai.br`; `tests/ai.rs`, `tests/web.rs`, `tests/linux.rs`; `docs/ai.html` (new), `docs/stdlib.html`, `docs/native.html`, navigation, `README.md`. Depends on `add-web` (HTTP client) and on the archived `add-json`, `add-comptime-reflection`, `add-error-kind`, `remove-async-coloring`. `add-mcp` and `add-rag` depend on this change.
