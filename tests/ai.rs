//! The `ai` library (spec `ai-client`) against a fake model server written in Zila Lang, in the same
//! program as the client: scripted answers, server-sent events with pauses, errors, retries, tool
//! calls, typed output and embeddings. Real APIs are not called here; the tests at the end run
//! only when `ZILA_TEST_ANTHROPIC_KEY` or `ZILA_TEST_OPENAI_URL` is set.
//!
//! Zila Lang text literals treat `{` as the start of an interpolation, so JSON inside the test
//! programs is written between `‹` and `›`; `prep` turns each such span into an escaped literal.

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("zila-ai-test-cache-{}", std::process::id()));
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
    let exe = std::env::temp_dir().join(format!("zila-ai-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap()).map(|_| exe)
}

fn run(src: &str) -> String {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "")
}

fn compile_error(src: &str) -> String {
    match build(src) {
        Ok(exe) => {
            let _ = std::fs::remove_file(exe);
            panic!("expected a compile error")
        }
        Err(e) => e,
    }
}

/// The fake server. A `Script` holds the answers in order (the last one repeats) and logs every
/// request (its head and body). An answer has a status, extra header lines, a body or `chunks`
/// (sent as a chunked response with `pause` milliseconds between them) and a `delay` before it.
const FAKE: &str = r##"
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

fn program(setup_code: &str, body: &str) -> String {
    prep(&format!(
        "package main\n\nuse ai\nuse json\n{FAKE}\n{setup_code}\nfunc main() -> error\n    let s = Script {{ answers: [], next: 0, log: [] }}\n    let listener = try await net.listen(0)\n    spawn accept_loop(listener, s)\n    let port = listener.port\n    let anth = ai.anthropic(\"sk-SECRET\")\n    let oa = ai.openai_compatible(\"http://127.0.0.1:{{port}}/v1\", \"sk-SECRET\")\n    var a2 = anth\n    a2.base_url = \"http://127.0.0.1:{{port}}\"\n    a2.max_retries = 0\n{body}\n    return\nend\n"
    ))
}

const ANTH_OK: &str = "‹{\"content\":[{\"type\":\"text\",\"text\":\"Hello there\"}],\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":12,\"output_tokens\":3}}›";

#[test]
fn chat_works_for_both_providers_and_sends_the_key_only_in_headers() {
    let body = format!(
        "    s.answers = [resp(200, {ANTH_OK})]\n    let r = try a2.chat(ai.request(\"m1\", [ai.user(\"hi\")], 50))\n    print(r.text, r.stop == ai.End, r.usage.input, r.usage.output)\n    s.answers = [resp(200, ‹{{\"choices\":[{{\"message\":{{\"content\":\"Olá\"}},\"finish_reason\":\"length\"}}],\"usage\":{{\"prompt_tokens\":7,\"completion_tokens\":2}}}}›)]\n    let o = try oa.chat(ai.request(\"m2\", [ai.user(\"oi\")], 50))\n    print(o.text, o.stop == ai.MaxTokens, o.usage.input, o.usage.output)\n    print(s.log.len)\n    print(text.contains(s.log[0], \"x-api-key: sk-SECRET\"), text.contains(s.log[0], \"anthropic-version\"), text.contains(s.log[0], ‹\"model\":\"m1\"›))\n    print(text.contains(s.log[1], \"Authorization: Bearer sk-SECRET\"), text.contains(s.log[1], \"POST /v1/chat/completions\"))\n    print(text.contains(s.log[0].slice(s.log[0].len - 80, s.log[0].len), \"sk-SECRET\"))\n"
    );
    let out = run(&program("", &body));
    assert_eq!(out, "Hello there true 12 3\nOlá true 7 2\n2\ntrue true true\ntrue true\nfalse\n");
}

