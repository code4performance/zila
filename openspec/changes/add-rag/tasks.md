## 1. Library

- [ ] 1.1 Register `rag` in the library tables of `src/codegen_x64.rs` (rejected in the bare-metal build) and implement `chunk` with `Size` and `Paragraph`, overlap, boundary preference, UTF-8 safety and invalid options. Verify: `tests/rag.rs` size/overlap, multibyte, paragraph merge, and the coverage property over several generated texts; the kernel build rejects `use rag` naming the library.
- [ ] 1.2 `Embedder`, `VectorStore`, `Item`, `Hit` and the memory store (normalized contiguous storage, cosine, filter, tie order, dimension check, zero-vector rejection) with `save`/`load_store` and checksum. Verify: `tests/rag.rs` ranking, filter, persist-equals-original, truncated file error, wrong dimension.
- [ ] 1.3 `rag.index` (batching, id scheme, replace and delete of old chunks, all-or-nothing on embedder failure) and `rag.retrieve` (`k`, `min_score`). Verify: `tests/rag.rs` with a deterministic fake embedder: a shrinking document, a mid-way failure, a user-defined embedder type.
- [ ] 1.4 pgvector store: `pg_store`, `migrate`, upsert in one transaction, cosine search, identifier validation. Verify: `tests/rag.rs` identifier test (no statement sent for a bad name) and a SQL-text snapshot with a fake driver; with `BRASA_TEST_POSTGRES` set, hit order equals the memory store's for the same data on the Docker server.
- [ ] 1.5 `rag.answer` (numbered delimited sources, per-call random delimiter checked against chunk text, `max_context_chars` by whole chunks, empty retrieval without a model call, usage) and `rag.search_tool`. Verify: `tests/rag.rs` with `add-ai-client`'s fake model server: sources returned, nothing found sends no request, the budget drops lowest-ranked whole chunks, a chunk containing the delimiter candidate gets a different delimiter, and the tool used by `client.run` returns text with the source.

## 2. Example and documentation

- [ ] 2.1 `examples/rag.br`: index a folder of `.md` files into the memory store (saved to disk) or pgvector, then answer questions with `rag.answer` and print sources. Verify: builds and passes `brasa fmt --check`; a test runs it end to end on three small files with the fake embedder and fake model.
- [ ] 2.2 RAG section in `docs/ai.html` (chunking, stores, index, retrieve, answer, tuning, the injection and character-budget notes), updates to `docs/stdlib.html`, `docs/native.html`, navigation and `README.md`. Verify: `cargo test --test docs` passes.
- [ ] 2.3 Windows and Linux parity for the `rag` fixtures in `tests/linux.rs`. Verify: `cargo test --test linux` passes (leave `tests/async.rs` out of routine runs).
- [ ] 2.4 `openspec validate add-rag --strict` passes and full `cargo test` (without `tests/async.rs`) is green.
