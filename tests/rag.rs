//! The `rag` library (spec `rag-library`): chunking, the in-memory and pgvector stores, indexing,
//! retrieval, grounded answers and the search tool, with a deterministic embedder written in the
//! test programs, a fake model server (as in `tests/ai.rs`) and a fake database driver. A real
//! pgvector table is used only when `ZILA_TEST_POSTGRES` names a server that already has the
//! `vector` extension; the table it makes is dropped at the end.
//!
//! JSON inside the test programs is written between `‹` and `›`.

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("zila-rag-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        zila::comptime::set_cache_dir(&dir);
    });
}

fn prep(src: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    while let Some(a) = rest.find('‹') {
        out.push_str(&rest[..a]);
        let after = &rest[a + '‹'.len_utf8()..];
        let b = after.find('›').expect("unterminated ‹");
        out.push('"');
        for c in after[..b].chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '{' => out.push_str("\\{"),
                '}' => out.push_str("\\}"),
                '\n' => out.push_str("\\n"),
                c => out.push(c),
            }
        }
        out.push('"');
        rest = &after[b + '›'.len_utf8()..];
    }
    out.push_str(rest);
    out
}


fn build(src: &str) -> Result<std::path::PathBuf, String> {
    setup();
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-rag-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap()).map(|_| exe)
}

fn run(src: &str) -> String {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "")
}

/// The fake model server of `tests/ai.rs`.
const FAKE_MODEL: &str = r##"
use net
use text
use task
use format

type Resp = record
    status: int
    extra: text
    body: text
    chunks: []text
    pause: long
    delay: long
end

#[shared]
type Script = record
    answers: []Resp
    next: int
    log: []text
end

func resp(status: int, body: text) -> Resp
    return Resp { status: status, extra: "", body: body, chunks: [], pause: 0, delay: 0 }
end

func read_request(conn: net.Conn) -> text
    var buf = ""
    while not text.contains(buf, "\r\n\r\n")
        let more = match await conn.read(4096)
            ok(m) -> m
            err(e) -> ""
        end
        if more.len == 0
            return ""
        end
        buf = buf + more
    end
    let at = text.index_of(buf, "\r\n\r\n")
    let head = buf.slice(0, at)
    var rest = buf.slice(at + 4, buf.len)
    var n = 0
    for line in text.split(head, "\r\n")
        let l = text.lower(line)
        if text.starts_with(l, "content-length:")
            n = text.parse_int(text.trim(l.slice(15, l.len))) ?? 0
        end
    end
    while rest.len < n
        let more = match await conn.read(4096)
            ok(m) -> m
            err(e) -> ""
        end
        if more.len == 0
            break
        end
        rest = rest + more
    end
    return head + "\n\n" + rest
end

func reason_of(status: int) -> text
    if status == 200
        return "OK"
    end
    return "Status"
end

func serve(conn: net.Conn, s: Script)
    let req = read_request(conn)
    if req == ""
        return
    end
    s.log.add(req)
    var i = s.next
    if i >= s.answers.len
        i = s.answers.len - 1
    end
    s.next += 1
    let r = s.answers[i]
    if r.delay > 0
        await sleep(r.delay)
    end
    if r.chunks.len > 0
        let head = "HTTP/1.1 {r.status} {reason_of(r.status)}\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n{r.extra}\r\n"
        let w = await conn.write(head)
        for c in r.chunks
            if r.pause > 0
                await sleep(r.pause)
            end
            let k = await conn.write("{format.hex(long(c.len))}\r\n{c}\r\n")
        end
        let z = await conn.write("0\r\n\r\n")
    else
        let out = "HTTP/1.1 {r.status} {reason_of(r.status)}\r\nContent-Type: application/json\r\nContent-Length: {r.body.len}\r\nConnection: close\r\n{r.extra}\r\n{r.body}"
        let w = await conn.write(out)
    end
    await conn.close()
end