#[test]
fn error_statuses_map_to_kinds_and_the_key_never_appears_in_a_message() {
    let body = "    for status in [401, 403, 400, 404, 429, 500]\n        s.next = 0\n        s.answers = [resp(status, ‹{\"error\":{\"type\":\"x\",\"message\":\"nope sk-SECRET\"}}›)]\n        match a2.chat(ai.request(\"m\", [ai.user(\"hi\")], 5))\n            ok(r) -> print(\"unexpected\")\n            err(e) -> print(status, e.kind.name, text.contains(e.message, \"nope\"))\n        end\n    end\n";
    let out = run(&program("", body));
    // the provider's own text is passed on; that the key stays out of what the library adds is
    // checked on a body without it below
    assert_eq!(out, "401 std.Unauthenticated true\n403 std.PermissionDenied true\n400 std.InvalidInput true\n404 std.NotFound true\n429 std.Unavailable true\n500 std.Unavailable true\n");
    let body = "    s.answers = [resp(401, ‹{\"error\":{\"message\":\"bad key\"}}›)]\n    match a2.chat(ai.request(\"m\", [ai.user(\"hi\")], 5))\n        ok(r) -> print(\"unexpected\")\n        err(e) -> print(text.contains(e.message, \"sk-SECRET\"), e.message)\n    end\n";
    assert_eq!(run(&program("", body)), "false model API error 401: bad key\n");
}

#[test]
fn a_missing_environment_variable_is_named_in_the_error() {
    let body = "    match ai.key_from_env(\"ZILA_SURELY_NOT_SET_9\")\n        ok(k) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name, e.message)\n    end\n";
    assert_eq!(run(&program("", body)), "std.InvalidInput the environment variable ZILA_SURELY_NOT_SET_9 is not set or is empty\n");
}

#[test]
fn transient_failures_are_retried_and_a_bad_request_is_not() {
    let body = format!(
        "    var c = anth\n    c.base_url = \"http://127.0.0.1:{{port}}\"\n    c.max_retries = 2\n    var busy = resp(503, ‹{{\"error\":{{\"message\":\"busy\"}}}}›)\n    busy.extra = \"retry-after: 0\\r\\n\"\n    s.answers = [busy, resp(200, {ANTH_OK})]\n    let r = try c.chat(ai.request(\"m\", [ai.user(\"hi\")], 5))\n    print(r.text, s.log.len)\n    s.next = 0\n    s.log = []\n    s.answers = [resp(400, ‹{{\"error\":{{\"message\":\"bad\"}}}}›)]\n    match c.chat(ai.request(\"m\", [ai.user(\"hi\")], 5))\n        ok(x) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name, s.log.len)\n    end\n"
    );
    assert_eq!(run(&program("", &body)), "Hello there 2\nstd.InvalidInput 1\n");
}

#[test]
fn a_stream_delivers_pieces_as_they_arrive_and_the_reply_at_the_end() {
    let events = "‹event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":9}}}\n\n›, ‹event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n›";
    let body = format!(
        "    var first = resp(200, \"\")\n    first.chunks = [{events}, ‹event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"Hel\"}}}}\n\n›, ‹data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"lo\"}}}}\n\n›, ‹data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"!\"}}}}\n\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"output_tokens\":3}}}}\n\n›]\n    first.pause = 150\n    s.answers = [first]\n    let seen = Seen {{ pieces: [], times: [] }}\n    let t0 = task.now()\n    let r = try a2.stream(ai.request(\"m\", [ai.user(\"hi\")], 5), func(p) return note(seen, p, task.now() - t0) end)\n    let total = task.now() - t0\n    print(r.text, r.usage.input, r.usage.output, r.stop == ai.End)\n    print(seen.pieces.len, seen.pieces[0], seen.pieces[1], seen.pieces[2])\n    print(seen.times[0] < total - 200)\n"
    );
    let setup = "#[shared]\ntype Seen = record\n    pieces: []text\n    times: []long\nend\n\nfunc note(s: Seen, p: text, at: long) -> error\n    s.pieces.add(p)\n    s.times.add(at)\n    return\nend\n";
    let src = program(setup, &body).replace("use json\n", "use json\nuse task\n");
    assert_eq!(run(&src), "Hello! 9 3 true\n3 Hel lo !\ntrue\n");
}

#[test]
fn a_callback_error_stops_the_stream_and_is_returned() {
    let body = "    var first = resp(200, \"\")\n    first.chunks = [‹data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"one\"}}\n\n›, ‹data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"two\"}}\n\n›]\n    first.pause = 50\n    s.answers = [first]\n    match a2.stream(ai.request(\"m\", [ai.user(\"hi\")], 5), func(p) return error(errors.Cancelled, \"stop after \" + p) end)\n        ok(r) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name, e.message)\n    end\n";
    let src = program("", body).replace("use json\n", "use json\nuse errors\n");
    assert_eq!(run(&src), "std.Cancelled stop after one\n");
}

const INVOICE: &str = "type Invoice = record\n    #[desc(\"the invoice number\")] number: text\n    total: float\n    paid: bool\n    note: text?\n    lines: []text\nend\n\n";

