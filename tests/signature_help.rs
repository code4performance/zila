//! `textDocument/signatureHelp`: the callee's parameters with the current one marked.

use std::collections::HashMap;
use zila::lsp::{handle, parse_json, Json};

fn send(docs: &mut HashMap<String, String>, msg: &str) -> Vec<Json> {
    let mut out: Vec<u8> = Vec::new();
    let mut shutdown = false;
    handle(&parse_json(msg).unwrap(), docs, &mut out, &mut shutdown);
    let text = String::from_utf8(out).unwrap();
    text.split("Content-Length: ")
        .skip(1)
        .map(|part| parse_json(part.split_once("\r\n\r\n").unwrap().1).unwrap())
        .collect()
}

fn open(docs: &mut HashMap<String, String>, uri: &str, text: &str) {
    let mut j = String::new();
    Json::Str(text.into()).write(&mut j);
    send(docs, &format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","text":{}}}}}}}"#, uri, j));
}

fn help(docs: &mut HashMap<String, String>, uri: &str, line: i64, character: i64) -> Json {
    let msg = format!(r#"{{"jsonrpc":"2.0","id":4,"method":"textDocument/signatureHelp","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":{},"character":{}}}}}}}"#, uri, line, character);
    send(docs, &msg).remove(0).get("result").clone()
}

/// (label of the signature, labels of its parameters, the active parameter)
fn shown(h: &Json) -> (String, Vec<String>, i64) {
    let sig = match h.get("signatures") {
        Json::Arr(v) => v[0].clone(),
        other => panic!("no signatures in {:?}", other),
    };
    let Json::Arr(ps) = sig.get("parameters") else { panic!() };
    let Json::Num(a) = h.get("activeParameter") else { panic!() };
    (sig.get("label").str().unwrap().to_string(), ps.iter().map(|p| p.get("label").str().unwrap().to_string()).collect(), *a as i64)
}

/// A program whose `main` holds `body` (each line indented), `body` starting at line 14.
fn program(body: &str) -> String {
    let lines: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\ntype Point = record\n    x: int\nend\n\n// Adds three things.\nfunc add(a: int, b: int, label: text) -> int\n    return a + b\nend\n\nfunc (p: Point) scale(k: int, m: int) -> int\n    return p.x * k * m\nend\n\nfunc main()\n    let p = Point{{x: 1}}\n{}end\n", lines)
}

const FIRST_BODY_LINE: i64 = 17;

#[test]
fn the_parameter_the_cursor_is_at_is_marked() {
    let mut docs = HashMap::new();
    let uri = "file:///s.zila";
    // `    add(1, 2, ` and its prefixes: the cursor is at the end of line FIRST_BODY_LINE
    for (typed, active) in [("add(", 0), ("add(1", 0), ("add(1,", 1), ("add(1, 2", 1), ("add(1, 2,", 2), ("add(1, 2, \"x\"", 2)] {
        open(&mut docs, uri, &program(typed));
        let h = help(&mut docs, uri, FIRST_BODY_LINE, 4 + typed.chars().count() as i64);
        let (label, params, got) = shown(&h);
        assert_eq!(label, "func add(a: int, b: int, label: text) -> int", "after `{}`", typed);
        assert_eq!(params, ["a: int", "b: int", "label: text"]);
        assert_eq!(got, active, "after `{}`", typed);
    }
}

#[test]
fn the_doc_comment_comes_with_the_signature() {
    let mut docs = HashMap::new();
    open(&mut docs, "file:///d.zila", &program("add("));
    let h = help(&mut docs, "file:///d.zila", FIRST_BODY_LINE, 8);
    let Json::Arr(sigs) = h.get("signatures") else { panic!() };
    assert_eq!(sigs[0].get("documentation").str(), Some("Adds three things."));
}

#[test]
fn a_nested_call_is_the_innermost_and_a_call_over_several_lines_counts_across_them() {
    let mut docs = HashMap::new();
    let uri = "file:///n.zila";
    open(&mut docs, uri, &program("add(1, add(2, "));
    let (label, _, active) = shown(&help(&mut docs, uri, FIRST_BODY_LINE, 4 + 14));
    assert_eq!((label.as_str(), active), ("func add(a: int, b: int, label: text) -> int", 1));
    // after the inner call is closed the outer one is the call again
    open(&mut docs, uri, &program("add(1, add(2, 3, \"y\"), "));
    let (_, _, active) = shown(&help(&mut docs, uri, FIRST_BODY_LINE, 4 + 23));
    assert_eq!(active, 2);
    // the arguments on several lines
    open(&mut docs, uri, &program("add(1,\n    2,\n    "));
    let (_, _, active) = shown(&help(&mut docs, uri, FIRST_BODY_LINE + 2, 8));
    assert_eq!(active, 2);
}

#[test]
fn a_method_call_shows_the_method() {
    let mut docs = HashMap::new();
    let uri = "file:///m.zila";
    open(&mut docs, uri, &program("p.scale(2, "));
    let (label, params, active) = shown(&help(&mut docs, uri, FIRST_BODY_LINE, 4 + 11));
    assert_eq!(label, "func scale(k: int, m: int) -> int");
    assert_eq!((params.len(), active), (2, 1));
}

#[test]
fn a_package_function_shows_its_signature() {
    let dir = std::env::temp_dir().join(format!("zila_sig_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("util")).unwrap();
    std::fs::write(dir.join("zila.toml"), "[package]\nname = \"app\"\n").unwrap();
    std::fs::write(dir.join("util/util.zila"), "package util\n\npub func twice(n: int) -> int\n    return n * 2\nend\n").unwrap();
    let p = dir.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    let uri = if p.starts_with('/') { format!("file://{}/main.zila", p) } else { format!("file:///{}/main.zila", p) };
    let mut docs = HashMap::new();
    open(&mut docs, &uri, "package main\n\nuse util\n\nfunc main()\n    util.twice(\nend\n");
    let (label, params, active) = shown(&help(&mut docs, &uri, 5, 15));
    assert_eq!((label.as_str(), params.len(), active), ("func twice(n: int) -> int", 1, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn outside_a_call_or_for_an_unknown_callee_the_answer_is_null() {
    let mut docs = HashMap::new();
    let uri = "file:///o.zila";
    open(&mut docs, uri, &program("let n = 5\nif (n == 5)\nend\nmissing(1, "));
    // not in a call
    assert_eq!(help(&mut docs, uri, FIRST_BODY_LINE, 6), Json::Null);
    // inside the grouping parentheses of an `if`
    assert_eq!(help(&mut docs, uri, FIRST_BODY_LINE + 1, 11), Json::Null);
    // a callee that does not exist
    assert_eq!(help(&mut docs, uri, FIRST_BODY_LINE + 3, 15), Json::Null);
    // a document that is not open
    assert_eq!(help(&mut docs, "file:///none.zila", 0, 0), Json::Null);
}