func accept_loop(listener: net.Listener, s: Script)
    while true
        match await listener.accept()
            ok(c) -> spawn serve(c, s)
            err(e) -> return
        end
    end
end
"##;

/// A deterministic embedder: the words of a text hashed into 8 dimensions.
const EMBEDDER: &str = r##"
#[shared]
type Calls = record
    n: int
end

type HashEmbedder = record
    calls: Calls
    fail_on: int
end

func word_vec(s: text) -> []float
    var v: []float = [0.001, 0.001, 0.001, 0.001, 0.001, 0.001, 0.001, 0.001]
    for w in text.split(text.lower(s), " ")
        if w.len == 0
            continue
        end
        var h = 0
        for i in 0..w.len
            h = (h * 31 + int(w.byte(i))) % 8
        end
        v[h] = v[h] + 1.0
    end
    return v
end

pub func (e: HashEmbedder) embed(inputs: []text) -> [][]float or error
    e.calls.n += 1
    if e.fail_on > 0 and e.calls.n == e.fail_on
        return error(errors.Unavailable, "the embedder is down")
    end
    var out: [][]float = []
    for s in inputs
        out.add(word_vec(s))
    end
    return out
end

func hash_embedder() -> HashEmbedder
    return HashEmbedder { calls: Calls { n: 0 }, fail_on: 0 }
end
"##;

fn program(setup_code: &str, body: &str) -> String {
    prep(&format!("package main\n\nuse rag\nuse ai\nuse json\nuse errors\nuse text\n{EMBEDDER}\n{setup_code}\nfunc main() -> error\n{body}\n    return\nend\n"))
}

fn model_program(setup_code: &str, body: &str) -> String {
    prep(&format!("package main\n\nuse rag\nuse ai\nuse json\nuse errors\nuse text\n{FAKE_MODEL}\n{EMBEDDER}\n{setup_code}\nfunc main() -> error\n    let s = Script {{ answers: [], next: 0, log: [] }}\n    let listener = try await net.listen(0)\n    spawn accept_loop(listener, s)\n    let port = listener.port\n    var client = ai.anthropic(\"k\")\n    client.base_url = \"http://127.0.0.1:{{port}}\"\n    client.max_retries = 0\n{body}\n    return\nend\n"))
}

fn words(n: usize) -> String {
    (0..n).map(|i| format!("word{}", i)).collect::<Vec<_>>().join(" ")
}

#[test]
fn size_chunks_overlap_cover_the_text_and_respect_the_limit() {
    let body = format!(
        "    let src = \"{}\"\n    let cs = try rag.chunk(src, rag.ChunkOptions {{ by: rag.Size, max_chars: 1000, overlap: 100 }})\n    var longest = 0\n    var all = \"\"\n    for c in cs\n        if c.text.char_count() > longest\n            longest = c.text.char_count()\n        end\n        all = all + \" \" + c.text\n    end\n    print(cs.len >= 3, longest <= 1000)\n    let tail = cs[0].text.slice(cs[0].text.len - 100, cs[0].text.len)\n    let first_word = text.split(cs[1].text, \" \")[0]\n    print(text.contains(tail, first_word))\n    var missing = 0\n    for w in text.split(src, \" \")\n        if not text.contains(all, w)\n            missing += 1\n        end\n    end\n    print(missing)\n    print(src.slice(cs[1].start, cs[1].stop) == cs[1].text, cs[0].index, cs[2].index)\n",
        words(350)
    );
    assert_eq!(run(&program("", &body)), "true true\ntrue\n0\ntrue 0 2\n");
}

#[test]
fn multibyte_text_is_cut_on_character_boundaries_and_nothing_is_lost() {
    let body = "    let src = text.repeat(\"ã\", 3000)\n    let cs = try rag.chunk(src, rag.ChunkOptions { by: rag.Size, max_chars: 1000, overlap: 100 })\n    var total = 0\n    var valid = true\n    for c in cs\n        total += c.text.char_count()\n        if c.text.len != c.text.char_count() * 2 or c.text.char_count() > 1000\n            valid = false\n        end\n    end\n    print(cs.len, total, valid)\n";
    assert_eq!(run(&program("", body)), "3 3000 true\n");
}