#[test]
fn the_schema_comes_from_the_type() {
    let setup = format!("{INVOICE}type Shape = enum\n    Dot\n    Circle(float)\n    Rect(float, float)\nend\n\ntype Holder = record\n    tags: []text\n    scores: Map<text, int>\n    shape: Shape\nend\n\n");
    let body = "    print(ai.schema(Invoice))\n    print(ai.schema(Holder))\n";
    let out = run(&program(&setup, body));
    assert_eq!(
        out,
        "{\"type\":\"object\",\"properties\":{\"number\":{\"type\":\"string\",\"description\":\"the invoice number\"},\"total\":{\"type\":\"number\"},\"paid\":{\"type\":\"boolean\"},\"note\":{\"type\":\"string\"},\"lines\":{\"type\":\"array\",\"items\":{\"type\":\"string\"}}},\"required\":[\"number\",\"total\",\"paid\",\"lines\"]}\n{\"type\":\"object\",\"properties\":{\"tags\":{\"type\":\"array\",\"items\":{\"type\":\"string\"}},\"scores\":{\"type\":\"object\",\"additionalProperties\":{\"type\":\"integer\"}},\"shape\":{\"oneOf\":[{\"enum\":[\"Dot\"]},{\"type\":\"object\",\"properties\":{\"Circle\":{\"type\":\"number\"}},\"required\":[\"Circle\"]},{\"type\":\"object\",\"properties\":{\"Rect\":{\"type\":\"array\",\"prefixItems\":[{\"type\":\"number\"},{\"type\":\"number\"}]}},\"required\":[\"Rect\"]}]}},\"required\":[\"tags\",\"scores\",\"shape\"]}\n"
    );
}

fn anth_text(t: &str) -> String {
    format!("‹{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}],\"stop_reason\":\"end_turn\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}}}}›", t.replace('"', "\\\"").replace('\n', "\\n"))
}

#[test]
fn ask_returns_a_value_of_the_type_repairs_once_and_then_gives_up() {
    let good = "{\"number\":\"A-7\",\"total\":12.5,\"paid\":true,\"lines\":[\"x\",\"y\"]}";
    let fenced = format!("```json\n{good}\n```");
    let body = format!(
        "    s.answers = [resp(200, {})]\n    let inv = try ai.ask(Invoice, a2, ai.request(\"m\", [ai.user(\"read this\")], 100))\n    print(inv.number, inv.total, inv.paid, inv.note == nil, inv.lines.len)\n    s.next = 0\n    s.log = []\n    s.answers = [resp(200, {}), resp(200, {})]\n    let again = try ai.ask(Invoice, a2, ai.request(\"m\", [ai.user(\"read this\")], 100))\n    print(again.number, s.log.len, text.contains(s.log[1], \"That answer was not valid\"), text.contains(s.log[0], \"JSON Schema\"))\n    s.next = 0\n    s.log = []\n    s.answers = [resp(200, {})]\n    match ai.ask(Invoice, a2, ai.request(\"m\", [ai.user(\"read this\")], 100))\n        ok(v) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name, s.log.len, text.contains(e.message, \"not json at all\"))\n    end\n",
        anth_text(&fenced),
        anth_text("sorry, here you go"),
        anth_text(good),
        anth_text("not json at all")
    );
    let out = run(&program(INVOICE, &body));
    assert_eq!(out, "A-7 12.5 true true 2\nA-7 2 true true\nstd.InvalidInput 2 true\n");
}

fn tool_use(id: &str, name: &str, input: &str) -> String {
    format!("‹{{\"content\":[{{\"type\":\"tool_use\",\"id\":\"{id}\",\"name\":\"{name}\",\"input\":{input}}}],\"stop_reason\":\"tool_use\",\"usage\":{{\"input_tokens\":4,\"output_tokens\":2}}}}›")
}

const WEATHER: &str = "type Weather = record\n    city: text\nend\n\n#[shared]\ntype Calls = record\n    cities: []text\nend\n\nfunc weather_tool(calls: Calls) -> ai.Tool\n    return ai.tool(Weather, \"get_weather\", \"The weather in a city\", func(w)\n        calls.cities.add(w.city)\n        return \"sunny in \" + w.city\n    end)\nend\n\n";

