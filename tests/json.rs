//! The `json` library (spec `json-library`): dynamic values, typed decode and encode derived through
//! comptime reflection, attributes, text and number rules, errors, limits and pretty printing.
//! Programs are compiled to native executables and run (the Linux run is skipped without WSL).
//!
//! Zila Lang text literals treat `{` as the start of an interpolation, so JSON inside the test
//! programs is written between `‹` and `›`; `prep` turns each such span into an escaped Zila Lang
//! literal.

use zila::codegen_x64::{compile_to_linux, CompileOptions};
use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("zila-json-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        zila::comptime::set_cache_dir(&dir);
    });
}

/// Replaces each `‹...›` with the Zila Lang text literal that holds the characters in between.
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

const TYPES: &str = r#"
type User = record
    id: i32
    #[json("full_name")] name: text
    #[json(skip)] password: text
    email: text?
end

type Plain = record
    id: int
    name: text
end

type OnlyId = record
    id: int
end

type Opt = record
    id: int
    email: text?
end

type Color = enum
    Red
    Green
end

type Shape = enum
    Circle(r: float)
    Rect(float, float)
    Dot
end

type Item = record
    email: text
end

type Team = record
    users: []Item
end

type Mixed = record
    name: text
    tags: []text
    scores: Map<text, int>
    owner: Opt
    shape: Shape
    note: text?
    flag: bool
    ratio: float
    big: long
end
"#;

fn program(uses: &str, body: &str) -> String {
    prep(&format!("package main\n\nuse json\n{}\n{}\nfunc main()\n{}\nend\n", uses, TYPES, body))
}

fn build(src: &str) -> Result<std::path::PathBuf, String> {
    setup();
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-json-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Err(e) if e.contains("could not find Visual Studio") => Err("no toolchain".into()),
        r => r.map(|_| exe),
    }
}

/// Builds and runs a program; its output. `None` when there is no native toolchain.
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

/// Decodes `json` as `ty` and prints either what `show` makes of the value `v`, or the error.
fn decode(ty: &str, json: &str, show: &str) -> String {
    format!("match json.parse<{ty}>(‹{json}›)\n    ok(v) -> print({show})\n    err(e) -> print(\"{{e.kind.name}}: \" + e.message)\nend\n")
}

fn wsl_ok() -> bool {
    Command::new("wsl.exe").args(["-e", "sh", "-c", "echo ready"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("ready")).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Dynamic values
// ---------------------------------------------------------------------------

#[test]
fn use_json_compiles_for_both_targets() {
    let src = program("", "print(\"hi\")");
    let file = parse_source(&src).unwrap();
    let out = std::env::temp_dir().join(format!("zila-json-elf-{}", std::process::id()));
    setup();
    compile_to_linux(&file, out.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("{}", e));
    let _ = std::fs::remove_file(&out);
    let _ = build(&src).map(std::fs::remove_file);
}

const SHOW_VALUE: &str = r#"
func show(v: json.Value) -> text
    match v
        Null -> return "null"
        Bool(b) -> return "bool({b})"
        Number(f, lit) -> return "num({lit})"
        Text(t) -> return "text({t})"
        List(xs) ->
            var out = "["
            for x in xs
                out = out + show(x) + ","
            end
            return out + "]"
        Object(m) ->
            var out = "\{"
            for k, x in m
                out = out + k + "=" + show(x) + ","
            end
            return out + "\}"
    end
end
"#;

#[test]
fn a_tree_is_parsed_stringified_and_keeps_big_integers() {
    let body = r#"
match json.parse_value(‹{"a": [1, true, null], "b": "x"}›)
    ok(v) -> print(show(v))
    err(e) -> print(e.message)
end
match json.parse_value(‹{ "a" : [ 1 , 2 ] }›)
    ok(v) -> print(json.stringify(v) ?? "bad")
    err(e) -> print(e.message)
end
match json.parse_value(‹12345678901234567890›)
    ok(v) -> print(json.stringify(v) ?? "bad")
    err(e) -> print(e.message)
end
match json.parse_value(‹  [ ]  ›)
    ok(v) -> print(show(v))
    err(e) -> print(e.message)
end
match json.parse_value(‹{"z":1,"a":2,"m":3}›)
    ok(v) -> print(json.stringify(v) ?? "bad")
    err(e) -> print(e.message)
end
"#;
    let src = program("", body).replace("func main()", &format!("{}\nfunc main()", prep(SHOW_VALUE)));
    assert_eq!(run(&src), "{a=[num(1),bool(true),null,],b=text(x),}\n{\"a\":[1,2]}\n12345678901234567890\n[]\n{\"z\":1,\"a\":2,\"m\":3}\n");
}

#[test]
fn the_grammar_is_strict() {
    let docs = ["[1,]", "01", "{'a':1}", "[1 2]", "NaN", "", "  ", "[1,2", "{\"a\"}", "{\"a\":1,}", "tru", "-", "1.", ".5", "1e", "+1", "\"abc", "\"a\nb\"", "[1] x", "\u{feff}[1]", "{\"a\" 1}", "nul", "[,1]", "{,}", "-01"];
    let mut body = String::new();
    for d in docs {
        let d = d.replace('\n', "\\n");
        body.push_str(&format!("match json.parse_value(‹{}›)\n    ok(v) -> print(\"accepted\")\n    err(e) -> print(e.kind.name)\nend\n", d));
    }
    let src = program("", &body);
    let out = run(&src);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), docs.len(), "{}", out);
    // the raw newline in the string case was escaped by the test, so that one is an escape and valid
    for (i, l) in lines.iter().enumerate() {
        if docs[i] == "\"a\nb\"" {
            assert_eq!(*l, "accepted");
        } else {
            assert_eq!(*l, "std.InvalidInput", "document {:?}", docs[i]);
        }
    }
}

