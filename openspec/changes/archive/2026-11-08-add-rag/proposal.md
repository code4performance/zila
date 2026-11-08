## Why

A model knows nothing about a program's own documents. Retrieval-augmented generation fixes that: split documents, embed the pieces, store the vectors, find the closest pieces to a question and ask the model to answer from them. The parts are small and independent of any one model or database, so they can be plain interfaces plus two stores: one in memory and one on PostgreSQL with pgvector (from `add-db-drivers`).

## What Changes

- **`use rag`**: `chunk` (by size with overlap, by paragraph; UTF-8 safe), the `Embedder` and `VectorStore` interfaces, `rag.index`, `rag.retrieve`, `rag.answer` (grounded answers with numbered sources, a context budget and untrusted-text delimiting), and `rag.search_tool` (the knowledge base as an `ai.Tool`).
- **Stores**: an in-memory store (exact cosine, metadata filter, save and load with a checksum) and a pgvector store over `db.Conn` (migrate, upsert, cosine search, identifier validation).
- **Example** `examples/rag.br` (index a folder of `.md` files, answer questions with sources) and a RAG section in `docs/ai.html`.

Non-goals: approximate indexes (HNSW/IVF) in the memory store (pgvector can create them); document loaders for PDF, HTML or Office formats; rerankers; hybrid keyword search; query rewriting; evaluation tooling.

## Capabilities

### New Capabilities

- `rag-library`: chunking, embedding, vector stores, retrieval and grounded answers.

### Modified Capabilities

None.

## Impact

New `std/rag.br`; `src/codegen_x64.rs` (library table, kernel rejection); `examples/rag.br`; `tests/rag.rs`, `tests/linux.rs`; `docs/ai.html`, `docs/stdlib.html`, `docs/native.html`, navigation, `README.md`. Depends on `add-ai-client` (`ai.Client`, `ai.Tool`, `client.embedder`) and `add-db-drivers` (`db.Conn`, `postgres`, `crypto.random` for the prompt delimiter, the Docker environment for the real pgvector test). It does not depend on `add-mcp`; a knowledge base can be served over MCP by registering `rag.search_tool` there.
