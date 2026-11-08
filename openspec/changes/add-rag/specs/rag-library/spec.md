## Purpose

Provides the parts of retrieval-augmented generation (splitting documents, embedding, storing and searching vectors, and answering with sources) as small composable pieces that work with any model client and any store.

## ADDED Requirements

### Requirement: Chunking
`rag.chunk(text, opts)` SHALL split text into `rag.Chunk` values (`text`, `start` and `end` byte offsets in the source, `index`). With `by: Size`, chunks SHALL have at most `max_chars` characters (default 1000) and consecutive chunks SHALL overlap by `overlap` characters (default 100, always less than `max_chars`), cutting at the last paragraph, then sentence, then word boundary inside the window when one exists, and never inside a UTF-8 sequence. With `by: Paragraph`, text SHALL be split at blank lines and short neighbors merged up to `max_chars`. Empty or whitespace-only chunks SHALL NOT be produced, and the chunks SHALL together cover all non-whitespace text of the source.

#### Scenario: Size with overlap
- **WHEN** a 2500-character text of words is chunked with `max_chars` 1000 and `overlap` 100
- **THEN** there are at least 3 chunks, none longer than 1000, and the last 100 characters of each chunk start the next one (to a word boundary)

#### Scenario: Multibyte text
- **WHEN** a text made only of the character `ã` repeated 3000 times is chunked
- **THEN** every chunk is valid UTF-8 and the chunks together contain all 3000 characters (counting overlap once)

#### Scenario: Invalid options
- **WHEN** `overlap` is not less than `max_chars`
- **THEN** `chunk` returns an `InvalidInput` error

### Requirement: Embedder and store interfaces
`use rag` SHALL define `rag.Embedder` (`embed(inputs: []text) -> [][]float or error`, plus the vector size it produces once known) and `rag.VectorStore` (`add(items: []Item)`, `search(vector, k, filter) -> []Hit`, `delete(ids)`, `count()`), where an `Item` holds an `id`, the vector, the text and `meta: Map<text, text>`, and a `Hit` holds the item and a `score` where larger means closer. The value returned by `ai`'s `client.embedder(model)` SHALL satisfy `rag.Embedder`, and any user type with those methods SHALL work with every function of the library. A store SHALL reject a vector whose size differs from the store's with an `InvalidInput` error.

#### Scenario: Custom embedder
- **WHEN** a program defines its own type with an `embed` method and passes it to `rag.index`
- **THEN** it works with no changes to the library

#### Scenario: Wrong dimension
- **WHEN** a store holding 3-dimensional vectors is given a 4-dimensional vector
- **THEN** `add` returns an `InvalidInput` error and the store is unchanged

### Requirement: In-memory store
`rag.memory_store()` SHALL keep items in memory and answer `search` by exact cosine similarity, returning the `k` best hits ordered by decreasing score with ties broken by insertion order, applying `filter` (a map that must match item metadata exactly) before ranking. `store.save(path)` and `rag.load_store(path)` SHALL write and read the store to a file such that a loaded store answers every search exactly as the original did; a corrupt or truncated file SHALL be an error, never a partly loaded store. A zero vector SHALL be rejected on `add`.

#### Scenario: Ranking
- **WHEN** a store holds the vectors (1,0), (0,1) and (0.9,0.1) and is searched with (1,0), k=2
- **THEN** the hits are the first and third items in that order

#### Scenario: Persist
- **WHEN** a store is saved, loaded and searched with the same query as before
- **THEN** the hits and scores are identical

#### Scenario: Filter
- **WHEN** a search carries the filter `{"source": "a.md"}`
- **THEN** only items whose metadata has that source are returned

### Requirement: pgvector store
`rag.pg_store(conn, table, dims)` SHALL keep items in a PostgreSQL table using `pgvector` through a `db.Conn` from `use postgres`: it SHALL create the table and a cosine index if they do not exist when asked (`store.migrate()`), insert with parameters (re-adding an existing `id` SHALL replace it), and search with the cosine-distance operator, converting distance to the same `score` convention as the memory store. The table name SHALL be checked against the pattern of a plain identifier (letters, digits and underscore, not starting with a digit) and any other name SHALL be an `InvalidInput` error, since identifiers cannot be bound as parameters.

#### Scenario: Same answers as memory
- **WHEN** the same items and queries run against the memory store and the pgvector store
- **THEN** the order of the hits is the same

#### Scenario: Bad table name
- **WHEN** a program passes the table name `items; drop table users`
- **THEN** `pg_store` returns an `InvalidInput` error and nothing is sent to the database

### Requirement: Indexing and retrieval
`rag.index(store, embedder, documents, opts)` SHALL chunk each document (`id`, `text`, metadata), embed the chunks in batches, and add them with ids `<document id>#<chunk index>` and metadata copied from the document plus `source`. Re-indexing a document SHALL replace its previous chunks and remove chunks that no longer exist. `rag.retrieve(store, embedder, question, opts)` SHALL embed the question and return the top `k` (default 4) hits at or above `min_score` (default none). A failing embedder SHALL make `index` return the error without leaving a half-indexed document.

#### Scenario: Reindex shrinks a document
- **WHEN** a document that had 5 chunks is re-indexed with text that makes 2 chunks
- **THEN** the store holds 2 chunks for it

#### Scenario: Embedder fails midway
- **WHEN** the embedder fails on the second batch of a document
- **THEN** `index` returns the error and the store holds no chunks of that document that were not there before

### Requirement: Grounded answers
`rag.answer(store, embedder, client, question, opts)` SHALL retrieve chunks, build a prompt that places each chunk in a numbered, delimited source block and instructs the model to answer only from the sources, to cite them as `[n]` and to say it does not know when they do not contain the answer, send it through `ai`, and return an `rag.Answer` with the text, the chunks used (with scores and metadata) and the token usage. Chunk text SHALL be passed as data inside delimiters, never as instructions to the library. When retrieval returns no chunk, the call SHALL return an answer saying no sources were found without calling the model. The total prompt SHALL respect `max_context_chars` (default 12000) by dropping the lowest-ranked chunks, never by cutting one in the middle.

#### Scenario: Sources are returned
- **WHEN** a fake model answers `The fee is 5 [1]` for a question whose best chunk is a document about fees
- **THEN** the `Answer` text is that text and `sources[0]` is the fee chunk with its metadata

#### Scenario: Nothing found
- **WHEN** the store is empty
- **THEN** the answer says no sources were found and the model server saw no request

#### Scenario: Context budget
- **WHEN** the top 4 chunks total more than `max_context_chars`
- **THEN** the prompt contains only whole chunks, starting from the best, that fit

### Requirement: Tool form
`rag.search_tool(store, embedder, name, description)` SHALL return an `ai.Tool` that takes a `query` and returns the top chunks as text with their sources, so that an agent, or an MCP server, can use the knowledge base as a tool.

#### Scenario: Used by an agent
- **WHEN** the tool is given to `client.run` and the fake model calls it with a query
- **THEN** the result text sent back to the model contains the best matching chunk and its source
