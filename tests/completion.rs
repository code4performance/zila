//! `textDocument/completion`: names in scope, members after `.`, package members, attributes, and a
//! text with syntax errors elsewhere.

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

/// The items offered at (line, character) as (label, kind, detail).
fn complete(docs: &mut HashMap<String, String>, uri: &str, line: i64, character: i64) -> Vec<(String, i64, String)> {
    let msg = format!(r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":{},"character":{}}}}}}}"#, uri, line, character);
    let r = send(docs, &msg).remove(0);
    let Json::Arr(items) = r.get("result").get("items") else { panic!("no items: {:?}", r) };
    items
        .iter()
        .map(|i| {
            let Json::Num(k) = i.get("kind") else { panic!() };
            (i.get("label").str().unwrap().to_string(), *k as i64, i.get("detail").str().unwrap_or("").to_string())
        })
        .collect()
}

fn labels(items: &[(String, i64, String)]) -> Vec<&str> {
    items.iter().map(|i| i.0.as_str()).collect()
}

const FIELD: i64 = 5;
const METHOD: i64 = 2;

const PROGRAM: &str = "package main\n\ntype Point = record\n    x: int\n    y: int\nend\n\nfunc (p: Point) sum() -> int\n    return p.x + p.y\nend\n\ntype Shape = enum\n    Circle(r: int)\n    Empty\nend\n\nfunc main()\n    let p = Point{x: 1, y: 2}\n    let n = 5\n    p.\n    print(\"{n}\")\nend\n";

#[test]
fn after_a_dot_the_fields_and_methods_of_the_value_are_listed_with_their_types() {
    let mut docs = HashMap::new();
    let uri = "file:///m.zila";
    open(&mut docs, uri, PROGRAM);
    // line 19 (index 19) is `    p.`; the cursor follows the dot, at character 6
    let items = complete(&mut docs, uri, 19, 6);
    let x = items.iter().find(|i| i.0 == "x").unwrap_or_else(|| panic!("{:?}", labels(&items)));
    assert_eq!((x.1, x.2.as_str()), (FIELD, "int"));
    assert!(items.iter().any(|i| i.0 == "y" && i.1 == FIELD));
    let sum = items.iter().find(|i| i.0 == "sum").unwrap();
    assert_eq!((sum.1, sum.2.as_str()), (METHOD, "func sum() -> int"));
    assert_eq!(items.len(), 3, "only the members: {:?}", labels(&items));
    // a partly typed member name is still a member list
    open(&mut docs, "file:///m2.zila", &PROGRAM.replace("    p.
", "    p.su
"));
    assert_eq!(labels(&complete(&mut docs, "file:///m2.zila", 19, 8)).len(), 3);
}

#[test]
fn the_variants_of_an_enum_follow_its_name() {
    let mut docs = HashMap::new();
    let uri = "file:///e.zila";
    open(&mut docs, uri, &PROGRAM.replace("    p.\n", "    Shape.\n"));
    let items = complete(&mut docs, uri, 19, 10);
    let mut names = labels(&items);
    names.sort();
    assert_eq!(names, ["Circle", "Empty"]);
    assert_eq!(items.iter().find(|i| i.0 == "Circle").unwrap().2, "int");
}

#[test]
fn a_package_is_completed_with_its_public_members() {
    let dir = std::env::temp_dir().join(format!("zila_complete_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("util")).unwrap();
    std::fs::write(dir.join("zila.toml"), "[package]\nname = \"app\"\n").unwrap();
    std::fs::write(dir.join("util/util.zila"), "package util\n\npub func twice(n: int) -> int\n    return n * 2\nend\n\nfunc hidden() -> int\n    return 1\nend\n").unwrap();
    let p = dir.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    let uri = if p.starts_with('/') { format!("file://{}/main.zila", p) } else { format!("file:///{}/main.zila", p) };
    let mut docs = HashMap::new();
    open(&mut docs, &uri, "package main\n\nuse util\n\nfunc main()\n    util.\nend\n");
    let items = complete(&mut docs, &uri, 5, 9);
    assert_eq!(labels(&items), ["twice"], "{:?}", items);
    assert_eq!(items[0].2, "func twice(n: int) -> int");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_standard_library_package_is_completed_too() {
    let mut docs = HashMap::new();
    let uri = "file:///s.zila";
    open(&mut docs, uri, "package main

use text

func main()
    text.
end
");
    let items = complete(&mut docs, uri, 5, 9);
    assert!(items.len() > 3, "{:?}", labels(&items));
}

#[test]
fn names_in_scope_are_listed_even_when_a_statement_elsewhere_has_a_syntax_error() {
    let mut docs = HashMap::new();
    let uri = "file:///b.zila";
    // line 17: a broken statement; line 19: the cursor, on an empty line, after `n` is declared
    let src = "package main\n\ntype Point = record\n    x: int\nend\n\nfunc helper(a: int) -> int\n    return a\nend\n\nfunc other()\n    let hidden = 1\nend\n\nfunc main(arg: int)\n    let n = 5\n    let = broken\n    let m = 6\n    \n    print(\"{n}\")\nend\n";
    open(&mut docs, uri, src);
    let items = complete(&mut docs, uri, 18, 4);
    let names = labels(&items);
    for want in ["n", "m", "arg", "helper", "Point", "main", "print"] {
        assert!(names.contains(&want), "`{}` is missing from {:?}", want, names);
    }
    assert!(!names.contains(&"hidden"), "a local of another function is not in scope");
    assert!(!names.contains(&"a"), "a parameter of another function is not in scope");
    // `m` is declared before the cursor, `late` after it
    let later = src.replace("    print(\"{n}\")\n", "    let late = 1\n");
    open(&mut docs, "file:///b2.zila", &later);
    let names2: Vec<String> = complete(&mut docs, "file:///b2.zila", 18, 4).into_iter().map(|i| i.0).collect();
    assert!(!names2.contains(&"late".to_string()));
    assert!(names2.contains(&"m".to_string()));
}

#[test]
fn a_function_without_its_end_still_offers_its_locals() {
    let mut docs = HashMap::new();
    let uri = "file:///t.zila";
    let src = "package main\n\nfunc main()\n    let n = 5\n    ";
    open(&mut docs, uri, src);
    let names: Vec<String> = complete(&mut docs, uri, 4, 4).into_iter().map(|i| i.0).collect();
    assert!(names.contains(&"n".to_string()), "{:?}", names);
}

#[test]
fn after_hash_bracket_the_attributes_are_listed() {
    let mut docs = HashMap::new();
    let uri = "file:///a.zila";
    open(&mut docs, uri, "package main\n\n#[ex\nfunc main()\nend\n");
    let items = complete(&mut docs, uri, 2, 4);
    assert!(labels(&items).contains(&"export") && labels(&items).contains(&"no_alloc"), "{:?}", labels(&items));
}

#[test]
fn nothing_is_offered_for_a_document_that_is_not_open() {
    let mut docs = HashMap::new();
    let msg = r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///none.zila"},"position":{"line":0,"character":0}}}"#;
    assert_eq!(send(&mut docs, msg).remove(0).get("result"), &Json::Null);
}