#[test]
fn numbers_in_a_tree_keep_their_literals_and_hand_built_ones_print_shortest() {
    let body = r#"
match json.parse_value(‹[1e3, 0.10, -0, 2.50E+2, 1E-2]›)
    ok(v) -> print(json.stringify(v) ?? "bad")
    err(e) -> print(e.message)
end
let built = json.List([json.Number(0.5, ""), json.Number(100.0, ""), json.Number(1.0 / 3.0, "")])
print(json.stringify(built) ?? "bad")
let zero = 0.0
match json.stringify(json.Number(zero / zero, ""))
    ok(t) -> print(t)
    err(e) -> print("error: " + e.message)
end
"#;
    assert_eq!(run(&program("", body)), "[1e3,0.10,-0,2.50E+2,1E-2]\n[0.5,100.0,0.3333333333333333]\nerror: cannot encode a number that is not finite\n");
}

// ---------------------------------------------------------------------------
// Typed decode
// ---------------------------------------------------------------------------

#[test]
fn records_lists_maps_optionals_and_unknown_keys() {
    let mut body = String::new();
    body.push_str(&decode("Plain", r#"{"id": 7, "name": "Ana"}"#, "\"{v.id} {v.name}\""));
    body.push_str(&decode("Map<text, []int>", r#"{"a": [1, 2], "b": []}"#, r##""{v["a"].len} {v["a"][1]} {v["b"].len} {v.len}""##));
    body.push_str(&decode("Opt", r#"{"id": 1}"#, "\"{v.id} {v.email == nil}\""));
    body.push_str(&decode("Opt", r#"{"id": 1, "email": null}"#, "\"{v.id} {v.email == nil}\""));
    body.push_str(&decode("Opt", r#"{"id": 1, "email": "a@b"}"#, r##""{v.id} {v.email ?? ""}""##));
    body.push_str(&decode("Plain", r#"{"id": 1}"#, "v.id"));
    body.push_str(&decode("OnlyId", r#"{"id": 1, "extra": [1, 2, {"deep": [null]}], "more": "x"}"#, "v.id"));
    body.push_str(&decode("[]int", "[3, 4, 5]", r##""{v.len} {v[2]}""##));
    body.push_str(&decode("[]Plain", r#"[{"id": 1, "name": "a"}, {"id": 2, "name": "b"}]"#, r##""{v.len} {v[1].name}""##));
    body.push_str(&decode("[][]int", "[[1], [], [2, 3]]", r##""{v.len} {v[2].len}""##));
    assert_eq!(
        run(&program("", &body)),
        "7 Ana\n2 2 0 2\n1 true\n1 true\n1 a@b\nstd.InvalidInput: missing field \"name\" at $\n1\n3 5\n2 b\n3 2\n"
    );
}

#[test]
fn enums_decode_from_text_and_one_key_objects() {
    let mut body = String::new();
    body.push_str(&decode("Color", r#""Red""#, "v == Color.Red"));
    body.push_str(&decode("Color", r#""Green""#, "v == Color.Green"));
    body.push_str(&decode("Color", r#""Blue""#, "v == Color.Red"));
    body.push_str(&decode("Shape", r#"{"Circle": 2.5}"#, "match v\n        Circle(r) -> \"circle {r}\"\n        _ -> \"other\"\n    end"));
    body.push_str(&decode("Shape", r#"{"Rect": [2, 3.5]}"#, "match v\n        Rect(w, h) -> \"rect {w} {h}\"\n        _ -> \"other\"\n    end"));
    body.push_str(&decode("Shape", r#""Dot""#, "match v\n        Dot -> \"dot\"\n        _ -> \"other\"\n    end"));
    body.push_str(&decode("Shape", r#"{"Rect": [2]}"#, "1"));
    body.push_str(&decode("Shape", r#"{"Circle": 1, "Dot": 2}"#, "1"));
    body.push_str(&decode("Shape", "5", "1"));
    assert_eq!(
        run(&program("", &body)),
        "true\ntrue\nstd.InvalidInput: unknown case \"Blue\" of Color at $\ncircle 2.5\nrect 2.0 3.5\ndot\n\
         std.InvalidInput: expected a list of 2 values at $.Rect\nstd.InvalidInput: expected an object with exactly one key at $\nstd.InvalidInput: expected text or object, found number at $\n"
    );
}

#[test]
fn a_new_type_needs_no_registration() {
    let src = prep(
        r#"package main

use json

type Fresh = record
    a: int
    b: []text
end

func main()
    match json.parse(Fresh, ‹{"a": 3, "b": ["x", "y"]}›)
        ok(f) -> print(f.a, f.b.len)
        err(e) -> print(e.message)
    end
    print(json.encode(Fresh, Fresh { a: 1, b: ["z"] }) ?? "bad")
end
"#,
    );
    assert_eq!(run(&src), "3 2\n{\"a\":1,\"b\":[\"z\"]}\n");
    // the compiler proper has no JSON in it: the only mention is the line that registers the library
    // (the language server and the debug adapter speak the JSON of their own protocols with a small
    // reader of their own, and the debug adapter's debug info is written as JSON too)
    let mut hits = Vec::new();
    let mut files = vec![std::path::PathBuf::from("src")];
    while let Some(p) = files.pop() {
        for e in std::fs::read_dir(&p).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                files.push(path);
            } else if path.extension().map(|x| x == "rs").unwrap_or(false) {
                let top = path.strip_prefix("src").unwrap().components().next().and_then(|c| c.as_os_str().to_str());
                let name = path.file_name().unwrap().to_str().unwrap();
                if matches!(top, Some("lsp") | Some("dap")) || (top == Some("codegen_x64") && matches!(name, "debuginfo.rs" | "output.rs")) {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for (i, l) in text.lines().enumerate() {
                    if l.to_lowercase().contains("json") {
                        hits.push(format!("{}:{}: {}", path.display(), i + 1, l.trim()));
                    }
                }
            }
        }
    }
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert!(hits[0].contains("\"json\" => Some(include_str!(\"../packages/json/json.zila\"))"), "{:?}", hits);
}

#[test]
fn a_mixed_record_with_every_kind_decodes() {
    let json = r#"{"name": "n", "tags": ["a", "b"], "scores": {"x": 1, "y": 2}, "owner": {"id": 9, "email": "o@x"}, "shape": {"Circle": 1.5}, "flag": true, "ratio": 0.25, "big": 9007199254740993}"#;
    let body = decode("Mixed", json, r##""{v.name} {v.tags.len} {v.scores["y"]} {v.owner.id} {v.owner.email ?? ""} {v.note == nil} {v.flag} {v.ratio} {v.big}""##);
    assert_eq!(run(&program("", &body)), "n 2 2 9 o@x true true 0.25 9007199254740993\n");
}

// ---------------------------------------------------------------------------
// Typed encode
// ---------------------------------------------------------------------------

#[test]
fn records_enums_optionals_and_maps_encode_and_round_trip() {
    let body = r#"
print(json.encode(Plain, Plain { id: 7, name: "Ana" }) ?? "bad")
print(json.encode<Opt>(Opt { id: 1, email: nil }) ?? "bad")
print(json.encode<Opt>(Opt { id: 1, email: "a@b" }) ?? "bad")
print(json.encode<Shape>(Shape.Circle(2.5)) ?? "bad")
print(json.encode<Shape>(Shape.Rect(2.0, 3.0)) ?? "bad")
print(json.encode<Shape>(Shape.Dot) ?? "bad")
print(json.encode<Color>(Color.Green) ?? "bad")
let scores: Map<text, int> = {}
scores["b"] = 2
scores["a"] = 1
let tags = ["x", "y"]
let m = Mixed { name: "n", tags: tags, scores: scores, owner: Opt { id: 9, email: "o@x" }, shape: Shape.Rect(1.0, 2.5), note: nil, flag: true, ratio: 0.1, big: 9007199254740993 }
let text1 = json.encode(Mixed, m) ?? "bad"
print(text1)
match json.parse(Mixed, text1)
    ok(back) ->
        print(back.name == m.name, back.tags.len, back.scores["a"], back.scores.len, back.owner.id, back.owner.email ?? "", back.note == nil, back.flag, back.ratio == 0.1, back.big == m.big)
        print((json.encode(Mixed, back) ?? "bad") == text1)
    err(e) -> print(e.message)
end
print(json.encode<[]int>([1, 2, 3]) ?? "bad")
print(json.encode<[]Plain>([]) ?? "bad")
"#;
    assert_eq!(
        run(&program("", body)),
        "{\"id\":7,\"name\":\"Ana\"}\n{\"id\":1,\"email\":null}\n{\"id\":1,\"email\":\"a@b\"}\n{\"Circle\":2.5}\n{\"Rect\":[2.0,3.0]}\n\"Dot\"\n\"Green\"\n\
         {\"name\":\"n\",\"tags\":[\"x\",\"y\"],\"scores\":{\"b\":2,\"a\":1},\"owner\":{\"id\":9,\"email\":\"o@x\"},\"shape\":{\"Rect\":[1.0,2.5]},\"note\":null,\"flag\":true,\"ratio\":0.1,\"big\":9007199254740993}\n\
         true 2 1 2 9 o@x true true true true\ntrue\n[1,2,3]\n[]\n"
    );
}

// ---------------------------------------------------------------------------
// Attributes
// ---------------------------------------------------------------------------

#[test]
fn rename_and_skip_work_in_both_directions() {
    let body = r#"
let u = User { id: 1, name: "Ana Lima", password: "secret", email: nil }
let text1 = json.encode(User, u) ?? "bad"
print(text1)
match json.parse(User, ‹{"id": 1, "password": "x", "full_name": "Bia", "name": "ignored"}›)
    ok(v) -> print(v.id, v.name, v.password.len, v.email == nil)
    err(e) -> print(e.message)
end
match json.parse(User, text1)
    ok(v) -> print(v.name, v.password.len)
    err(e) -> print(e.message)
end
"#;
    assert_eq!(run(&program("", body)), "{\"id\":1,\"full_name\":\"Ana Lima\",\"email\":null}\n1 Bia 0 true\nAna Lima 0\n");
}

fn attr_program(types: &str, call: &str) -> String {
    prep(&format!("package main\n\nuse json\n\n{}\nfunc main()\n    {}\nend\n", types, call))
}

#[test]
fn a_bad_attribute_or_a_key_clash_is_a_compile_error_naming_the_fields() {
    let e = compile_error(&attr_program("type Bad = record\n    id: int\n    #[json(rename)] name: text\nend\n", "print(json.encode(Bad, Bad { id: 1, name: \"x\" }) ?? \"\")"));
    assert!(e.contains("name") && e.contains("json attribute"), "{}", e);
    let e = compile_error(&attr_program("type Bad = record\n    #[json(\"a\", \"b\")] name: text\nend\n", "print(json.encode(Bad, Bad { name: \"x\" }) ?? \"\")"));
    assert!(e.contains("takes one argument") && e.contains("name"), "{}", e);
    let e = compile_error(&attr_program("type Bad = record\n    id: int\n    #[json(\"id\")] ident: int\nend\n", "print(json.parse(Bad, \"\").is_ok())"));
    assert!(e.contains("id") && e.contains("ident") && e.contains("json key"), "{}", e);
    // the same clash through a rename of two fields
    let e = compile_error(&attr_program("type Bad = record\n    #[json(\"k\")] a: int\n    #[json(\"k\")] b: int\nend\n", "print(json.encode(Bad, Bad { a: 1, b: 2 }) ?? \"\")"));
    assert!(e.contains("a and b") && e.contains("\"k\""), "{}", e);
    // a quoted word is a key: json("skip") renames, json(skip) leaves the field out
    let src = attr_program("type Ok = record\n    #[json(\"rename\")] a: int\n    #[json(skip)] b: int\nend\n", "print(json.encode(Ok, Ok { a: 1, b: 2 }) ?? \"\")");
    assert_eq!(run(&src), "{\"rename\":1}\n");
}

#[test]
fn unsupported_types_are_refused_at_compile_time() {
    let e = compile_error(&attr_program("type Bad = record\n    f: func(int) -> int\nend\n", "print(json.encode(Bad, Bad { f: func(x) x end }) ?? \"\")"));
    assert!(e.contains("json cannot encode") && e.contains("func"), "{}", e);
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

#[test]
fn escapes_unicode_and_surrogates() {
    let mut body = String::new();
    body.push_str(&decode("text", r#""café 😀""#, "v"));
    body.push_str(&decode("text", r#""café 😀""#, "v"));
    body.push_str(&decode("text", r#""a\"b\\c\/d\b\f\n\r\te""#, "v.len"));
    body.push_str(&decode("text", r#""\ud83d""#, "v"));
    body.push_str(&decode("text", r#""\ude00""#, "v"));
    body.push_str(&decode("text", r#""\ud83dx""#, "v"));
    body.push_str(&decode("text", r#""\ud83dA""#, "v"));
    body.push_str(&decode("text", r#""\x""#, "v"));
    body.push_str(&decode("text", r#""\u12""#, "v"));
    body.push_str(&decode("text", r#""\u12G4""#, "v"));
    let out = run(&program("", &body));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "café 😀");
    assert_eq!(lines[1], "café 😀");
    assert_eq!(lines[2], "13");
    for (i, l) in lines.iter().enumerate().skip(3) {
        assert!(l.starts_with("std.InvalidInput: "), "line {}: {}", i, l);
    }
    assert!(lines[3].contains("unpaired surrogate") && lines[3].contains("line 1, column 2"), "{}", lines[3]);
}

#[test]
fn control_characters_and_invalid_utf8_are_errors() {
    // a file with bad bytes is the one way to get a text that is not UTF-8
    let dir = std::env::temp_dir().join(format!("zila-json-bytes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("lone", b"\"a\xffb\"".to_vec()),
        ("cut", b"\"a\xc3\"".to_vec()),
        ("overlong", b"\"\xc0\x80\"".to_vec()),
        ("surrogate", b"\"\xed\xa0\x80\"".to_vec()),
        ("toobig", b"\"\xf5\x80\x80\x80\"".to_vec()),
        ("ok", "\"ação\"".as_bytes().to_vec()),
        ("ctrl", b"\"a\x01b\"".to_vec()),
        ("bom", b"\xef\xbb\xbf[1]".to_vec()),
    ];
    let mut body = String::new();
    for (name, bytes) in &cases {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        let path = p.to_str().unwrap().replace('\\', "/");
        body.push_str(&format!("match await fs.read(\"{}\")\n    ok(raw) ->\n        match json.parse_value(raw)\n            ok(v) -> print(\"{}: ok\")\n            err(e) -> print(\"{}: \" + e.message)\n        end\n    err(e) -> print(\"read failed \" + e.message)\nend\n", path, name, name));
    }
    let out = run(&program("use fs", &body));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), cases.len(), "{}", out);
    assert_eq!(lines[0], "lone: invalid UTF-8 in a string at line 1, column 3");
    assert!(lines[1].starts_with("cut: invalid UTF-8"), "{}", lines[1]);
    assert!(lines[2].starts_with("overlong: invalid UTF-8"), "{}", lines[2]);
    assert!(lines[3].starts_with("surrogate: invalid UTF-8"), "{}", lines[3]);
    assert!(lines[4].starts_with("toobig: invalid UTF-8"), "{}", lines[4]);
    assert_eq!(lines[5], "ok: ok");
    assert!(lines[6].starts_with("ctrl: unescaped control character"), "{}", lines[6]);
    assert!(lines[7].starts_with("bom: unexpected"), "{}", lines[7]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn encoding_escapes_what_must_be_escaped_and_nothing_else() {
    let body = r#"
let s = "q\"uote\\ \n tab\t nul" + "é 😀 \{\}"
print(json.encode<text>(s) ?? "bad")
print(json.encode<text>("ctl" + char(1).to_text() + char(31).to_text() + char(127).to_text()) ?? "bad")
match json.parse(text, json.encode<text>(s) ?? "")
    ok(back) -> print(back == s)
    err(e) -> print(e.message)
end
"#;
    assert_eq!(run(&program("", body)), "\"q\\\"uote\\\\ \\n tab\\t nulé 😀 {}\"\n\"ctl\\u0001\\u001f\u{7f}\"\ntrue\n");
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

#[test]
fn integers_are_exact_and_never_wrap() {
    let mut body = String::new();
    for (ty, j) in [
        ("u8", "300"),
        ("u8", "255"),
        ("u8", "-1"),
        ("u8", "256"),
        ("int", "9223372036854775808"),
        ("long", "9223372036854775808"),
        ("long", "9223372036854775807"),
        ("long", "-9223372036854775808"),
        ("long", "-9223372036854775809"),
        ("int", "2147483647"),
        ("int", "2147483648"),
        ("int", "-2147483648"),
        ("int", "-2147483649"),
        ("i8", "-128"),
        ("i8", "128"),
        ("u16", "65535"),
        ("u32", "4294967295"),
        ("u32", "4294967296"),
        ("u64", "18446744073709551615"),
        ("u64", "18446744073709551616"),
        ("u64", "-0"),
        ("u64", "-1"),
        ("int", "1e2"),
        ("int", "5.0"),
        ("int", "1.5"),
        ("int", "1e-1"),
        ("int", "12e-1"),
        ("int", "-0.0"),
        ("int", "0e99999"),
        ("int", "1e99999"),
        ("int", "100e-2"),
        ("long", "1E18"),
        ("int", "\"7\""),
        ("int", "true"),
        ("int", "null"),
    ] {
        body.push_str(&decode(ty, j, "\"{v}\""));
    }
    let out = run(&program("", &body));
    let lines: Vec<&str> = out.lines().collect();
    let want = [
        "std.InvalidInput: 300 does not fit u8 at $",
        "255",
        "std.InvalidInput: -1 does not fit u8 at $",
        "std.InvalidInput: 256 does not fit u8 at $",
        "std.InvalidInput: 9223372036854775808 does not fit int at $",
        "std.InvalidInput: 9223372036854775808 does not fit long at $",
        "9223372036854775807",
        "-9223372036854775808",
        "std.InvalidInput: -9223372036854775809 does not fit long at $",
        "2147483647",
        "std.InvalidInput: 2147483648 does not fit int at $",
        "-2147483648",
        "std.InvalidInput: -2147483649 does not fit int at $",
        "-128",
        "std.InvalidInput: 128 does not fit i8 at $",
        "65535",
        "4294967295",
        "std.InvalidInput: 4294967296 does not fit u32 at $",
        "18446744073709551615",
        "std.InvalidInput: 18446744073709551616 does not fit u64 at $",
        "0",
        "std.InvalidInput: -1 does not fit u64 at $",
        "100",
        "5",
        "std.InvalidInput: expected int, found the number 1.5 at $",
        "std.InvalidInput: expected int, found the number 1e-1 at $",
        "std.InvalidInput: expected int, found the number 12e-1 at $",
        "0",
        "0",
        "std.InvalidInput: 1e99999 does not fit int at $",
        "1",
        "1000000000000000000",
        "std.InvalidInput: expected int, found text at $",
        "std.InvalidInput: expected int, found bool at $",
        "std.InvalidInput: expected int, found null at $",
    ];
    assert_eq!(lines.len(), want.len(), "{}", out);
    for (i, w) in want.iter().enumerate() {
        // the u64 results print through the compiler's own formatting; only the error texts and the
        // values that fit a signed type are compared exactly
        if w.starts_with("18446744073709551615") {
            continue;
        }
        assert_eq!(lines[i], *w, "case {}", i);
    }
}

#[test]
fn floats_round_trip_and_print_shortest() {
    let body = r#"
print(json.encode<float>(0.1) ?? "bad")
print(json.encode<float>(1e21) ?? "bad")
print(json.encode<float>(100.0) ?? "bad")
print(json.encode<float>(-0.0) ?? "bad")
print(json.encode<f32>(0.1) ?? "bad")
let zero = 0.0
match json.encode<float>(zero / zero)
    ok(t) -> print(t)
    err(e) -> print("nan: " + e.kind.name + ": " + e.message)
end
match json.encode<float>(1.0 / zero)
    ok(t) -> print(t)
    err(e) -> print("inf: " + e.message)
end
match json.encode<f32>(f32(1.0 / zero))
    ok(t) -> print(t)
    err(e) -> print("inf32: " + e.message)
end
for lit in ["1e2", "5.0", "1.5", "-2.5e-3", "1E400", "0", "123456789.123456789"]
    match json.parse(float, lit)
        ok(f) -> print("{f}")
        err(e) -> print(e.message)
    end
end
"#;
    let out = run(&program("", body));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(&lines[..5], ["0.1", "1e21", "100.0", "-0.0", "0.1"]);
    assert_eq!(lines[5], "nan: std.InvalidInput: cannot encode a number that is not finite");
    assert_eq!(lines[6], "inf: cannot encode a number that is not finite");
    assert_eq!(lines[7], "inf32: cannot encode a number that is not finite");
    assert_eq!(&lines[8..13], ["100.0", "5.0", "1.5", "-0.0025", "inf"]);
}

#[test]
fn encoding_then_decoding_thousands_of_random_floats_keeps_every_bit() {
    let body = r#"
var seed: long = 88172645463325252
var bad = 0
var skipped = 0
for i in 0..4000
    seed = seed bitxor (seed << 13)
    seed = seed bitxor (seed >> 7)
    seed = seed bitxor (seed << 17)
    let x = __from_bits(seed)
    if x - x != 0.0
        skipped += 1
    else
        let t = json.encode<float>(x) ?? "bad"
        match json.parse(float, t)
            ok(y) -> if __bits(y) != __bits(x)
                bad += 1
            end
            err(e) -> bad += 1
        end
    end
end
print(bad, skipped < 40)
"#;
    assert_eq!(run(&program("", body)), "0 true\n");
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[test]
fn syntax_errors_carry_line_and_column() {
    let body = r#"
match json.parse_value(‹{"a": 1,
  x}›)
    ok(v) -> print("accepted")
    err(e) -> print(e.kind.name + " | " + e.message)
end
match json.parse_value(‹[1, 2
]x›)
    ok(v) -> print("accepted")
    err(e) -> print(e.message)
end
match json.parse_value(‹["é", @]›)
    ok(v) -> print("accepted")
    err(e) -> print(e.message)
end
match json.parse_value(‹›)
    ok(v) -> print("accepted")
    err(e) -> print(e.message)
end
match json.parse_value(‹[1,
 tru]›)
    ok(v) -> print("accepted")
    err(e) -> print(e.message)
end
"#;
    let out = run(&program("", body));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "std.InvalidInput | expected a text key, found 'x' at line 2, column 3");
    assert!(lines[0].ends_with("line 2, column 3"));
    assert_eq!(lines[1], "unexpected 'x' after the end of the value at line 2, column 2");
    assert_eq!(lines[2], "unexpected '@' at line 1, column 7");
    assert_eq!(lines[3], "unexpected end of input at line 1, column 1");
    assert_eq!(lines[4], "invalid literal, expected true at line 2, column 2");
}

#[test]
fn type_errors_carry_a_path() {
    let mut body = String::new();
    body.push_str(&decode("Team", r#"{"users": [{"email": "a"}, {"email": "b"}, {"email": 5}]}"#, "v.users.len"));
    body.push_str(&decode("Team", r#"{"users": [{"email": "a"}, {}]}"#, "v.users.len"));
    body.push_str(&decode("Team", r#"{"users": {"email": "a"}}"#, "v.users.len"));
    body.push_str(&decode("Team", r#"[1]"#, "v.users.len"));
    body.push_str(&decode("Map<text, []int>", r#"{"a": [1, "x"]}"#, "v.len"));
    body.push_str(&decode("Mixed", r#"{"name": "n", "tags": [], "scores": {}, "owner": {"id": 1, "email": 3}, "shape": "Dot", "flag": true, "ratio": 1, "big": 1}"#, "v.name"));
    assert_eq!(
        run(&program("", &body)),
        "std.InvalidInput: expected text, found number at $.users[2].email\n\
         std.InvalidInput: missing field \"email\" at $.users[1]\n\
         std.InvalidInput: expected list, found object at $.users\n\
         std.InvalidInput: expected object, found list at $\n\
         std.InvalidInput: expected int, found text at $.a[1]\n\
         std.InvalidInput: expected text, found number at $.owner.email\n"
    );
}

// ---------------------------------------------------------------------------
// Duplicates and depth
// ---------------------------------------------------------------------------

#[test]
fn duplicate_keys_are_errors_in_types_and_last_wins_in_values() {
    let mut body = String::new();
    body.push_str(&decode("OnlyId", r#"{"id": 1, "id": 2}"#, "v.id"));
    body.push_str(&decode("Map<text, int>", r#"{"a": 1, "a": 2}"#, "v.len"));
    body.push_str(&decode("Team", r#"{"users": [{"email": "a", "email": "b"}]}"#, "v.users.len"));
    body.push_str("match json.parse_value(‹{\"a\": 1, \"b\": 0, \"a\": 2}›)\n    ok(v) -> print(json.stringify(v) ?? \"bad\")\n    err(e) -> print(e.message)\nend\n");
    assert_eq!(
        run(&program("", &body)),
        "std.InvalidInput: duplicate key \"id\" at $\nstd.InvalidInput: duplicate key \"a\" at $\nstd.InvalidInput: duplicate key \"email\" at $.users[0]\n{\"a\":2,\"b\":0}\n"
    );
}

#[test]
fn nesting_is_limited_to_128_levels_everywhere() {
    let body = r#"
func nest(n: int) -> json.Value
    var v = json.Null
    for i in 0..n
        v = json.List([v])
    end
    return v
end

func depth_of(s: text) -> text
    match json.parse_value(s)
        ok(v) -> return "ok"
        err(e) -> return e.message
    end
end
"#;
    let main = r#"
print(depth_of(text.repeat("[", 128) + text.repeat("]", 128)))
print(depth_of(text.repeat("[", 129) + text.repeat("]", 129)))
print(depth_of(text.repeat("[", 200) + text.repeat("]", 200)))
print(depth_of(text.repeat("[", 100000)))
print(depth_of(text.repeat("\{\"a\":", 129) + "1" + text.repeat("\}", 129)))
match json.parse<[]json.Value>(text.repeat("[", 129) + text.repeat("]", 129))
    ok(v) -> print("typed ok")
    err(e) -> print("typed: " + e.message)
end
match json.parse(OnlyId, "\{\"id\": 1, \"skipme\": " + text.repeat("[", 200) + "\}")
    ok(v) -> print("skipped ok")
    err(e) -> print("skip: " + e.message)
end
match json.stringify(nest(128))
    ok(t) -> print("stringify ok " + str_len(t))
    err(e) -> print(e.message)
end
match json.stringify(nest(129))
    ok(t) -> print("stringify ok")
    err(e) -> print("stringify: " + e.message)
end
match json.encode<json.Value>(nest(200))
    ok(t) -> print("encode ok")
    err(e) -> print("encode: " + e.message)
end
"#;
    let src = program("use text", main).replace("func main()", &format!("{}\nfunc str_len(t: text) -> text\n    return \"{{t.len}}\"\nend\n\nfunc main()", body));
    let out = run(&src);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "ok");
    for l in &lines[1..5] {
        assert!(l.contains("nesting is deeper than the limit of 128 levels"), "{}", l);
    }
    assert!(lines[5].starts_with("typed: nesting is deeper than the limit of 128 levels"), "{}", lines[5]);
    assert!(lines[6].starts_with("skip: nesting is deeper than the limit of 128 levels"), "{}", lines[6]);
    assert_eq!(lines[7], format!("stringify ok {}", 128 * 2 + 4));
    assert!(lines[8].starts_with("stringify: nesting is deeper than the limit of 128 levels"), "{}", lines[8]);
    assert!(lines[9].starts_with("encode: nesting is deeper than the limit of 128 levels"), "{}", lines[9]);
}

// ---------------------------------------------------------------------------
// Pretty printing
// ---------------------------------------------------------------------------

#[test]
fn indented_output_puts_each_member_on_its_own_line() {
    let body = r#"
match json.parse_value(‹{"a":[1,{}],"b":null,"c":[],"d":{"e":"x"}}›)
    ok(v) ->
        print(json.stringify_with(v, json.Options { indent: 2 }) ?? "bad")
        print(json.stringify_with(v, json.Options { indent: 0 }) ?? "bad")
        print(json.stringify_with(v, json.Options { indent: 4 }) ?? "bad")
    err(e) -> print(e.message)
end
print(json.encode_with(Opt, Opt { id: 1, email: "e" }, json.Options { indent: 1 }) ?? "bad")
print(json.encode(Opt, Opt { id: 1, email: "e" }) ?? "bad")
print(json.encode_with<[]int>([], json.Options { indent: 2 }) ?? "bad")
print(json.encode_with<[]int>([1], json.Options { indent: 2 }) ?? "bad")
"#;
    let out = run(&program("", body));
    let want = "{\n  \"a\": [\n    1,\n    {}\n  ],\n  \"b\": null,\n  \"c\": [],\n  \"d\": {\n    \"e\": \"x\"\n  }\n}\n\
                {\"a\":[1,{}],\"b\":null,\"c\":[],\"d\":{\"e\":\"x\"}}\n\
                {\n    \"a\": [\n        1,\n        {}\n    ],\n    \"b\": null,\n    \"c\": [],\n    \"d\": {\n        \"e\": \"x\"\n    }\n}\n\
                {\n \"id\": 1,\n \"email\": \"e\"\n}\n{\"id\":1,\"email\":\"e\"}\n[]\n[\n  1\n]\n";
    assert_eq!(out, want);
    assert!(!out.lines().any(|l| l.ends_with(' ')));
}

#[test]
fn a_hundred_thousand_items_stringify_in_reasonable_time() {
    let body = r#"
let xs: []int = []
for i in 0..100000
    xs.add(i)
end
let s = json.encode<[]int>(xs) ?? "bad"
print(s.len)
match json.parse<[]int>(s)
    ok(back) -> print(back.len, back[99999])
    err(e) -> print(e.message)
end
match json.parse_value(s)
    ok(v) -> print((json.stringify(v) ?? "bad").len)
    err(e) -> print(e.message)
end
"#;
    let started = std::time::Instant::now();
    assert_eq!(run(&program("", body)), "588891\n100000 99999\n588891\n");
    assert!(started.elapsed().as_secs() < 60, "took {:?}", started.elapsed());
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

#[test]
fn decoding_and_encoding_in_a_loop_leaks_nothing() {
    let body = r#"
let doc = ‹{"name": "n", "tags": ["a", "b", "c"], "scores": {"x": 1, "y": 2}, "owner": {"id": 9, "email": "o@x"}, "shape": {"Rect": [1, 2]}, "note": "z", "flag": true, "ratio": 0.5, "big": 5}›
var before = 0
var after = 0
for round in 0..1000
    if round == 100
        before = live_objects()
    end
    match json.parse(Mixed, doc)
        ok(m) ->
            let t = json.encode(Mixed, m) ?? "bad"
            if t.len == 0
                print("empty")
            end
        err(e) -> print(e.message)
    end
    match json.parse_value(doc)
        ok(v) -> if (json.stringify(v) ?? "").len == 0
            print("empty")
        end
        err(e) -> print(e.message)
    end
    match json.parse(Mixed, ‹{"name": 1}›)
        ok(m) -> print("accepted")
        err(e) -> if e.message.len == 0
            print("empty")
        end
    end
end
after = live_objects()
print(before == after)
"#;
    assert_eq!(run(&program("", body)), "true\n");
}

// ---------------------------------------------------------------------------
// Random corruption never stops the program
// ---------------------------------------------------------------------------

#[test]
fn two_thousand_random_mutations_return_a_value_or_an_error() {
    let body = r#"
let doc = ‹{"name": "né", "tags": ["a", "b"], "scores": {"x": 1, "y": -2}, "owner": {"id": 9, "email": null}, "shape": {"Rect": [1, 2.5e0]}, "note": "z", "flag": false, "ratio": 0.5, "big": 12}›
let bytes = doc.bytes()
var seed: long = 123456789
var oks = 0
var errs = 0
for round in 0..2000
    seed = seed bitxor (seed << 13)
    seed = seed bitxor (seed >> 7)
    seed = seed bitxor (seed << 17)
    let copy = doc.bytes()
    let n = 1 + int((seed >> 3) bitand 3)
    for k in 0..n
        seed = seed bitxor (seed << 13)
        seed = seed bitxor (seed >> 7)
        seed = seed bitxor (seed << 17)
        let at = int((seed >> 5) bitand 1023) % copy.len
        let pick = int((seed >> 20) bitand 7)
        var b = int((seed >> 30) bitand 127)
        if pick < 3
            b = int("\{[]\},:\"\\ 0-.e".byte(int((seed >> 40) bitand 255) % 13))
        end
        copy[at] = u8(b)
    end
    match text.from_bytes(copy)
        ok(t) ->
            match json.parse(Mixed, t)
                ok(m) -> oks += 1
                err(e) -> errs += 1
            end
            match json.parse_value(t)
                ok(v) -> oks += 1
                err(e) -> errs += 1
            end
        err(e) -> errs += 1
    end
end
print(oks + errs >= 2000, errs > 0, oks > 0)
"#;
    assert_eq!(run(&program("use text", body)), "true true true\n");
}

// ---------------------------------------------------------------------------
// Targets and numbers on both
// ---------------------------------------------------------------------------

#[test]
fn windows_and_linux_print_the_same() {
    let body = r#"
let scores: Map<text, int> = {}
scores["b"] = 2
scores["a"] = 1
let m = Mixed { name: "né\n", tags: ["x", "y"], scores: scores, owner: Opt { id: 9, email: "o@x" }, shape: Shape.Circle(0.1), note: nil, flag: true, ratio: 1e21, big: -9223372036854775807 }
let t = json.encode_with(Mixed, m, json.Options { indent: 2 }) ?? "bad"
print(t)
match json.parse(Mixed, t)
    ok(b) -> print(json.encode(Mixed, b) ?? "bad")
    err(e) -> print(e.message)
end
match json.parse(Mixed, ‹{"name": "x", "owner": {"id": 1.5}}›)
    ok(b) -> print("accepted")
    err(e) -> print(e.message)
end
match json.parse_value(‹[1, 2, ]›)
    ok(b) -> print("accepted")
    err(e) -> print(e.message)
end
"#;
    let src = program("", body);
    let win = run(&src);
    assert!(win.contains("\"ratio\": 1e21"), "{}", win);
    if !wsl_ok() {
        eprintln!("WSL is not available: skipping the Linux half");
        return;
    }
    let file = parse_source(&src).unwrap();
    let exe = std::env::temp_dir().join(format!("zila-json-linux-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    setup();
    compile_to_linux(&file, exe.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("compile error: {}", e));
    let p = exe.to_str().unwrap().replace('\\', "/");
    let wsl = format!("/mnt/{}{}", p[..1].to_ascii_lowercase(), &p[2..]);
    let name = exe.file_name().unwrap().to_str().unwrap().to_string();
    let script = format!("cp '{}' /tmp/{n} && chmod +x /tmp/{n} && /tmp/{n}; rm -f /tmp/{n}", wsl, n = name);
    let out = Command::new("wsl.exe").args(["-e", "sh", "-c", &script]).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let linux = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    assert_eq!(linux, win);
}

// ---------------------------------------------------------------------------
// The examples
// ---------------------------------------------------------------------------

#[test]
fn the_examples_that_mention_json_use_the_real_signatures() {
    let types = std::fs::read_to_string("examples/types.zila").unwrap();
    let file = parse_source(&types).unwrap_or_else(|e| panic!("{}", e));
    assert!(types.contains("#[json(\"full_name\")]") && types.contains("#[json(skip)]"));
    // the User type of the example decodes and encodes with the library
    let user = types.split("type User = record").nth(1).unwrap().split("\nend").next().unwrap();
    let src = prep(&format!(
        "package main\n\nuse json\n\ntype User = record{}\nend\n\nfunc main()\n    let u = User {{ id: 3, name: \"Ana Lima\", password: \"x\", email: \"a@b\" }}\n    print(json.encode(User, u) ?? \"bad\")\n    match json.parse(User, ‹{{\"id\": 4, \"full_name\": \"Bia\"}}›)\n        ok(v) -> print(v.id, v.name, v.email == nil)\n        err(e) -> print(e.message)\n    end\nend\n",
        user
    ));
    assert_eq!(run(&src), "{\"id\":3,\"full_name\":\"Ana Lima\",\"email\":\"a@b\"}\n4 Bia true\n");
    drop(file);
    let errors = std::fs::read_to_string("examples/errors.zila").unwrap();
    parse_source(&errors).unwrap_or_else(|e| panic!("{}", e));
}

#[test]
fn the_json_example_prints_what_the_docs_say() {
    let src = std::fs::read_to_string("examples/json.zila").unwrap();
    let out = run(&src);
    let want = "port: 8080\nhost: example.org\nproxy: none\n\
                {\n  \"port\": 8080,\n  \"host_name\": \"example.org\",\n  \"proxy\": null,\n  \"tags\": [\n    \"a\",\n    \"b\"\n  ]\n}\n\
                bad config: expected int, found text at $.port\n[1,{\"a\":null}]\n";
    assert_eq!(out, want);
}
