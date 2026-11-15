//! The language server: JSON, syntax-error diagnostics and formatting, driven message by message
//! (no process is started).

use zila::lsp::{handle, parse_json, Json};
use std::collections::HashMap;

/// Sends one message and returns the JSON bodies the server wrote back.
fn send(docs: &mut HashMap<String, String>, msg: &str) -> Vec<Json> {
    let mut out: Vec<u8> = Vec::new();
    let mut shutdown = false;
    handle(&parse_json(msg).unwrap(), docs, &mut out, &mut shutdown);
    let text = String::from_utf8(out).unwrap();
    text.split("Content-Length: ")
        .skip(1)
        .map(|part| {
            let body = part.split_once("\r\n\r\n").unwrap().1;
            parse_json(body).unwrap()
        })
        .collect()
}

fn open(docs: &mut HashMap<String, String>, uri: &str, text: &str) -> Vec<Json> {
    let mut j = String::new();
    Json::Str(text.into()).write(&mut j);
    send(docs, &format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","text":{}}}}}}}"#, uri, j))
}

#[test]
fn json_round_trip() {
    let v = parse_json(r#"{"a":[1,2.5,"x\n\u00e9\ud83d\ude00"],"b":{"c":null,"d":true}}"#).unwrap();
    assert_eq!(v.get("b").get("d"), &Json::Bool(true));
    let mut s = String::new();
    v.write(&mut s);
    assert_eq!(parse_json(&s).unwrap(), v);
    assert!(parse_json("{\"a\":}").is_err());
}

#[test]
fn initialize_announces_the_features() {
    let mut docs = HashMap::new();
    let r = send(&mut docs, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let caps = r[0].get("result").get("capabilities");
    assert_eq!(caps.get("documentFormattingProvider"), &Json::Bool(true));
    assert_eq!(r[0].get("id"), &Json::Num(1.0));
}

#[test]
fn a_good_file_has_no_diagnostics_and_a_bad_one_has_one() {
    let mut docs = HashMap::new();
    let r = open(&mut docs, "file:///a.zila", "package main\n\nfunc main()\nend\n");
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
    let r = open(&mut docs, "file:///b.zila", "package main\n\nfunc main()\n    let = 3\nend\n");
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(3.0));
    assert_eq!(d[0].get("severity"), &Json::Num(1.0));
    assert!(d[0].get("message").str().unwrap().len() > 3);
}

#[test]
fn editing_republishes_and_formatting_returns_one_edit() {
    let mut docs = HashMap::new();
    open(&mut docs, "file:///c.zila", "package main\n\nfunc main()\nend\n");
    let r = send(
        &mut docs,
        r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///c.zila"},"contentChanges":[{"text":"package main\n\nfunc main()\nprintln(1,2)\nend\n"}]}}"#,
    );
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
    let r = send(&mut docs, r#"{"jsonrpc":"2.0","id":7,"method":"textDocument/formatting","params":{"textDocument":{"uri":"file:///c.zila"}}}"#);
    let Json::Arr(edits) = r[0].get("result") else { panic!() };
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].get("newText").str(), Some("package main\n\nfunc main()\n    println(1, 2)\nend\n"));
    assert_eq!(edits[0].get("range").get("end").get("line"), &Json::Num(5.0));
    // an already formatted file needs no edit
    open(&mut docs, "file:///d.zila", "package main\n\nfunc main()\nend\n");
    let r = send(&mut docs, r#"{"jsonrpc":"2.0","id":8,"method":"textDocument/formatting","params":{"textDocument":{"uri":"file:///d.zila"}}}"#);
    assert_eq!(r[0].get("result"), &Json::Arr(vec![]));
}

#[test]
fn shutdown_and_exit() {
    let mut docs = HashMap::new();
    let mut out: Vec<u8> = Vec::new();
    let mut shutdown = false;
    assert!(handle(&parse_json(r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#).unwrap(), &mut docs, &mut out, &mut shutdown));
    assert!(shutdown);
    assert!(!handle(&parse_json(r#"{"jsonrpc":"2.0","method":"exit"}"#).unwrap(), &mut docs, &mut out, &mut shutdown));
}

#[test]
fn a_no_alloc_violation_is_a_diagnostic_on_its_line() {
    let mut docs = HashMap::new();
    let src = "package main

#[no_alloc]
func f(n: int) -> text
    return \"n = {n}\"
end

func main()
end
";
    let r = open(&mut docs, "file:///na.zila", src);
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(3.0));
    let m = d[0].get("message").str().unwrap();
    assert!(m.contains("'f' is #[no_alloc] but allocates") && m.contains("text interpolation"), "{}", m);
}

#[test]
fn block_content_is_never_parsed_and_generators_never_run() {
    let mut docs = HashMap::new();
    let src = "package main

use reflect

#[derive(lsp_never_runs)]
type Point = record
    x: int
end

#[derive]
comptime func lsp_never_runs(t: Type) -> text
    return \"\"
end

#[block]
comptime func raw_text(raw: text) -> text
    return raw
end

func main()
    let s = raw_text
        {{ this ) is ] not Zila Lang ((
    end
    print(s)
end
";
    let r = open(&mut docs, "file:///blocks.zila", src);
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
    assert!(!zila::comptime::launch_log().iter().any(|l| l.starts_with("lsp_never_runs(")));
    // an error outside the block is still reported
    let r = open(&mut docs, "file:///bad.zila", &src.replace("print(s)", "print(s"));
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
}

#[test]
fn indexing_a_text_is_a_diagnostic_that_points_to_the_alternatives() {
    let mut docs = HashMap::new();
    let src = "package main\n\nfunc main()\n    let s = \"ação\"\n    let b = s[0]\nend\n";
    let r = open(&mut docs, "file:///idx.zila", src);
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(4.0));
    let m = d[0].get("message").str().unwrap();
    assert!(m.contains("text cannot be indexed") && m.contains("bytes()") && m.contains("slice") && m.contains("for c in s"), "{}", m);
}

#[test]
fn a_non_transferable_spawn_thread_argument_is_a_diagnostic() {
    let mut docs = HashMap::new();
    let src = "package main\n\nuse thread\n\ntype Node = record\n    value: int\nend\n\nfunc show(n: Rc<Node>)\nend\n\nfunc main()\n    let n = new Node{ value: 1 }\n    spawn_thread show(n)\nend\n";
    let r = open(&mut docs, "file:///thr.zila", src);
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    let m = d[0].get("message").str().unwrap();
    assert!(m.contains("argument 'n'") && m.contains("not transferable") && m.contains("Mutex"), "{}", m);
}

#[test]
fn pub_declarations_have_no_diagnostics_and_a_misplaced_pub_is_reported_on_its_line() {
    let mut docs = HashMap::new();
    let good = "package lib\n\npub type P = record\n    pub x: int\n    y: int\nend\n\n#[export(\"f\")]\npub func f() -> int\n    return 1\nend\n\npub async func g()\nend\n\npub let K = 1\n";
    let r = open(&mut docs, "file:///pub.zila", good);
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
    let r = open(&mut docs, "file:///bad.zila", "package lib\n\npub embed \"x\"\n");
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(2.0));
    assert!(d[0].get("message").str().unwrap().contains("'pub' cannot be applied to 'embed'"));
}

#[test]
fn packages_of_the_project_are_seen_by_the_editor() {
    // a project with a local package: names resolve, a block of the package is a block, and a
    // name found nowhere is reported with the hint
    let dir = std::env::temp_dir().join(format!("zila_lsp_pkg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("util")).unwrap();
    std::fs::write(dir.join("zila.toml"), "[package]\nname = \"app\"\n").unwrap();
    std::fs::write(dir.join("util/util.zila"), "package util\n\npub func twice(n: int) -> int\n    return n * 2\nend\n\n#[block]\npub comptime func raw_text(raw: text) -> text\n    return raw\nend\n").unwrap();
    let p = dir.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    let uri = |f: &str| if p.starts_with('/') { format!("file://{}/{}", p, f) } else { format!("file:///{}/{}", p, f) };
    let mut docs = HashMap::new();
    let good = "package main\n\nuse util\n\nfunc main()\n    print(\"{util.twice(2)}\")\nend\n";
    let r = open(&mut docs, &uri("main.zila"), good);
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]), "a package of the project resolves");
    let blocks = "package main\n\nuse util\n\nfunc main()\n    let s = raw_text\n        {{ not ) Zila Lang ((\n    end\n    print(s)\nend\n";
    let r = open(&mut docs, &uri("blocks.zila"), blocks);
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]), "a block of a package of the project");
    let r = open(&mut docs, &uri("bad.zila"), &good.replace("use util", "use nothing"));
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    let m = d[0].get("message").str().unwrap();
    assert!(m.contains("unknown package 'nothing'") && m.contains("--add"), "{}", m);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sql_block_has_no_diagnostics_and_syntax_errors_around_it_are_reported() {
    let mut docs = HashMap::new();
    let good = "package main\n\nuse db\nuse postgres\n\nfunc main()\n    let limit = 5\n    let q = sql postgres\n        select id from customers\n        where id < {limit} and name = 'it''s {not a hole}'\n    end\n    print(q.text)\nend\n";
    let r = open(&mut docs, "file:///sql_good.zila", good);
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
    // a syntax error after the block is still reported on its own line, and the SQL text is not
    // taken for Zila Lang (the server does not compile files that use `db`, which has generators)
    let bad = good.replace("print(q.text)", "print(q.text");
    let r = open(&mut docs, "file:///sql_bad.zila", &bad);
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1, "{:?}", d);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(12.0));
    // a block that never ends is a syntax error at the line where it began
    let open_block = "package main

use db
use postgres

func main()
    let q = sql postgres
        select 1
";
    let r = open(&mut docs, "file:///sql_open.zila", open_block);
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1, "{:?}", d);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(6.0));
    // an ordinary identifier called sql is not a block
    let r = open(&mut docs, "file:///sql_ident.zila", "package main\n\nfunc f(sql: text) -> text\n    return sql\nend\n\nfunc main()\n    print(f(\"x\"))\nend\n");
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
}

