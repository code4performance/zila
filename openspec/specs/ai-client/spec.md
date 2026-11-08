# ai-client Specification

## Purpose
Lets a Zila Lang program use a language model as a typed function: send messages, stream the answer, get values of its own types back and let the model call Zila Lang functions as tools.

## Requirements

### Requirement: Providers and credentials
`use ai` SHALL offer `ai.anthropic(key)` (Anthropic Messages API over HTTPS) and `ai.openai_compatible(base_url, key)` (the chat-completions and embeddings endpoints of OpenAI-style servers, including local servers over plain `http://`), each returning an `ai.Client`. `ai.key_from_env(name)` SHALL read a key from an environment variable and return an error naming the variable when it is unset or empty. The key SHALL be sent only in the request header, SHALL NOT appear in any error message, log line or `Debug` output, and `https://` endpoints SHALL be validated by `tls` with no way to turn validation off. The model is always named by the caller; the library has no default model.

#### Scenario: Missing key
- **WHEN** a program calls `ai.key_from_env("ANTHROPIC_API_KEY")` and the variable is unset
- **THEN** it gets an error whose message contains `ANTHROPIC_API_KEY`

#### Scenario: Key never leaks
- **WHEN** a request fails with an HTTP 401 and the program prints the error
- **THEN** the printed text does not contain the key

### Requirement: Chat
`client.chat(req)` SHALL send a request with `model`, `system` (optional), `messages` (a list of `ai.Message` with a role of `User` or `Assistant` and text content), `max_tokens` and optional `temperature` and `stop` and return `ai.Reply or error` with `text`, `stop_reason` (`End`, `MaxTokens`, `ToolUse`, `Other(text)`), `usage` (input and output token counts) and the tool calls the model asked for. A non-2xx answer SHALL be an error carrying the provider's message and the HTTP status, with kind `Unauthenticated` for 401, `PermissionDenied` for 403, `InvalidInput` for 400 and 422, `Unavailable` for 429 and 5xx, and `Timeout` for a request that exceeds `timeout` (default 120 s).

#### Scenario: Simple question
- **WHEN** a program sends one user message to a fake server that answers with a text block and usage numbers
- **THEN** the reply holds that text, the stop reason `End` and the usage numbers

#### Scenario: Error mapping
- **WHEN** the fake server answers 429 with a JSON error body
- **THEN** the call fails with kind `Unavailable` and a message containing the provider's text

### Requirement: Retries
The client SHALL retry a request that failed with a connection error, 429, 500, 502, 503, 504 or 529, at most `max_retries` times (default 2), waiting with exponential back-off with jitter and honoring a `retry-after` header, without blocking other tasks. It SHALL NOT retry other statuses, and SHALL NOT retry after part of a streamed answer has been delivered to the caller.

#### Scenario: Transient failure
- **WHEN** a fake server answers 503 once and then 200
- **THEN** the call succeeds and the server saw two requests

#### Scenario: Bad request is not retried
- **WHEN** the fake server answers 400
- **THEN** the call fails after one request

### Requirement: Streaming
`client.stream(req, on_text)` SHALL deliver the answer incrementally by calling `on_text(piece)` for each text fragment as it arrives (server-sent events over a chunked response) and return the same `ai.Reply` as `chat` at the end. A callback SHALL be able to stop the stream by returning an error, which closes the connection and is returned. The `stream` events of the provider that the library does not know SHALL be ignored, and an `error` event in the stream SHALL be returned as an error.

#### Scenario: Pieces arrive in order
- **WHEN** a fake server sends the text `Hel`, `lo` and `!` as three events with a pause between them
- **THEN** the callback is called three times in that order, the first call happening before the server sends the last event, and the final reply text is `Hello!`

#### Scenario: Caller stops
- **WHEN** the callback returns an error after the first piece
- **THEN** the call returns that error and the server's connection is closed