#[test]
fn a_tool_round_trip_runs_the_handler_once_and_returns_the_final_text() {
    let body = format!(
        "    let calls = Calls {{ cities: [] }}\n    s.answers = [resp(200, {}), resp(200, {})]\n    let r = try a2.run(ai.request(\"m\", [ai.user(\"weather in Recife?\")], 100), [weather_tool(calls)])\n    print(r.reply.text, r.steps, r.messages.len, r.usage.input)\n    print(calls.cities.len, calls.cities[0])\n    print(text.contains(s.log[1], \"sunny in Recife\"), text.contains(s.log[0], \"input_schema\"), text.contains(s.log[0], \"get_weather\"))\n",
        tool_use("tu_1", "get_weather", "{\"city\":\"Recife\"}"),
        anth_text("It is sunny in Recife.")
    );
    let out = run(&program(WEATHER, &body));
    assert_eq!(out, "It is sunny in Recife. 2 3 5\n1 Recife\ntrue true true\n");
}

#[test]
fn the_loop_has_a_step_limit_and_bad_arguments_and_unknown_tools_are_data() {
    let body = format!(
        "    let calls = Calls {{ cities: [] }}\n    var c = a2\n    c.max_steps = 3\n    s.answers = [resp(200, {})]\n    match c.run(ai.request(\"m\", [ai.user(\"go\")], 100), [weather_tool(calls)])\n        ok(r) -> print(\"unexpected\")\n        err(e) -> print(e.message, s.log.len, calls.cities.len)\n    end\n    s.next = 0\n    s.log = []\n    s.answers = [resp(200, {}), resp(200, {}), resp(200, {})]\n    let r = try a2.run(ai.request(\"m\", [ai.user(\"go\")], 100), [weather_tool(calls)])\n    print(r.reply.text, calls.cities.len, text.contains(s.log[1], \"do not match\"), text.contains(s.log[1], \"city\"))\n    print(text.contains(s.log[2], \"there is no tool named nothing\"), text.contains(s.log[2], \"is_error\"))\n",
        tool_use("tu", "get_weather", "{\"city\":\"Recife\"}"),
        tool_use("bad", "get_weather", "{\"city\":5}"),
        tool_use("gone", "nothing", "{}"),
        anth_text("done")
    );
    let out = run(&program(WEATHER, &body));
    assert_eq!(out, "the agent loop stopped after 3 model calls (max_steps) 3 3\ndone 3 true true\ntrue true\n");
}

#[test]
fn a_handler_error_goes_back_to_the_model_and_calls_in_one_turn_run_together() {
    let two = "‹{\"content\":[{\"type\":\"tool_use\",\"id\":\"a\",\"name\":\"slow\",\"input\":{\"n\":1}},{\"type\":\"tool_use\",\"id\":\"b\",\"name\":\"slow\",\"input\":{\"n\":2}}],\"stop_reason\":\"tool_use\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}›";
    let setup = "type Slow = record\n    n: int\nend\n\ntype Boom = record\n    n: int\nend\n\n";
    let body = format!(
        "    let slow = ai.tool(Slow, \"slow\", \"waits\", func(w)\n        await sleep(200)\n        return \"waited {{w.n}}\"\n    end)\n    let boom = ai.tool(Boom, \"boom\", \"fails\", func(w)\n        return error(errors.Other, \"it broke\")\n    end)\n    s.answers = [resp(200, {two}), resp(200, {fine})]\n    let t0 = task.now()\n    let r = try a2.run(ai.request(\"m\", [ai.user(\"go\")], 100), [slow, boom])\n    let took = task.now() - t0\n    print(r.reply.text, took < 380, text.contains(s.log[1], \"waited 1\"), text.contains(s.log[1], \"waited 2\"))\n    s.next = 0\n    s.log = []\n    s.answers = [resp(200, {boom}), resp(200, {recovered})]\n    let r2 = try a2.run(ai.request(\"m\", [ai.user(\"go\")], 100), [slow, boom])\n    print(r2.reply.text, text.contains(s.log[1], \"it broke\"), text.contains(s.log[1], ‹\"is_error\":true›))\n",
        boom = tool_use("x", "boom", "{\"n\":1}"),
        fine = anth_text("fine"),
        recovered = anth_text("recovered")
    );
    let body = body.replace("{anth_text(\"fine\")}", "");
    let src = program(setup, &body).replace("use json\n", "use json\nuse task\nuse errors\n");
    let out = run(&src);
    assert_eq!(out, "fine true true true\nrecovered true true\n");
}