fn change(docs: &mut HashMap<String, String>, uri: &str, text: &str) -> Vec<Json> {
    let mut j = String::new();
    Json::Str(text.into()).write(&mut j);
    send(docs, &format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{}"}},"contentChanges":[{{"text":{}}}]}}}}"#, uri, j))
}

fn published<'a>(msgs: &'a [Json], uri: &str) -> &'a Json {
    msgs.iter().find(|m| m.get("params").get("uri").str() == Some(uri)).unwrap_or_else(|| panic!("nothing published for {}", uri)).get("params").get("diagnostics")
}

#[test]
fn a_type_error_is_one_error_on_the_expression_and_goes_away_when_fixed() {
    let mut docs = HashMap::new();
    let uri = "file:///t.zila";
    // line 4 (index 3): a text where an int is wanted
    let r = open(&mut docs, uri, "package main\n\nfunc main()\n    let x: int = \"text\"\nend\n");
    let Json::Arr(d) = published(&r, uri) else { panic!() };
    assert_eq!(d.len(), 1, "{:?}", d);
    assert_eq!(d[0].get("severity"), &Json::Num(1.0));
    assert_eq!(d[0].get("source").str(), Some("zila"));
    let (start, end) = (d[0].get("range").get("start"), d[0].get("range").get("end"));
    assert_eq!((start.get("line"), end.get("line")), (&Json::Num(3.0), &Json::Num(3.0)));
    // the range is the text literal: `    let x: int = "text"` has it at characters 17 to 23
    assert_eq!((start.get("character"), end.get("character")), (&Json::Num(17.0), &Json::Num(23.0)));
    let r = change(&mut docs, uri, "package main\n\nfunc main()\n    let x: int = 4\nend\n");
    assert_eq!(published(&r, uri), &Json::Arr(vec![]));
}