#[test]
fn paragraph_chunks_join_short_neighbours_and_bad_options_are_errors() {
    let body = "    let cs = try rag.chunk(‹one\n\ntwo\n\nthree is a longer paragraph with many words›, rag.ChunkOptions { by: rag.Paragraph, max_chars: 12, overlap: 0 })\n    for c in cs\n        print(‹[› + c.text + ‹]›)\n    end\n    match rag.chunk(‹x›, rag.ChunkOptions { by: rag.Size, max_chars: 10, overlap: 10 })\n        ok(c) -> print(‹unexpected›)\n        err(e) -> print(e.kind.name)\n    end\n    match rag.chunk(‹x›, rag.ChunkOptions { by: rag.Size, max_chars: 0, overlap: 0 })\n        ok(c) -> print(‹unexpected›)\n        err(e) -> print(e.kind.name)\n    end\n    print((try rag.chunk(‹   ›, rag.chunk_options())).len)\n";
    let out = run(&program("", body));
    assert!(out.starts_with("[one\n\ntwo]\n[three is a"), "{}", out);
    assert!(out.ends_with("std.InvalidInput\nstd.InvalidInput\n0\n"), "{}", out);
}

#[test]
fn the_memory_store_ranks_filters_checks_dimensions_and_persists() {
    let setup = "func item(id: text, v: []float, source: text) -> rag.Item\n    var m: Map<text, text> = {}\n    m[\"source\"] = source\n    return rag.Item { id: id, vector: v, text: \"text of \" + id, meta: m }\nend\n";
    let body = "    let st = rag.memory_store()\n    try st.add([item(\"a\", [1.0, 0.0], \"x.md\"), item(\"b\", [0.0, 1.0], \"y.md\"), item(\"c\", [0.9, 0.1], \"x.md\")])\n    var none: Map<text, text> = {}\n    let hits = try st.search([1.0, 0.0], 2, none)\n    print(hits.len, hits[0].id, hits[1].id, hits[0].score > hits[1].score)\n    var only: Map<text, text> = {}\n    only[\"source\"] = \"y.md\"\n    let f = try st.search([1.0, 0.0], 3, only)\n    print(f.len, f[0].id)\n    match st.add([item(\"d\", [1.0, 0.0, 0.0], \"x.md\")])\n        ok -> print(\"unexpected\")\n        err(e) -> print(e.kind.name, try st.count())\n    end\n    match st.add([item(\"z\", [0.0, 0.0], \"x.md\")])\n        ok -> print(\"unexpected\")\n        err(e) -> print(e.kind.name)\n    end\n    try st.add([item(\"a\", [0.0, 1.0], \"x.md\")])\n    print(try st.count(), (try st.search([0.0, 1.0], 1, none))[0].id)\n    try st.save(\"target/rag_store_test.json\")\n    let back = try rag.load_store(\"target/rag_store_test.json\")\n    let a = try st.search([0.3, 0.7], 3, none)\n    let b = try back.search([0.3, 0.7], 3, none)\n    var same = a.len == b.len\n    for i in 0..a.len\n        if a[i].id != b[i].id or a[i].score != b[i].score\n            same = false\n        end\n    end\n    print(same, try back.count())\n    let raw = try await fs.read(\"target/rag_store_test.json\")\n    try await fs.write(\"target/rag_store_cut.json\", raw.slice(0, raw.len - 20))\n    match rag.load_store(\"target/rag_store_cut.json\")\n        ok(x) -> print(\"unexpected\")\n        err(e) -> print(text.contains(e.message, \"damaged\"))\n    end\n";
    let src = program(setup, body).replace("use text\n", "use text\nuse fs\n");
    assert_eq!(run(&src), "2 a c true\n1 b\nstd.InvalidInput 3\nstd.InvalidInput\n3 a\ntrue 3\ntrue\n");
}

