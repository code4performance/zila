//! The language server: JSON, syntax-error diagnostics and formatting, driven message by message
//! (no process is started).

use brasa::lsp::{handle, parse_json, Json};
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
    let r = open(&mut docs, "file:///a.br", "package main\n\nfunc main()\nend\n");
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
    let r = open(&mut docs, "file:///b.br", "package main\n\nfunc main()\n    let = 3\nend\n");
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(3.0));
    assert_eq!(d[0].get("severity"), &Json::Num(1.0));
    assert!(d[0].get("message").str().unwrap().len() > 3);
}

#[test]
fn editing_republishes_and_formatting_returns_one_edit() {
    let mut docs = HashMap::new();
    open(&mut docs, "file:///c.br", "package main\n\nfunc main()\nend\n");
    let r = send(
        &mut docs,
        r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///c.br"},"contentChanges":[{"text":"package main\n\nfunc main()\nprintln(1,2)\nend\n"}]}}"#,
    );
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
    let r = send(&mut docs, r#"{"jsonrpc":"2.0","id":7,"method":"textDocument/formatting","params":{"textDocument":{"uri":"file:///c.br"}}}"#);
    let Json::Arr(edits) = r[0].get("result") else { panic!() };
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].get("newText").str(), Some("package main\n\nfunc main()\n    println(1, 2)\nend\n"));
    assert_eq!(edits[0].get("range").get("end").get("line"), &Json::Num(5.0));
    // an already formatted file needs no edit
    open(&mut docs, "file:///d.br", "package main\n\nfunc main()\nend\n");
    let r = send(&mut docs, r#"{"jsonrpc":"2.0","id":8,"method":"textDocument/formatting","params":{"textDocument":{"uri":"file:///d.br"}}}"#);
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
    let r = open(&mut docs, "file:///na.br", src);
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
        {{ this ) is ] not Brasa ((
    end
    print(s)
end
";
    let r = open(&mut docs, "file:///blocks.br", src);
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
    assert!(!brasa::comptime::launch_log().iter().any(|l| l.starts_with("lsp_never_runs(")));
    // an error outside the block is still reported
    let r = open(&mut docs, "file:///bad.br", &src.replace("print(s)", "print(s"));
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
}

#[test]
fn indexing_a_text_is_a_diagnostic_that_points_to_the_alternatives() {
    let mut docs = HashMap::new();
    let src = "package main\n\nfunc main()\n    let s = \"ação\"\n    let b = s[0]\nend\n";
    let r = open(&mut docs, "file:///idx.br", src);
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
    let r = open(&mut docs, "file:///thr.br", src);
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    let m = d[0].get("message").str().unwrap();
    assert!(m.contains("argument 'n'") && m.contains("not transferable") && m.contains("Mutex"), "{}", m);
}

#[test]
fn pub_declarations_have_no_diagnostics_and_a_misplaced_pub_is_reported_on_its_line() {
    let mut docs = HashMap::new();
    let good = "package lib\n\npub type P = record\n    pub x: int\n    y: int\nend\n\n#[export(\"f\")]\npub func f() -> int\n    return 1\nend\n\npub async func g()\nend\n\npub let K = 1\n";
    let r = open(&mut docs, "file:///pub.br", good);
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]));
    let r = open(&mut docs, "file:///bad.br", "package lib\n\npub embed \"x\"\n");
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].get("range").get("start").get("line"), &Json::Num(2.0));
    assert!(d[0].get("message").str().unwrap().contains("'pub' cannot be applied to 'embed'"));
}

#[test]
fn packages_of_the_project_are_seen_by_the_editor() {
    // a project with a local package: names resolve, a block of the package is a block, and a
    // name found nowhere is reported with the hint
    let dir = std::env::temp_dir().join(format!("brasa_lsp_pkg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("util")).unwrap();
    std::fs::write(dir.join("brasa.toml"), "[package]\nname = \"app\"\n").unwrap();
    std::fs::write(dir.join("util/util.br"), "package util\n\npub func twice(n: int) -> int\n    return n * 2\nend\n\n#[block]\npub comptime func raw_text(raw: text) -> text\n    return raw\nend\n").unwrap();
    let p = dir.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    let uri = |f: &str| if p.starts_with('/') { format!("file://{}/{}", p, f) } else { format!("file:///{}/{}", p, f) };
    let mut docs = HashMap::new();
    let good = "package main\n\nuse util\n\nfunc main()\n    print(\"{util.twice(2)}\")\nend\n";
    let r = open(&mut docs, &uri("main.br"), good);
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]), "a package of the project resolves");
    let blocks = "package main\n\nuse util\n\nfunc main()\n    let s = raw_text\n        {{ not ) Brasa ((\n    end\n    print(s)\nend\n";
    let r = open(&mut docs, &uri("blocks.br"), blocks);
    assert_eq!(r[0].get("params").get("diagnostics"), &Json::Arr(vec![]), "a block of a package of the project");
    let r = open(&mut docs, &uri("bad.br"), &good.replace("use util", "use nothing"));
    let Json::Arr(d) = r[0].get("params").get("diagnostics") else { panic!() };
    assert_eq!(d.len(), 1);
    let m = d[0].get("message").str().unwrap();
    assert!(m.contains("unknown package 'nothing'") && m.contains("--add"), "{}", m);
    let _ = std::fs::remove_dir_all(&dir);
}
