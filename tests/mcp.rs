//! The `mcp` library (spec `mcp-library`): the JSON-RPC core through `handle`, tools, resources and
//! prompts, the stdio transport through a compiled server driven over pipes, and the Streamable
//! HTTP transport and the client over loopback. The same `ai.Tool` value is served and called
//! directly. JSON inside the test programs is written between `‹` and `›`.

use brasa::{compile_to_native, parse_source};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("brasa-mcp-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        brasa::comptime::set_cache_dir(&dir);
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
    let exe = std::env::temp_dir().join(format!("brasa-mcp-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap()).map(|_| exe)
}

fn run(src: &str) -> String {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "")
}

/// A demo server: tools `find` and `fail`, a resource, a template and a prompt.
const SETUP: &str = r##"
type Search = record
    #[desc("the name to look for")] name: text
end

func find_tool() -> ai.Tool
    return ai.tool(Search, "find", "Finds a customer", func(q)
        return "found " + q.name
    end)
end

func make_server() -> mcp.Server
    let s = mcp.server("demo", "1.0")
    s.tool(find_tool())
    s.tool(ai.tool(Search, "fail", "Always fails", func(q)
        return error(errors.Other, "it broke")
    end))
    s.resource("db://schema", "schema", "text/plain", func()
        return "CREATE TABLE t"
    end)
    s.template("db://rows/\{table\}", "rows", "text/plain", func(v)
        return "rows of " + v["table"]
    end)
    s.prompt("greet", "Greets", [mcp.PromptArg { name: "who", description: "", required: true }], func(a)
        return [ai.user("Hello " + a["who"])]
    end)
    return s
end

func send(s: mcp.Server, session: mcp.Session, message: text) -> text
    return s.handle(session, message) ?? "(no answer)"
end

func has(s: text, part: text) -> bool
    return text.contains(s, part)
end
"##;

fn program(body: &str) -> String {
    prep(&format!("package main\n\nuse mcp\nuse ai\nuse json\nuse errors\nuse text\nuse web\n{SETUP}\nfunc main() -> error\n    let s = make_server()\n    let session = s.new_session()\n{body}\n    return\nend\n"))
}

const INIT_MSG: &str = "‹{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}›";

#[test]
fn the_handshake_negotiates_the_version_and_lists_only_what_exists() {
    let body = format!(
        "    let r = send(s, session, {INIT_MSG})\n    print(has(r, ‹\"protocolVersion\":\"2025-06-18\"›), has(r, ‹\"tools\":›), has(r, ‹\"resources\":›), has(r, ‹\"prompts\":›), has(r, ‹\"serverInfo\"›))\n    let old = send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"2024-11-05\"}}}}›)\n    print(has(old, ‹\"protocolVersion\":\"2024-11-05\"›))\n    let odd = send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"1999-01-01\"}}}}›)\n    print(has(odd, ‹\"protocolVersion\":\"2025-06-18\"›))\n    let bare = mcp.server(\"bare\", \"0\")\n    bare.tool(find_tool())\n    let b = send(bare, bare.new_session(), {INIT_MSG})\n    print(has(b, ‹\"tools\":›), has(b, ‹\"resources\":›), has(b, ‹\"prompts\":›))\n    print(s.handle(session, ‹{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}›) == nil)\n"
    );
    assert_eq!(run(&program(&body)), "true true true true true\ntrue\ntrue\ntrue false false\ntrue\n");
}

#[test]
fn requests_before_initialize_unknown_methods_garbage_and_batches_get_the_standard_errors() {
    let body = format!(
        "    print(has(send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}}›), ‹\"code\":-32600›))\n    print(has(send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}}›), ‹\"result\"›))\n    let r = send(s, session, {INIT_MSG})\n    print(has(send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"nothing/here\"}}›), ‹\"code\":-32601›))\n    print(has(send(s, session, ‹not json›), ‹\"code\":-32700›))\n    print(has(send(s, session, ‹[1,2]›), ‹\"code\":-32600›))\n    print(has(send(s, session, ‹5›), ‹\"code\":-32600›))\n"
    );
    assert_eq!(run(&program(&body)), "true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn tools_are_listed_with_their_schema_and_called_and_failures_are_results() {
    let body = format!(
        "    let r0 = send(s, session, {INIT_MSG})\n    let l = send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}}›)\n    print(has(l, ‹\"name\":\"find\"›), has(l, ‹\"inputSchema\"›), has(l, ‹\"required\":[\"name\"]›), has(l, ‹the name to look for›))\n    let c = send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{{\"name\":\"find\",\"arguments\":{{\"name\":\"Ana\"}}}}}}›)\n    print(has(c, ‹found Ana›), has(c, ‹\"isError\":false›))\n    let f = send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{{\"name\":\"fail\",\"arguments\":{{\"name\":\"x\"}}}}}}›)\n    print(has(f, ‹it broke›), has(f, ‹\"isError\":true›), has(f, ‹\"error\"›))\n    let bad = send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{{\"name\":\"find\",\"arguments\":{{\"name\":5}}}}}}›)\n    print(has(bad, ‹\"code\":-32602›), has(bad, ‹name›))\n    let unknown = send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":{{\"name\":\"nothing\"}}}}›)\n    print(has(unknown, ‹\"code\":-32602›))\n    match find_tool().call(‹{{\"name\":\"direct\"}}›)\n        ok(t) -> print(t)\n        err(e) -> print(e.message)\n    end\n"
    );
    assert_eq!(run(&program(&body)), "true true true true\ntrue true\ntrue true false\ntrue true\ntrue\nfound direct\n");
}

#[test]
fn resources_templates_prompts_and_cursors() {
    let body = format!(
        "    let r0 = send(s, session, {INIT_MSG})\n    print(has(send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"resources/list\"}}›), ‹db://schema›))\n    print(has(send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"resources/templates/list\"}}›), ‹db://rows/{{table}}›))\n    let a = send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"resources/read\",\"params\":{{\"uri\":\"db://schema\"}}}}›)\n    print(has(a, ‹CREATE TABLE t›), has(a, ‹\"mimeType\":\"text/plain\"›))\n    print(has(send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"resources/read\",\"params\":{{\"uri\":\"db://rows/users\"}}}}›), ‹rows of users›))\n    print(has(send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"resources/read\",\"params\":{{\"uri\":\"db://nothing\"}}}}›), ‹\"code\":-32002›))\n    print(has(send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"prompts/list\"}}›), ‹greet›))\n    let g = send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"prompts/get\",\"params\":{{\"name\":\"greet\",\"arguments\":{{\"who\":\"Ana\"}}}}}}›)\n    print(has(g, ‹Hello Ana›), has(g, ‹\"role\":\"user\"›))\n    print(has(send(s, session, ‹{{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"prompts/get\",\"params\":{{\"name\":\"greet\"}}}}›), ‹\"code\":-32602›))\n    let big = mcp.server(\"big\", \"1\")\n    for k in 0..250\n        big.resource(\"db://r/{{k}}\", \"r{{k}}\", \"text/plain\", func() return \"x\" end)\n    end\n    let bs = big.new_session()\n    let z = send(big, bs, {INIT_MSG})\n    let p1 = send(big, bs, ‹{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"resources/list\"}}›)\n    print(has(p1, ‹\"nextCursor\":\"100\"›), has(p1, ‹db://r/99›), has(p1, ‹db://r/100›))\n    let p3 = send(big, bs, ‹{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"resources/list\",\"params\":{{\"cursor\":\"200\"}}}}›)\n    print(has(p3, ‹db://r/249›), has(p3, ‹nextCursor›))\n"
    );
    assert_eq!(run(&program(&body)), "true\ntrue\ntrue true\ntrue\ntrue\ntrue\ntrue true\ntrue\ntrue true false\ntrue false\n");
}

#[test]
fn a_stdio_server_answers_one_line_per_message_and_survives_noise() {
    let src = prep(&format!("package main\n\nuse mcp\nuse ai\nuse json\nuse errors\nuse text\n{SETUP}\nfunc main()\n    make_server().serve_stdio()\nend\n"));
    let exe = build(&src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let mut child = Command::new(&exe).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        let lines = [
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}",
            "not json",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"find\",\"arguments\":{\"name\":\"Ana\"}}}",
        ];
        for l in lines {
            writeln!(stdin, "{}", l).unwrap();
        }
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{}", text);
    assert!(lines[0].contains("\"protocolVersion\":\"2025-06-18\""), "{}", text);
    assert!(lines[1].contains("-32700"), "{}", text);
    assert!(lines[2].contains("found Ana") && lines[2].contains("\"isError\":false"), "{}", text);
    assert!(lines.iter().all(|l| !l.contains('\n')), "{}", text);
}

const HTTP_SETUP: &str = r##"
func start_http(s: mcp.Server) -> web.Running or error
    var app = web.server(port: 0)
    s.serve_http(app, "/mcp")
    return try app.start()
end

func post_raw(port: long, headers: Map<text, text>, body: text) -> web.ClientResponse or error
    return web.request(web.Post, "http://127.0.0.1:{port}/mcp", headers, body)
end
"##;

fn http_program(body: &str) -> String {
    prep(&format!("package main\n\nuse mcp\nuse ai\nuse json\nuse errors\nuse text\nuse web\n{SETUP}\n{HTTP_SETUP}\nfunc main() -> error\n    let s = make_server()\n    let up = try start_http(s)\n    let url = \"http://127.0.0.1:{{up.port}}/mcp\"\n{body}\n    return\nend\n"))
}

#[test]
fn the_client_works_against_the_http_server_and_remote_tools_are_ai_tools() {
    let body = "    let c = try mcp.connect(url, \"\")\n    let tools = try c.tools()\n    print(tools.len, tools[0].name)\n    print(try c.call(\"find\", ‹{\"name\":\"Zé\"}›))\n    match c.call(\"fail\", ‹{\"name\":\"x\"}›)\n        ok(t) -> print(\"unexpected\")\n        err(e) -> print(e.message)\n    end\n    print(try c.read(\"db://schema\"))\n    print(try c.read(\"db://rows/orders\"))\n    let rs = try c.resources()\n    print(rs.len, rs[0].uri)\n    var args: Map<text, text> = {}\n    args[\"who\"] = \"Bia\"\n    let msgs = try c.prompt(\"greet\", args)\n    print(msgs[0].content)\n    let remote = try c.ai_tools()\n    print(remote.len, remote[0].name)\n    print(try remote[0].call(‹{\"name\":\"remote\"}›))\n    c.close()\n    match mcp.connect(url, \"\")\n        ok(again) -> print(\"reconnected\")\n        err(e) -> print(e.message)\n    end\n";
    assert_eq!(run(&http_program(body)), "2 find\nfound Zé\nit broke\nCREATE TABLE t\nrows of orders\n1 db://schema\nHello Bia\n2 find\nfound remote\nreconnected\n");
}

#[test]
fn sessions_origins_tokens_and_limits_are_enforced_over_http() {
    let body = "    var h: Map<text, text> = {}\n    h[\"Content-Type\"] = \"application/json\"\n    let init = try post_raw(up.port, h, INIT_JSON)\n    let sid = init.header(\"mcp-session-id\") ?? \"\"\n    print(init.status, sid.len == 32)\n    var h2 = h\n    h2[\"Mcp-Session-Id\"] = sid\n    let list = try post_raw(up.port, h2, ‹{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}›)\n    print(list.status, text.contains(list.body, \"find\"))\n    var h3 = h\n    h3[\"Mcp-Session-Id\"] = \"deadbeef\"\n    print((try post_raw(up.port, h3, ‹{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\"}›)).status)\n    print((try post_raw(up.port, h, ‹{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\"}›)).status)\n    let note = try post_raw(up.port, h2, ‹{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}›)\n    print(note.status)\n    var evil = h2\n    evil[\"Origin\"] = \"http://evil.example\"\n    print((try post_raw(up.port, evil, ‹{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/list\"}›)).status)\n    var same = h2\n    same[\"Origin\"] = \"http://127.0.0.1:{up.port}\"\n    print((try post_raw(up.port, same, ‹{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/list\"}›)).status)\n    var badv = h2\n    badv[\"MCP-Protocol-Version\"] = \"1999-01-01\"\n    print((try post_raw(up.port, badv, ‹{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/list\"}›)).status)\n    print((try web.get(url)).status)\n    let gone = try web.request(web.Delete, url, h2, \"\")\n    print(gone.status, (try post_raw(up.port, h2, ‹{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/list\"}›)).status)\n";
    let src = http_program(body).replace("func main() -> error", &prep(&format!("let INIT_JSON = {}\n\nfunc main() -> error", INIT_MSG)));
    assert_eq!(run(&src), "200 true\n200 true\n404\n400\n202\n403\n200\n400\n405\n200 404\n");
}

#[test]
fn a_bearer_token_is_required_and_compared_by_value() {
    let body = "    s.bearer_token = \"secret\"\n    var app = web.server(port: 0)\n    s.serve_http(app, \"/mcp\")\n    let guarded = try app.start()\n    let u = \"http://127.0.0.1:{guarded.port}/mcp\"\n    var h: Map<text, text> = {}\n    h[\"Content-Type\"] = \"application/json\"\n    let none = try web.request(web.Post, u, h, INIT_JSON)\n    print(none.status, none.header(\"www-authenticate\") ?? \"-\")\n    h[\"Authorization\"] = \"Bearer wrong\"\n    print((try web.request(web.Post, u, h, INIT_JSON)).status)\n    h[\"Authorization\"] = \"Bearer secret\"\n    print((try web.request(web.Post, u, h, INIT_JSON)).status)\n    match mcp.connect(u, \"nope\")\n        ok(c) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name)\n    end\n    let good = try mcp.connect(u, \"secret\")\n    print((try good.tools()).len)\n";
    let src = http_program(body).replace("func main() -> error", &prep(&format!("let INIT_JSON = {}\n\nfunc main() -> error", INIT_MSG)));
    assert_eq!(run(&src), "401 Bearer\n401\n200\nstd.Unauthenticated\n2\n");
}

#[test]
fn an_oversized_message_is_refused_without_running_anything() {
    let body = "    s.max_message = 200\n    var app = web.server(port: 0)\n    s.serve_http(app, \"/mcp\")\n    let small = try app.start()\n    var h: Map<text, text> = {}\n    h[\"Content-Type\"] = \"application/json\"\n    let r = try web.request(web.Post, \"http://127.0.0.1:{small.port}/mcp\", h, text.repeat(\"x\", 500))\n    print(r.status, text.contains(r.body, \"larger than 200\"))\n";
    assert_eq!(run(&http_program(body)), "413 true\n");
}

#[test]
fn the_mcp_example_builds() {
    let src = std::fs::read_to_string("examples/mcp_server.br").unwrap();
    let exe = build(&src).unwrap_or_else(|e| panic!("examples/mcp_server.br: {}", e));
    let _ = std::fs::remove_file(exe);
}