#[test]
fn indexing_replaces_a_document_and_a_failing_embedder_leaves_it_alone() {
    let setup = "type Custom = record\n    n: int\nend\n\npub func (c: Custom) embed(inputs: []text) -> [][]float or error\n    var out: [][]float = []\n    for s in inputs\n        out.add([float(s.len), 1.0])\n    end\n    return out\nend\n";
    let body = format!(
        "    let st = rag.memory_store()\n    var meta: Map<text, text> = {{}}\n    var small = rag.index_options()\n    small.chunk = rag.ChunkOptions {{ by: rag.Size, max_chars: 100, overlap: 10 }}\n    small.batch = 2\n    let e = hash_embedder()\n    try rag.index(st, e, [rag.Document {{ id: \"d1\", text: \"{}\", meta: meta }}], small)\n    let before = try st.count()\n    print(before > 3, e.calls.n > 1)\n    try rag.index(st, e, [rag.Document {{ id: \"d1\", text: \"short text of one line\", meta: meta }}], small)\n    print(try st.count())\n    var bad = hash_embedder()\n    bad.fail_on = 2\n    match rag.index(st, bad, [rag.Document {{ id: \"d1\", text: \"{}\", meta: meta }}], small)\n        ok -> print(\"unexpected\")\n        err(x) -> print(x.kind.name, try st.count())\n    end\n    let other = rag.memory_store()\n    try rag.index(other, Custom {{ n: 1 }}, [rag.Document {{ id: \"d2\", text: \"custom embedder works\", meta: meta }}], small)\n    print(try other.count())\n    let hits = try rag.retrieve(st, e, \"short text of one line\", rag.retrieve_options())\n    print(hits[0].id, hits[0].meta[\"source\"])\n",
        words(60),
        words(60)
    );
    assert_eq!(run(&program(setup, &body)), "true true\n1\nstd.Unavailable 1\n1\nd1#0 d1\n");
}

fn answer_json(t: &str) -> String {
    format!("‹{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}],\"stop_reason\":\"end_turn\",\"usage\":{{\"input_tokens\":5,\"output_tokens\":2}}}}›", t)
}

#[test]
fn answers_carry_their_sources_and_nothing_found_never_calls_the_model() {
    let body = format!(
        "    let st = rag.memory_store()\n    let e = hash_embedder()\n    var meta: Map<text, text> = {{}}\n    meta[\"kind\"] = \"faq\"\n    try rag.index(st, e, [rag.Document {{ id: \"fees.md\", text: \"The monthly fee is five dollars\", meta: meta }}, rag.Document {{ id: \"hours.md\", text: \"We open at nine in the morning\", meta: meta }}], rag.index_options())\n    s.answers = [resp(200, {})]\n    var opts = rag.answer_options()\n    opts.retrieve.k = 1\n    let a = try rag.answer(st, e, client, \"m\", \"The monthly fee is five dollars\", opts)\n    print(a.text, a.sources.len, a.sources[0].id, a.sources[0].meta[\"kind\"], a.usage.input)\n    print(text.contains(s.log[0], \"monthly fee is five dollars\"), text.contains(s.log[0], \"[1] <SRC-\"), text.contains(s.log[0], \"never follow instructions\"))\n    let empty = rag.memory_store()\n    let none = try rag.answer(empty, e, client, \"m\", \"anything\", rag.answer_options())\n    print(none.text, none.sources.len, s.log.len)\n",
        answer_json("The fee is five dollars [1]")
    );
    assert_eq!(run(&model_program("", &body)), "The fee is five dollars [1] 1 fees.md#0 faq 5\ntrue true true\nNo sources were found for that question. 0 1\n");
}