#[test]
fn embeddings_are_batched_in_order_and_anthropic_has_none() {
    let reply = "‹{\"data\":[@ITEMS@]}›";
    let _ = reply;
    let setup = "func vectors(n: int, offset: int) -> text\n    var items: []text = []\n    for k in 0..n\n        let at = n - 1 - k\n        items.add(\"\\{\\\"index\\\":{at},\\\"embedding\\\":[{offset + at}.0,0.5]\\}\")\n    end\n    return \"\\{\\\"data\\\":[\" + text.join(items, \",\") + \"]\\}\"\nend\n\n";
    let body = "    var inputs: []text = []\n    for k in 0..150\n        inputs.add(\"doc {k}\")\n    end\n    s.answers = [resp(200, vectors(64, 0)), resp(200, vectors(64, 64)), resp(200, vectors(22, 128))]\n    let e = oa.embedder(\"emb\")\n    let vs = try e.embed(inputs)\n    print(vs.len, s.log.len, vs[0][0], vs[63][0], vs[64][0], vs[149][0])\n    print((try oa.embed(\"emb\", [])).len, s.log.len)\n    match a2.embed(\"emb\", [\"x\"])\n        ok(v) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name, text.contains(e.message, \"no embeddings endpoint\"))\n    end\n";
    let out = run(&program(setup, body));
    assert_eq!(out, "150 3 0.0 63.0 64.0 149.0\n0 3\nstd.InvalidInput true\n");
}

#[test]
fn twenty_chats_on_one_client_run_at_the_same_time() {
    let setup = "async func ask_one(c: ai.Client) -> text\n    match c.chat(ai.request(\"m\", [ai.user(\"hi\")], 5))\n        ok(r) -> return r.text\n        err(e) -> return e.message\n    end\n    return \"\"\nend\n\n";
    let body = format!(
        "    var slow = resp(200, {ANTH_OK})\n    slow.delay = 250\n    s.answers = [slow]\n    var fs: []Future<text> = []\n    let t0 = task.now()\n    for k in 0..20\n        fs.add(ask_one(a2))\n    end\n    var ok = 0\n    for f in fs\n        let t = await f\n        if t == \"Hello there\"\n            ok += 1\n        end\n    end\n    print(ok, task.now() - t0 < 2500)\n"
    );
    let src = program(setup, &body).replace("use json\n", "use json\nuse task\n");
    assert_eq!(run(&src), "20 true\n");
}

#[test]
fn a_value_that_is_not_a_message_list_is_a_compile_error() {
    let e = compile_error(&program("", "    let r = try a2.chat(ai.request(\"m\", \"hi\", 5))\n"));
    assert!(e.contains("messages") || e.contains("Message") || e.contains("expected"), "{}", e);
}

// ---------------------------------------------------------------------------
// Real APIs (opt in)
// ---------------------------------------------------------------------------

#[test]
fn a_real_chat_when_a_key_is_given() {
    let Ok(key) = std::env::var("ZILA_TEST_ANTHROPIC_KEY") else {
        eprintln!("skipped: set ZILA_TEST_ANTHROPIC_KEY (and ZILA_TEST_ANTHROPIC_MODEL) to call the real API");
        return;
    };
    let model = std::env::var("ZILA_TEST_ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-haiku-5-5".to_string());
    let src = format!("package main\n\nuse ai\n\nfunc main() -> error\n    let c = ai.anthropic(\"{key}\")\n    let r = try c.chat(ai.request(\"{model}\", [ai.user(\"Reply with the single word: pong\")], 20))\n    print(r.text.len > 0)\n    return\nend\n");
    assert_eq!(run(&src), "true\n");
}

#[test]
fn the_ai_example_builds_and_is_rejected_in_the_kernel() {
    let src = std::fs::read_to_string("examples/ai.zila").unwrap();
    let exe = build(&src).unwrap_or_else(|e| panic!("examples/ai.zila: {}", e));
    let _ = std::fs::remove_file(exe);
    setup();
    let file = parse_source("package main

use ai

func main()
    print(ai.user(\"x\").content)
end
").unwrap();
    let e = zila::codegen_x64::generate_asm_for(&file, &zila::codegen_x64::CompileOptions::default(), zila::codegen_x64::Target::Bare).expect_err("expected the kernel build to fail");
    assert!(e.contains("bare-metal") || e.contains("Windows"), "{}", e);
}