#[test]
fn an_edit_of_another_open_file_changes_the_diagnostics_of_the_first_without_saving() {
    let dir = std::env::temp_dir().join(format!("zila_lsp_other_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("util")).unwrap();
    std::fs::write(dir.join("zila.toml"), "[package]\nname = \"app\"\n").unwrap();
    let util_ok = "package util\n\npub func twice(n: int) -> int\n    return n * 2\nend\n";
    std::fs::write(dir.join("util/util.zila"), util_ok).unwrap();
    let p = dir.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    let uri = |f: &str| if p.starts_with('/') { format!("file://{}/{}", p, f) } else { format!("file:///{}/{}", p, f) };
    let (main_uri, util_uri) = (uri("main.zila"), uri("util/util.zila"));
    let mut docs = HashMap::new();
    let r = open(&mut docs, &util_uri, util_ok);
    assert_eq!(published(&r, &util_uri), &Json::Arr(vec![]));
    let main = "package main\n\nuse util\n\nfunc main()\n    print(\"{util.twice(2)}\")\nend\n";
    let r = open(&mut docs, &main_uri, main);
    assert_eq!(published(&r, &main_uri), &Json::Arr(vec![]));
    // rename the function in the other file; nothing is saved
    let r = change(&mut docs, &util_uri, &util_ok.replace("twice", "double"));
    let Json::Arr(d) = published(&r, &main_uri) else { panic!() };
    assert_eq!(d.len(), 1, "{:?}", d);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(5.0));
    assert_eq!(std::fs::read_to_string(dir.join("util/util.zila")).unwrap(), util_ok, "the file on disk is untouched");
    // renaming back clears it
    let r = change(&mut docs, &util_uri, util_ok);
    assert_eq!(published(&r, &main_uri), &Json::Arr(vec![]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_warning_of_the_compiler_is_published_with_severity_2() {
    // a package of the project that takes the name of a standard library that is not core
    let dir = std::env::temp_dir().join(format!("zila_lsp_warn_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(dir.join("zila.toml"), "[package]\nname = \"app\"\n").unwrap();
    std::fs::write(dir.join("ui/ui.zila"), "package ui\n\npub func one() -> int\n    return 1\nend\n").unwrap();
    let p = dir.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    let uri = if p.starts_with('/') { format!("file://{}/main.zila", p) } else { format!("file:///{}/main.zila", p) };
    let mut docs = HashMap::new();
    let r = open(&mut docs, &uri, "package main\n\nuse ui\n\nfunc main()\n    print(\"{ui.one()}\")\nend\n");
    let Json::Arr(d) = published(&r, &uri) else { panic!() };
    assert_eq!(d.len(), 1, "{:?}", d);
    assert_eq!(d[0].get("severity"), &Json::Num(2.0));
    assert!(d[0].get("message").str().unwrap().contains("shadows the standard library"), "{:?}", d);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_cut_anywhere_gets_only_its_syntax_error_and_never_stops_the_server() {
    let mut tried = 0;
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "zila") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let chars: Vec<usize> = src.char_indices().map(|(i, _)| i).collect();
        for pct in [15, 35, 55, 80] {
            let text = &src[..chars[chars.len() * pct / 100]];
            let mut docs = HashMap::new();
            let r = open(&mut docs, "file:///cut.zila", text);
            let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
            tried += 1;
            if zila::parse_source(text).is_err() {
                assert_eq!(d.len(), 1, "{} at {}%: {:?}", path.display(), pct, d);
                assert_eq!(d[0].get("severity"), &Json::Num(1.0));
            }
        }
    }
    assert!(tried > 100, "{}", tried);
}

#[test]
fn rapid_edits_publish_only_the_diagnostics_of_the_last_text() {
    use std::time::Duration;
    let uri = "file:///fast.zila";
    let msg = move |method: &str, text: &str| {
        let mut j = String::new();
        Json::Str(text.into()).write(&mut j);
        let doc = if method == "didOpen" { format!(r#"{{"uri":"{}","text":{}}}"#, uri, j) } else { format!(r#"{{"uri":"{}"}}"#, uri) };
        let changes = if method == "didChange" { format!(r#","contentChanges":[{{"text":{}}}]"#, j) } else { String::new() };
        parse_json(&format!(r#"{{"jsonrpc":"2.0","method":"textDocument/{}","params":{{"textDocument":{}{}}}}}"#, method, doc, changes)).unwrap()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let sender = std::thread::spawn(move || {
        tx.send(msg("didOpen", "package main\n\nfunc main()\nend\n")).unwrap();
        // five edits one after the other, the first four wrong in different ways, the last with a type error
        for text in ["package main\n\nfunc main()\n    let = 1\n", "package main\n\nfunc main(\n", "pack", "package main\n\nfunc main()\n    let a: int = 1\nend\n"] {
            tx.send(msg("didChange", text)).unwrap();
        }
        tx.send(msg("didChange", "package main\n\nfunc main()\n    let a: int = \"x\"\nend\n")).unwrap();
        std::thread::sleep(Duration::from_millis(800));
        tx.send(parse_json(r#"{"jsonrpc":"2.0","id":1,"method":"shutdown"}"#).unwrap()).unwrap();
        tx.send(parse_json(r#"{"jsonrpc":"2.0","method":"exit"}"#).unwrap()).unwrap();
    });
    let mut out: Vec<u8> = Vec::new();
    let code = zila::lsp::serve(rx, &mut out, Duration::from_millis(100));
    sender.join().unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    let msgs: Vec<Json> = text.split("Content-Length: ").skip(1).map(|p| parse_json(p.split_once("\r\n\r\n").unwrap().1).unwrap()).collect();
    let published: Vec<&Json> = msgs.iter().filter(|m| m.get("method").str() == Some("textDocument/publishDiagnostics")).collect();
    assert_eq!(published.len(), 1, "{} diagnostics were published", published.len());
    let Json::Arr(d) = published[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1, "{:?}", d);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(3.0), "the error of the last text");
}

fn hover(docs: &mut HashMap<String, String>, uri: &str, line: i64, character: i64) -> Json {
    let msg = format!(r#"{{"jsonrpc":"2.0","id":9,"method":"textDocument/hover","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":{},"character":{}}}}}}}"#, uri, line, character);
    send(docs, &msg).remove(0).get("result").clone()
}

#[test]
fn hovering_a_function_shows_its_signature_and_doc_comment() {
    let mut docs = HashMap::new();
    let uri = "file:///h.zila";
    let src = "package main\n\n// Adds one to n.\n// Second line.\n#[export(\"inc\")]\nfunc inc(n: int) -> int\n    return n + 1\nend\n\nfunc main()\n    let x = inc(1)\n    print(\"{x}\")\nend\n";
    open(&mut docs, uri, src);
    // on the call `inc(1)`: line 10, `    let x = inc(1)` has `inc` at characters 12 to 15
    let h = hover(&mut docs, uri, 10, 13);
    let value = h.get("contents").get("value").str().unwrap().to_string();
    assert!(value.contains("func inc(n: int) -> int"), "{}", value);
    assert!(value.contains("Adds one to n.\nSecond line."), "{}", value);
    assert_eq!(h.get("range").get("start").get("character"), &Json::Num(12.0));
    assert_eq!(h.get("range").get("end").get("character"), &Json::Num(15.0));
    // on the declaration itself
    let h = hover(&mut docs, uri, 5, 6);
    assert!(h.get("contents").get("value").str().unwrap().contains("Adds one to n."));
    // a local, in the text interpolation: its type
    let h = hover(&mut docs, uri, 11, 12);
    assert!(h.get("contents").get("value").str().unwrap().contains("x: int"), "{:?}", h);
    // a parameter in its body
    let h = hover(&mut docs, uri, 6, 11);
    assert!(h.get("contents").get("value").str().unwrap().contains("n: int"), "{:?}", h);
}

#[test]
fn hover_answers_null_where_there_is_nothing_to_show() {
    let mut docs = HashMap::new();
    let uri = "file:///n.zila";
    open(&mut docs, uri, "package main\n\nfunc main()\n    let x = 1\n    print(\"{x}\")\nend\n");
    assert_eq!(hover(&mut docs, uri, 3, 1), Json::Null, "indentation");
    assert_eq!(hover(&mut docs, uri, 0, 3), Json::Null, "a keyword");
    assert_eq!(hover(&mut docs, uri, 40, 0), Json::Null, "past the end");
    // a text with a syntax error: the broken statement shows nothing, the rest still does
    open(&mut docs, "file:///b.zila", "package main\n\nfunc main()\n    let = 1\n    let y = 2\n    print(\"{y}\")\nend\n");
    assert_eq!(hover(&mut docs, "file:///b.zila", 3, 6), Json::Null);
    assert!(hover(&mut docs, "file:///b.zila", 5, 12).get("contents").get("value").str().unwrap().contains("y: int"));
    // a document that is not open
    assert_eq!(hover(&mut docs, "file:///closed.zila", 0, 0), Json::Null);
}

#[test]
fn hover_follows_an_unsaved_edit() {
    let mut docs = HashMap::new();
    let uri = "file:///e.zila";
    open(&mut docs, uri, "package main\n\nfunc main()\n    let x = 1\n    print(\"{x}\")\nend\n");
    assert!(hover(&mut docs, uri, 4, 12).get("contents").get("value").str().unwrap().contains("x: int"));
    change(&mut docs, uri, "package main\n\nfunc main()\n    let x = \"one\"\n    print(\"{x}\")\nend\n");
    assert!(hover(&mut docs, uri, 4, 12).get("contents").get("value").str().unwrap().contains("x: text"));
}