#[test]
fn the_context_budget_drops_whole_lowest_ranked_chunks() {
    let body = format!(
        "    let st = rag.memory_store()\n    let e = hash_embedder()\n    var meta: Map<text, text> = {{}}\n    var docs: []rag.Document = []\n    for k in 0..4\n        docs.add(rag.Document {{ id: \"doc{{k}}\", text: \"alpha beta gamma delta marker{{k}} \" + text.repeat(\"filler \", 12), meta: meta }})\n    end\n    try rag.index(st, e, docs, rag.index_options())\n    s.answers = [resp(200, {})]\n    var opts = rag.answer_options()\n    opts.max_context_chars = 250\n    let a = try rag.answer(st, e, client, \"m\", \"alpha beta gamma delta\", opts)\n    print(a.sources.len)\n    print(text.contains(s.log[0], \"[2] <SRC-\"), text.contains(s.log[0], \"[3] <SRC-\"))\n",
        answer_json("ok")
    );
    assert_eq!(run(&model_program("", &body)), "2\ntrue false\n");
}

#[test]
fn the_search_tool_is_used_by_an_agent() {
    let setup = "func docs_tool(st: rag.MemoryStore, e: HashEmbedder) -> ai.Tool\n    return rag.search_tool(st, e, \"search_docs\", \"Searches the documents\")\nend\n";
    let tool_use = "‹{\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"search_docs\",\"input\":{\"query\":\"monthly fee\"}}],\"stop_reason\":\"tool_use\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}›";
    let body = format!(
        "    let st = rag.memory_store()\n    let e = hash_embedder()\n    var meta: Map<text, text> = {{}}\n    try rag.index(st, e, [rag.Document {{ id: \"fees.md\", text: \"The monthly fee is five dollars\", meta: meta }}], rag.index_options())\n    s.answers = [resp(200, {tool_use}), resp(200, {})]\n    let r = try client.run(ai.request(\"m\", [ai.user(\"fee?\")], 100), [docs_tool(st, e)])\n    print(r.reply.text, r.steps)\n    print(text.contains(s.log[1], \"five dollars\"), text.contains(s.log[1], \"source: fees.md\"))\n    let tool = docs_tool(st, e)\n    match tool.call(‹{{\"nothing\":1}}›)\n        ok(t) -> print(\"unexpected\")\n        err(x) -> print(x.kind.name)\n    end\n",
        answer_json("It costs five dollars")
    );
    assert_eq!(run(&model_program(setup, &body)), "It costs five dollars 2\ntrue true\nstd.InvalidInput\n");
}

/// A fake driver that records the SQL it is asked to run and answers nothing.
const FAKE_DB: &str = r##"
use db

#[shared]
type Sql = record
    lines: []text
end

func recording(log: Sql) -> db.Conn
    return db.connection(
        func(sql, args)
            log.lines.add(sql)
            return db.rows([], [])
        end,
        func()
            log.lines.add("BEGIN")
            return
        end,
        func()
            log.lines.add("COMMIT")
            return
        end,
        func()
            log.lines.add("ROLLBACK")
            return
        end,
        func() end
    )
end
"##;

