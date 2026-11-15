//! Definition and references across files and packages, through the language server.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use zila::lsp::{handle, parse_json, Json};

fn send(docs: &mut HashMap<String, String>, msg: &str) -> Vec<Json> {
    let mut out: Vec<u8> = Vec::new();
    let mut shutdown = false;
    handle(&parse_json(msg).unwrap(), docs, &mut out, &mut shutdown);
    let text = String::from_utf8(out).unwrap();
    text.split("Content-Length: ").skip(1).map(|part| parse_json(part.split_once("\r\n\r\n").unwrap().1).unwrap()).collect()
}

fn open(docs: &mut HashMap<String, String>, uri: &str, text: &str) {
    let mut j = String::new();
    Json::Str(text.into()).write(&mut j);
    send(docs, &format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","text":{}}}}}}}"#, uri, j));
}

fn request(docs: &mut HashMap<String, String>, method: &str, uri: &str, line: i64, character: i64, extra: &str) -> Json {
    let msg = format!(r#"{{"jsonrpc":"2.0","id":5,"method":"{}","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":{},"character":{}}}{}}}}}"#, method, uri, line, character, extra);
    send(docs, &msg).remove(0).get("result").clone()
}

static N: AtomicUsize = AtomicUsize::new(0);

/// A project with a package `util` of two files, and its `file://` base.
fn project() -> (PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("zila_nav_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("util")).unwrap();
    std::fs::write(dir.join("zila.toml"), "[package]\nname = \"app\"\n").unwrap();
    // `twice` is in the second file, so that the merged text and the files have different lines
    std::fs::write(dir.join("util/a.zila"), "package util\n\n// First file.\npub func one() -> int\n    return 1\nend\n").unwrap();
    std::fs::write(dir.join("util/b.zila"), "package util\n\n// Doubles n.\npub func twice(n: int) -> int\n    return n * 2\nend\n").unwrap();
    let p = dir.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    let base = if p.starts_with('/') { format!("file://{}", p) } else { format!("file:///{}", p) };
    (dir, base)
}

const MAIN: &str = "package main\n\nuse util\nuse text\n\nfunc helper(n: int) -> int\n    return util.twice(n)\nend\n\nfunc main()\n    let a = helper(1)\n    let b = helper(a)\n    print(\"{util.twice(b)} {text.contains(\"abc\", \"b\")}\")\nend\n";

fn line_of(loc: &Json) -> f64 {
    let Json::Num(l) = loc.get("range").get("start").get("line") else { panic!("{:?}", loc) };
    *l
}

#[test]
fn definition_of_a_function_of_another_file_of_a_package_gives_that_file_and_its_line() {
    let (dir, base) = project();
    let uri = format!("{}/main.zila", base);
    let mut docs = HashMap::new();
    open(&mut docs, &uri, MAIN);
    // `util.twice(n)` on line 6: `    return util.twice(n)`, `twice` at characters 16 to 21
    let loc = request(&mut docs, "textDocument/definition", &uri, 6, 17, "");
    assert_eq!(loc.get("uri").str(), Some(format!("{}/util/b.zila", base).as_str()), "{:?}", loc);
    assert_eq!(line_of(&loc), 3.0, "`pub func twice` is on line 4 of b.zila");
    assert_eq!(loc.get("range").get("start").get("character"), &Json::Num(9.0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn definition_in_the_same_document_and_of_a_local() {
    let (dir, base) = project();
    let uri = format!("{}/main.zila", base);
    let mut docs = HashMap::new();
    open(&mut docs, &uri, MAIN);
    // `helper(1)` on line 10
    let loc = request(&mut docs, "textDocument/definition", &uri, 10, 13, "");
    assert_eq!(loc.get("uri").str(), Some(uri.as_str()));
    assert_eq!(line_of(&loc), 5.0);
    // the parameter `n` used on line 6 is declared on line 5
    let loc = request(&mut docs, "textDocument/definition", &uri, 6, 22, "");
    assert_eq!((loc.get("uri").str(), line_of(&loc)), (Some(uri.as_str()), 5.0), "{:?}", loc);
    // nothing to go to on a keyword or a built-in
    assert_eq!(request(&mut docs, "textDocument/definition", &uri, 12, 8, ""), Json::Null, "print");
    assert_eq!(request(&mut docs, "textDocument/definition", &uri, 9, 1, ""), Json::Null, "func");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn definition_in_the_standard_library_opens_a_copy_of_its_source() {
    let (dir, base) = project();
    let uri = format!("{}/main.zila", base);
    let mut docs = HashMap::new();
    open(&mut docs, &uri, MAIN);
    // `text.contains(...)` on line 12; `contains` starts at character 38 (after `print("{util.twice(b)} {text.`)
    let line = MAIN.lines().nth(12).unwrap();
    let at = line.find("contains").unwrap() as i64 + 2;
    let loc = request(&mut docs, "textDocument/definition", &uri, 12, at, "");
    let target = loc.get("uri").str().unwrap_or_else(|| panic!("{:?}", loc)).to_string();
    assert!(target.ends_with("/zila-std/text.zila"), "{}", target);
    let path = PathBuf::from(target.trim_start_matches("file:///").replace("%20", " "));
    let src = std::fs::read_to_string(&path).or_else(|_| std::fs::read_to_string(format!("/{}", path.display()))).unwrap();
    let l = line_of(&loc) as usize;
    assert!(src.lines().nth(l).unwrap().starts_with("pub func contains("), "line {}: {}", l, src.lines().nth(l).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn references_are_every_use_in_the_program_with_and_without_the_declaration() {
    let (dir, base) = project();
    let uri = format!("{}/main.zila", base);
    let mut docs = HashMap::new();
    open(&mut docs, &uri, MAIN);
    // `helper` is declared on line 5 and used on lines 10 and 11
    let with = request(&mut docs, "textDocument/references", &uri, 10, 13, r#","context":{"includeDeclaration":true}"#);
    let Json::Arr(v) = &with else { panic!("{:?}", with) };
    let mut lines: Vec<f64> = v.iter().map(line_of).collect();
    lines.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(lines, [5.0, 10.0, 11.0]);
    let without = request(&mut docs, "textDocument/references", &uri, 10, 13, r#","context":{"includeDeclaration":false}"#);
    let Json::Arr(v) = &without else { panic!() };
    assert_eq!(v.len(), 2);
    // `twice` is used in this file twice and declared in b.zila
    let r = request(&mut docs, "textDocument/references", &uri, 6, 17, r#","context":{"includeDeclaration":true}"#);
    let Json::Arr(v) = &r else { panic!("{:?}", r) };
    let in_b: Vec<&Json> = v.iter().filter(|l| l.get("uri").str().unwrap().ends_with("/util/b.zila")).collect();
    assert_eq!(v.len(), 3, "{:?}", v);
    assert_eq!(in_b.len(), 1);
    assert_eq!(line_of(in_b[0]), 3.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn definition_and_references_answer_null_outside_a_name_or_in_a_closed_document() {
    let mut docs = HashMap::new();
    assert_eq!(request(&mut docs, "textDocument/definition", "file:///none.zila", 0, 0, ""), Json::Null);
    assert_eq!(request(&mut docs, "textDocument/references", "file:///none.zila", 0, 0, ""), Json::Null);
}

fn document_symbols(docs: &mut HashMap<String, String>, uri: &str) -> Json {
    let msg = format!(r#"{{"jsonrpc":"2.0","id":6,"method":"textDocument/documentSymbol","params":{{"textDocument":{{"uri":"{}"}}}}}}"#, uri);
    send(docs, &msg).remove(0).get("result").clone()
}

const SHAPES: &str = "package main\n\ntype Point = record\n    x: int\n    y: int\nend\n\ntype Shape = enum\n    Circle(r: int)\n    Empty\nend\n\ntype Named = interface\n    func name(self) -> text\nend\n\nlet limit = 10\n\nfunc (p: Point) sum() -> int\n    return p.x + p.y\nend\n\nfunc main()\nend\n";

#[test]
fn a_document_lists_its_declarations_with_kinds_and_members() {
    let mut docs = HashMap::new();
    let uri = "file:///sym.zila";
    open(&mut docs, uri, SHAPES);
    let Json::Arr(top) = document_symbols(&mut docs, uri) else { panic!() };
    let summary: Vec<(String, f64, usize)> = top
        .iter()
        .map(|s| {
            let Json::Num(k) = s.get("kind") else { panic!() };
            let n = if let Json::Arr(c) = s.get("children") { c.len() } else { 0 };
            (s.get("name").str().unwrap().to_string(), *k, n)
        })
        .collect();
    // 23 struct, 10 enum, 11 interface, 13 variable, 6 method, 12 function
    assert_eq!(
        summary,
        [("Point".to_string(), 23.0, 2), ("Shape".to_string(), 10.0, 2), ("Named".to_string(), 11.0, 1), ("limit".to_string(), 13.0, 0), ("sum".to_string(), 6.0, 0), ("main".to_string(), 12.0, 0)]
    );
    let Json::Arr(fields) = top[0].get("children") else { panic!() };
    assert_eq!((fields[0].get("name").str(), fields[0].get("kind")), (Some("x"), &Json::Num(8.0)));
    assert_eq!(top[0].get("selectionRange").get("start").get("line"), &Json::Num(2.0));
    assert_eq!(document_symbols(&mut docs, "file:///none.zila"), Json::Null);
}

#[test]
fn the_workspace_lists_declarations_of_every_file_of_the_program_matching_the_query() {
    let (dir, base) = project();
    let uri = format!("{}/main.zila", base);
    let mut docs = HashMap::new();
    open(&mut docs, &uri, MAIN);
    let find = |docs: &mut HashMap<String, String>, q: &str| -> Vec<(String, String, f64)> {
        let msg = format!(r#"{{"jsonrpc":"2.0","id":7,"method":"workspace/symbol","params":{{"query":"{}"}}}}"#, q);
        let r = send(docs, &msg).remove(0).get("result").clone();
        let Json::Arr(v) = r else { panic!() };
        v.iter().map(|s| (s.get("name").str().unwrap().to_string(), s.get("location").get("uri").str().unwrap().to_string(), line_of(s.get("location")))).collect()
    };
    let r = find(&mut docs, "TWI");
    assert_eq!(r, [("twice".to_string(), format!("{}/util/b.zila", base), 3.0)]);
    let r = find(&mut docs, "helper");
    assert_eq!(r, [("helper".to_string(), uri.clone(), 5.0)]);
    let all: Vec<String> = find(&mut docs, "").into_iter().map(|x| x.0).collect();
    for n in ["helper", "main", "one", "twice"] {
        assert!(all.contains(&n.to_string()), "{} is missing from {:?}", n, all);
    }
    assert!(find(&mut docs, "nothing like this").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The edits of a rename: per uri, (line, start character, end character, new text).
type Edits = HashMap<String, Vec<(f64, f64, f64, String)>>;

/// Sends a rename; the answer is the edits per uri or the error message.
fn rename(docs: &mut HashMap<String, String>, uri: &str, line: i64, character: i64, new: &str) -> Result<Edits, String> {
    let msg = format!(r#"{{"jsonrpc":"2.0","id":8,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":{},"character":{}}},"newName":"{}"}}}}"#, uri, line, character, new);
    let r = send(docs, &msg).remove(0);
    if let Json::Obj(_) = r.get("error") {
        assert_eq!(r.get("result"), &Json::Null, "an error carries no edits");
        return Err(r.get("error").get("message").str().unwrap().to_string());
    }
    let Json::Obj(changes) = r.get("result").get("changes") else { panic!("{:?}", r) };
    let num = |j: &Json| match j {
        Json::Num(n) => *n,
        _ => panic!(),
    };
    Ok(changes
        .iter()
        .map(|(u, edits)| {
            let Json::Arr(es) = edits else { panic!() };
            (u.clone(), es.iter().map(|e| (num(e.get("range").get("start").get("line")), num(e.get("range").get("start").get("character")), num(e.get("range").get("end").get("character")), e.get("newText").str().unwrap().to_string())).collect())
        })
        .collect())
}

#[test]
fn renaming_a_function_edits_every_file_that_names_it() {
    let (dir, base) = project();
    let uri = format!("{}/main.zila", base);
    let mut docs = HashMap::new();
    open(&mut docs, &uri, MAIN);
    let edits = rename(&mut docs, &uri, 6, 17, "double").unwrap();
    let b = format!("{}/util/b.zila", base);
    let mut keys: Vec<&String> = edits.keys().collect();
    keys.sort();
    assert_eq!(keys, [&uri, &b]);
    // the declaration in b.zila: `pub func twice(n: int) -> int` on line 3, characters 9 to 14
    assert_eq!(edits[&b], [(3.0, 9.0, 14.0, "double".to_string())]);
    // two uses in main.zila, lines 6 and 12
    let mut lines: Vec<f64> = edits[&uri].iter().map(|e| e.0).collect();
    lines.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(lines, [6.0, 12.0]);
    // applying them gives a text that still checks
    let mut text: Vec<String> = MAIN.lines().map(|l| l.to_string()).collect();
    for (line, from, to, new) in &edits[&uri] {
        let l: Vec<char> = text[*line as usize].chars().collect();
        text[*line as usize] = l[..*from as usize].iter().collect::<String>() + new + &l[*to as usize..].iter().collect::<String>();
    }
    assert!(text[6].contains("util.double(n)") && text[12].contains("util.double(b)"), "{:?}", text);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn renaming_a_local_edits_its_declaration_and_uses_only() {
    let (dir, base) = project();
    let uri = format!("{}/main.zila", base);
    let mut docs = HashMap::new();
    open(&mut docs, &uri, MAIN);
    // `a` declared on line 10 (character 8) and used on line 11
    let edits = rename(&mut docs, &uri, 10, 8, "first").unwrap();
    assert_eq!(edits.len(), 1);
    let mut e = edits[&uri].clone();
    e.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(e.iter().map(|x| x.0).collect::<Vec<_>>(), [10.0, 11.0]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_rename_to_a_name_that_is_taken_is_an_error_without_edits() {
    let (dir, base) = project();
    let uri = format!("{}/main.zila", base);
    let mut docs = HashMap::new();
    open(&mut docs, &uri, MAIN);
    // a function to the name of another function of the package
    let e = rename(&mut docs, &uri, 10, 13, "main").unwrap_err();
    assert!(e.contains("'main' is already declared"), "{}", e);
    // a local to the name of another local of the same scope (`a` to `b`)
    let e = rename(&mut docs, &uri, 10, 8, "b").unwrap_err();
    assert!(e.contains("'b' is already declared in this scope"), "{}", e);
    // a local to the name of a function it would hide in a call (`n` to `helper` hides the call on its line? no: `a` to `helper`)
    let e = rename(&mut docs, &uri, 10, 8, "helper").unwrap_err();
    assert!(e.contains("would hide"), "{}", e);
    // not a name
    assert!(rename(&mut docs, &uri, 10, 8, "1st").unwrap_err().contains("not a valid name"));
    assert!(rename(&mut docs, &uri, 10, 8, "end").unwrap_err().contains("not a valid name"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_symbol_declared_outside_the_package_cannot_be_renamed() {
    let (dir, base) = project();
    let uri = format!("{}/main.zila", base);
    let mut docs = HashMap::new();
    open(&mut docs, &uri, MAIN);
    let line = MAIN.lines().nth(12).unwrap();
    let at = line.find("contains").unwrap() as i64 + 2;
    let e = rename(&mut docs, &uri, 12, at, "has").unwrap_err();
    assert!(e.contains("outside the package"), "{}", e);
    // a built-in, and nothing at all
    assert!(rename(&mut docs, &uri, 12, 8, "show").is_err());
    assert!(rename(&mut docs, &uri, 9, 0, "x").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn renaming_a_field_and_a_method_follows_them_through_literals_and_calls() {
    let mut docs = HashMap::new();
    let uri = "file:///f.zila";
    let src = "package main\n\ntype P = record\n    x: int\n    y: int\nend\n\nfunc (p: P) sum() -> int\n    return p.x + p.y\nend\n\nfunc main()\n    let p = P{x: 1, y: 2}\n    print(\"{p.sum()} {p.x}\")\nend\n";
    open(&mut docs, uri, src);
    // the field `x` on its declaration: line 3
    let edits = rename(&mut docs, uri, 3, 4, "left").unwrap();
    let lines: Vec<f64> = { let mut v: Vec<f64> = edits[uri].iter().map(|e| e.0).collect(); v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v };
    assert_eq!(lines, [3.0, 8.0, 12.0, 13.0], "the declaration, the use in the method, the literal and a use in main");
    // the method on a call
    let edits = rename(&mut docs, uri, 13, 15, "total").unwrap();
    let lines: Vec<f64> = { let mut v: Vec<f64> = edits[uri].iter().map(|e| e.0).collect(); v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v };
    assert_eq!(lines, [7.0, 13.0]);
    // a field to the name of a method of the same type
    assert!(rename(&mut docs, uri, 3, 4, "sum").unwrap_err().contains("already a member"));
}
