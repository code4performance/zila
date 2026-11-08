## Context

`add-ai-client` provides `ai.Client`, `client.run`, `ai.Tool` and `client.embedder(model)` (a value with `embed(inputs: []text) -> [][]float or error`). `add-db-drivers` provides `db.Conn`, the PostgreSQL driver with pgvector values, `crypto.random` and the Docker environment with `items`-style vector tables. This change is plain Zila Lang and has no protocol of its own.

## Depends on

`add-ai-client` and `add-db-drivers`. The memory store and everything except the pgvector store only need `add-ai-client`.

## Goals / Non-Goals

**Goals:** small composable pieces that work with any embedder and any store; correct, testable chunking and ranking; answers that return their sources; safe handling of retrieved text.

**Non-Goals:** the proposal's non-goals.

## Decisions

1. **Two interfaces, any implementation.** `Embedder` and `VectorStore` are interfaces so `ai`'s embedder, the memory store, the pgvector store and user types are interchangeable. The language lets an interface bound a type parameter but not be the type of a value, so `index`, `retrieve`, `answer` and `search_tool` are generic (`index<S: VectorStore, E: Embedder>(store, embedder, ...)`), and `ai`'s embedder satisfies `Embedder` structurally across packages. `VectorStore` has `delete_source(source)` (everything indexed from one document) because re-indexing replaces a document's chunks without knowing their ids. `rag` imports `ai` only for `Tool`, `Client` and `Usage`.
2. **Memory store.** Vectors are normalized at insertion and kept in one contiguous `[]float`, so cosine similarity is a dot product; search scans linearly with a bounded heap and breaks ties by insertion order. Honest range: a few hundred thousand chunks; beyond that use pgvector. Persistence is a small versioned binary file with a checksum so truncation is detected and a partly loaded store is impossible.
3. **pgvector store.** Uses the `<=>` operator, converts distance to the shared `score` convention, upserts by id, and `migrate()` creates the table and an optional `hnsw` or `ivfflat` index using the extension's documented SQL (checked against pgvector's documentation for the version in the Docker image when implemented). Table names cannot be parameters, so they are validated against a plain-identifier pattern before any SQL is built.
4. **Chunking.** Window of `max_chars` with `overlap`, cut at the last paragraph, then sentence, then word boundary inside the window; never inside a UTF-8 sequence; the coverage property (all non-whitespace text appears in some chunk) is tested over generated texts.
5. **Re-indexing is replace-by-document.** Chunk ids are `<document id>#<index>`; indexing a document first embeds everything, then replaces its chunks in one step (in one transaction for pgvector), so a failing embedder never leaves half a document.
6. **Prompt assembly treats chunks as untrusted data.** Sources go inside numbered blocks with a per-call delimiter made with `crypto.random` and checked not to occur in any chunk; the system text says the blocks are data, to cite as `[n]` and to say when the answer is not in them. This reduces prompt injection, it does not remove it, and the docs say so. With no retrieved chunk, `answer` returns "no sources" without a model call.
7. **Context budget.** `max_context_chars` drops whole lowest-ranked chunks, never cuts one; token counts are not estimated (no tokenizer), characters are the unit and the docs say why.
8. **Tests without the world.** A deterministic fake embedder (hash of words into a small vector) and `add-ai-client`'s fake model server; the pgvector test against the Docker server runs only when `ZILA_TEST_POSTGRES` is set.

## What changed while implementing

- The pgvector store reads results through `conn.rows` and its cells instead of `db.query(T, ...)`: generated functions are expanded in a program's functions, not in a library's, and the four columns are fixed anyway.
- `search_tool` builds its `ai.Tool` by hand (the schema is one fixed object) for the same reason.
- `Hit` carries the item's id, text and metadata rather than the whole item with its vector.
- `answer` takes the model name as an argument, like `ai`'s `request`.
- The context budget counts characters. A chunk longer than the whole budget is left out.

## Risks / Trade-offs

- **Quality depends on the embedding model and chunk sizes, which this change cannot test.** → Defaults are conventional and every parameter is exposed; the example shows tuning knobs.
- **Prompt injection from stored documents.** → Delimiting and docs; users are told to index trusted content or to run the agent with least-privilege tools.
- **Linear scan.** → Documented range; interface lets pgvector take over.
- **Character budget is not a token budget.** → Conservative default; the provider's `usage` is returned so callers can adjust.