### Requirement: Typed output
`client.ask(T, req)` SHALL return a value of the caller's type `T` (a record, enum, list or optional the reflection of `use json` can describe) as `T or error`. The library SHALL derive a JSON Schema from `T` by comptime reflection (field names respecting `#[json("name")]`, `#[json(skip)]`, optionals as not required, enums as `enum` or `oneOf`, descriptions from `#[desc("...")]` on fields and types), instruct the model to answer with JSON that matches it, parse the answer with `json.parse(T, ...)` and, when parsing fails, retry once more with the error message appended to the conversation. A second failure SHALL be an error of kind `InvalidInput` that includes the model's raw answer.

#### Scenario: Record from text
- **WHEN** a program asks for `Invoice` (`number: text`, `total: float`, `paid: bool`) from a paragraph of text and the fake model answers with matching JSON
- **THEN** the result is an `Invoice` with those values

#### Scenario: Schema from the type
- **WHEN** `ai.schema(T)` is printed for a record with a `text`, an `int`, a `text?` and a `[]text` field, the first described with `#[desc("customer name")]`
- **THEN** it is an object schema with four properties, the description on the first, and `required` listing the three non-optional fields

#### Scenario: One repair
- **WHEN** the fake model first answers with text that is not valid JSON and then with valid JSON
- **THEN** the call succeeds and the server saw two requests, the second containing the parse error

### Requirement: Tools and the agent loop
`ai.tool(I, name, description, handler)` SHALL define a tool whose input is the record type `I` and whose handler is `func(I) -> text or error`; its JSON Schema SHALL come from `I` as for typed output. `client.run(req, tools)` SHALL run the agent loop: send the request, and while the model asks for tools, decode each call's arguments into `I`, call the handler, send the result (or the error message, marked as an error) back, and continue until the model stops asking, returning the final `ai.Reply` and the whole conversation. The loop SHALL stop with an error after `max_steps` model calls (default 10). Several tool calls in one model turn SHALL run concurrently as tasks and their results SHALL be returned in the order of the calls. A handler error SHALL NOT end the loop (the model sees it); arguments that do not match `I` SHALL be reported to the model as an error result without calling the handler. A tool name that is not registered SHALL be reported to the model as an error result.

#### Scenario: One tool round trip
- **WHEN** a fake model asks for `get_weather` with `{"city":"Recife"}` and then answers with text after receiving the result
- **THEN** the handler is called once with `city` equal to `Recife`, and `run` returns the final text

#### Scenario: Step limit
- **WHEN** a fake model asks for a tool in every turn and `max_steps` is 3
- **THEN** `run` returns an error after the third model call

#### Scenario: Bad arguments
- **WHEN** the model calls a tool with `{"city": 5}` for a `city: text` field
- **THEN** the handler is not called and the next request to the model contains an error result mentioning `city`

#### Scenario: Handler error is data
- **WHEN** a handler returns an error
- **THEN** the loop continues and the model receives the error message as the tool result

### Requirement: Embeddings
`client.embed(model, inputs)` SHALL return one `[]float` per input, in order, for providers with an embeddings endpoint (OpenAI-compatible servers), batching inputs up to `batch_size` (default 64) per request. For a provider without an embeddings endpoint (`ai.anthropic`) it SHALL return an error of kind `InvalidInput` that says so, rather than guess. An empty input list SHALL return an empty list without a request. The value returned by `client.embedder(model)` SHALL have the method `embed(inputs: []text) -> [][]float or error`, so that it satisfies the embedder interface of `use rag` without `ai` depending on it.

#### Scenario: Batches
- **WHEN** 150 inputs are embedded with the default batch size against a fake server
- **THEN** the server sees 3 requests and the result has 150 vectors in input order

#### Scenario: No endpoint
- **WHEN** a program calls `embed` on an `ai.anthropic` client
- **THEN** it gets an `InvalidInput` error saying the provider has no embeddings endpoint

### Requirement: Cooperative and cancellable
Every call SHALL wait without blocking other tasks, and a call SHALL return an error of kind `Timeout` when its total time exceeds `timeout`. Two or more agents or chats MAY run at the same time in different tasks on one client.

#### Scenario: Parallel chats
- **WHEN** 20 tasks each call `chat` on one client against a fake server that delays each answer 200 ms
- **THEN** all finish in well under 20 times 200 ms