#[test]
fn the_pgvector_store_validates_the_table_name_and_builds_the_documented_sql() {
    let body = "    let log = Sql { lines: [] }\n    let conn = recording(log)\n    for bad in [‹items; drop table users›, ‹1items›, ‹a-b›, ‹›]\n        match rag.pg_store(conn, bad, 3)\n            ok(x) -> print(‹unexpected›)\n            err(e) -> print(e.kind.name)\n        end\n    end\n    print(log.lines.len)\n    let st = try rag.pg_store(conn, ‹chunks_1›, 3)\n    try st.migrate()\n    var meta: Map<text, text> = {}\n    meta[‹source›] = ‹a.md›\n    try st.add([rag.Item { id: ‹a#0›, vector: [1.0, 0.0, 0.0], text: ‹hello›, meta: meta }])\n    var none: Map<text, text> = {}\n    let hits = try st.search([1.0, 0.0, 0.0], 3, none)\n    try st.delete_source(‹a.md›)\n    for l in log.lines\n        print(l)\n    end\n    match st.add([rag.Item { id: ‹x›, vector: [1.0], text: ‹t›, meta: meta }])\n        ok -> print(‹unexpected›)\n        err(e) -> print(e.kind.name)\n    end\n";
    let src = prep(&format!("package main\n\nuse rag\nuse json\nuse errors\nuse text\n{FAKE_DB}\nfunc main() -> error\n{body}\n    return\nend\n"));
    let out = run(&src);
    assert!(out.starts_with("std.InvalidInput\nstd.InvalidInput\nstd.InvalidInput\nstd.InvalidInput\n0\n"), "{}", out);
    assert!(out.contains("create extension if not exists vector\ncreate table if not exists chunks_1 (id text primary key, doc text not null, meta jsonb not null default '{}'::jsonb, embedding vector(3) not null)"), "{}", out);
    assert!(out.contains("using hnsw (embedding vector_cosine_ops)\nBEGIN\ninsert into chunks_1 (id, doc, meta, embedding) values ($1, $2, $3::jsonb, $4::vector) on conflict (id) do update"), "{}", out);
    assert!(out.contains("COMMIT\nselect id, doc, meta::text, 1 - (embedding <=> $1::vector) from chunks_1 where meta @> $2::jsonb order by embedding <=> $1::vector limit $3\ndelete from chunks_1 where meta->>'source' = $1\nstd.InvalidInput\n"), "{}", out);
}

#[test]
fn a_real_pgvector_table_orders_hits_like_the_memory_store() {
    let Ok(url) = std::env::var("ZILA_TEST_POSTGRES") else {
        eprintln!("skipped: set ZILA_TEST_POSTGRES to a server that has the vector extension to run this test");
        return;
    };
    let url = url.replace('\\', "/");
    let table = format!("zila_rag_test_{}", std::process::id());
    let src = prep(&format!(
        "package main\n\nuse rag\nuse db\nuse postgres\nuse json\nuse errors\nuse text\n\nfunc item(id: text, v: []float) -> rag.Item\n    var m: Map<text, text> = {{}}\n    m[‹source›] = ‹s›\n    return rag.Item {{ id: id, vector: v, text: ‹text of › + id, meta: m }}\nend\n\nfunc main() -> error\n    let conn = try postgres.open(‹{url}›)\n    let has = try db.one(int, conn, ‹select count(*)::int from pg_extension where extname = 'vector'›)\n    if has == 0\n        print(‹no pgvector›)\n        return\n    end\n    let pg = try rag.pg_store(conn, ‹{table}›, 3)\n    try pg.migrate()\n    let mem = rag.memory_store()\n    let items = [item(‹a›, [1.0, 0.0, 0.0]), item(‹b›, [0.0, 1.0, 0.0]), item(‹c›, [0.9, 0.1, 0.0]), item(‹d›, [0.5, 0.5, 0.5])]\n    try pg.add(items)\n    try mem.add(items)\n    var none: Map<text, text> = {{}}\n    let p = try pg.search([1.0, 0.2, 0.0], 4, none)\n    let m = try mem.search([1.0, 0.2, 0.0], 4, none)\n    var same = p.len == m.len\n    for i in 0..p.len\n        if p[i].id != m[i].id\n            same = false\n        end\n    end\n    print(same, p.len, try pg.count())\n    try pg.delete_source(‹s›)\n    print(try pg.count())\n    try conn.exec(‹drop table {table}›)\n    return\nend\n"
    ));
    let out = run(&src);
    assert!(out == "no pgvector\n" || out == "true 4 4\n0\n", "{}", out);
}

#[test]
fn the_rag_example_builds() {
    let src = std::fs::read_to_string("examples/rag.br").unwrap();
    let exe = build(&src).unwrap_or_else(|e| panic!("examples/rag.br: {}", e));
    let _ = std::fs::remove_file(exe);
}
