//! `char`, text as UTF-8 (`len` in bytes, `slice` by bytes with boundary checks, `byte`, `bytes`,
//! `char_count`, `for c in s`), the removal of `s[i]` and `chr`, and the text library on
//! characters. Programs are compiled to native Windows executables and run (skipped without the
//! Visual Studio build tools); the Linux build shares the same runtime and is exercised by
//! `tests/linux.rs`.

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    out: String,
    err: String,
    code: i32,
}

fn build_run(src: &str) -> Option<Ran> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-textchar-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") || e.contains("vcvars64") => {
            eprintln!("no native toolchain: skipping");
            return None;
        }
        Err(e) => panic!("compile error: {}", e),
    }
    let o = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    Some(Ran {
        out: String::from_utf8_lossy(&o.stdout).replace('\r', ""),
        err: String::from_utf8_lossy(&o.stderr).replace('\r', ""),
        code: o.status.code().unwrap_or(-1),
    })
}

fn main_of(uses: &str, body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\n{}\n\nfunc main()\n{}end\n", uses, indented)
}

/// The program must run to the end and print exactly `want`.
fn prints(uses: &str, body: &str, want: &str) {
    let Some(r) = build_run(&main_of(uses, body)) else { return };
    assert_eq!(r.err, "", "stderr of\n{}", body);
    assert_eq!(r.out, want, "stdout of\n{}", body);
    assert_eq!(r.code, 0);
}

/// The program must abort with `message` (at any line) and exit with status 101.
fn aborts(uses: &str, body: &str, message: &str) {
    let Some(r) = build_run(&main_of(uses, body)) else { return };
    assert!(r.err.starts_with("abort: ") && r.err.ends_with(&format!(": {}\n", message)), "stderr {:?} of\n{}", r.err, body);
    assert_eq!(r.code, 101, "{}", body);
}

/// The compile error of a program that must not compile.
fn compile_err(uses: &str, body: &str) -> String {
    let file = parse_source(&main_of(uses, body)).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-textchar-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => panic!("expected a compile error for\n{}", body),
        Err(e) => e,
    }
}

// ---- the char type ----

#[test]
fn char_literals_compare_and_print() {
    prints(
        "",
        r#"let c: char = 'ç'
println(c == 'ç', c < 'z', c > 'z', c != 'a')
print('ç')
let n = 'ã'
println("{n}o")
println("a" + 'ç' + "b")
println('x' + "y")"#,
        "true false true true\nç\não\naçb\nxy\n",
    );
}

#[test]
fn char_has_no_implicit_int() {
    let e = compile_err("", "let c = 'a'\nlet n: int = c");
    assert!(e.contains("char") && e.contains("int"), "{}", e);
    let e = compile_err("", "let c = 'a'\nlet n = c + 1");
    assert!(e.contains("char"), "{}", e);
}

#[test]
fn char_conversions() {
    prints(
        "",
        r#"println('ç'.code, char(231).to_text(), 'ç'.len_utf8())
println('a'.len_utf8(), '€'.len_utf8(), char(0x1F600).len_utf8())
println('a'.is_ascii(), 'ç'.is_ascii())
println(char(0x1F600).to_text())
println(char(0).code, char(0x10FFFF).code, char(0xD7FF).code, char(0xE000).code)
let x = 233
println(char(x).to_text())"#,
        "231 ç 2\n1 3 4\ntrue false\n😀\n0 1114111 55295 57344\né\n",
    );
}

#[test]
fn char_aborts_on_an_invalid_code_point() {
    aborts("", "var n = 55296\nprint(char(n).code)", "char(55296) is not a valid character");
    aborts("", "var n = 1114112\nprint(char(n).code)", "char(1114112) is not a valid character");
    aborts("", "var n = 0 - 1\nprint(char(n).code)", "char(-1) is not a valid character");
}

#[test]
fn char_in_lists_records_parameters_match_and_maps() {
    prints(
        r#"type P = record
    first: char
    n: int
end

func twice(c: char) -> text
    return c.to_text() + c
end

func shout(c: char) -> text
    return "{c}!"
end

func kind(c: char) -> text
    match c
        'a' -> return "letter"
        'ç' -> return "accent"
        _ -> return "other"
    end
end"#,
        r#"let cs: []char = ['a', 'ç', '😀']
println(cs.len, cs[1] == 'ç', cs[2].code)
let r = P { first: 'ã', n: 2 }
println(r.first, r.n)
println(twice('ç'), shout('ã'))
let k: Map<char, int> = {}
k['a'] = 1
k['ç'] = 2
k['a'] = 3
println(k['a'], k['ç'], k.len)
println(kind('a'), kind('ç'), kind('?'))"#,
        "3 true 128512
ã 2
çç ã!
3 2 2
letter accent other
",
    );
}

// ---- indexing and chr are gone ----

#[test]
fn text_cannot_be_indexed() {
    let e = compile_err("", "let s = \"abc\"
let b = s[0]");
    assert!(e.starts_with("main.zila:7: text cannot be indexed"), "{}", e);
    assert!(e.contains("bytes()") && e.contains("slice") && e.contains("for c in s"), "{}", e);
    // lists and maps still index
    prints("", "let xs = [10, 20]
let m = { \"a\": 5 }
println(xs[1], m[\"a\"])", "20 5
");
}

#[test]
fn chr_is_gone() {
    let e = compile_err("", "print(chr(65))");
    assert!(e.contains("unknown function chr") && e.contains("char(n).to_text()"), "{}", e);
}

// ---- text as bytes and characters ----

#[test]
fn len_counts_bytes_and_char_count_characters() {
    prints(
        "",
        r#"let s = "ação"
println(s.len, s.char_count(), s.bytes().len, s.byte(1))
println("".len, "".char_count(), "".bytes().len)
println("😀".len, "😀".char_count(), "coração".len, "coração".char_count(), "São Paulo".char_count(), "não".char_count())
let b = s.bytes()
b[0] = 66
println(s, b[0], b.len)"#,
        "6 4 6 195\n0 0 0\n4 1 9 7 9 3\nação 66 6\n",
    );
}

#[test]
fn byte_aborts_out_of_range_with_the_position() {
    aborts("", "var i = 3\nprint(\"abc\".byte(i))", "byte position 3 is out of range");
    aborts("", "var i = 0 - 2\nprint(\"abc\".byte(i))", "byte position -2 is out of range");
}

#[test]
fn slice_cuts_by_bytes_on_character_boundaries() {
    prints(
        "",
        r#"let s = "ação"
println(s.slice(0, 3), s.slice(3, 6), s.slice(0, 1), s.slice(1, 5), s.slice(0, 6) == s)
println(s.slice(6, 6) == "", s.slice(0, 0) == "", "abc".slice(1, 3))"#,
        "aç ão a çã true
true true bc
",
    );
}

#[test]
fn slice_aborts_with_the_failing_position() {
    aborts("", "var b = 2\nprint(\"ação\".slice(0, b))", "text position 2 is not on a character boundary");
    aborts("", "var a = 4\nprint(\"ação\".slice(a, 6))", "text position 4 is not on a character boundary");
    aborts("", "var b = 9\nprint(\"abc\".slice(1, b))", "text position 9 is out of range");
    aborts("", "var a = 0 - 1\nprint(\"abc\".slice(a, 2))", "text position -1 is out of range");
    aborts("", "var a = 7\nprint(\"abc\".slice(a, 9))", "text position 7 is out of range");
    aborts("", "var a = 3\nvar b = 1\nprint(\"abc\".slice(a, b))", "slice start 3 is after the slice end");
}

#[test]
fn for_walks_the_characters() {
    prints(
        "",
        r#"var codes = ""
for c in "ação"
    codes = codes + "{c.code} "
end
println(codes)
var runs = 0
for c in ""
    runs += 1
end
println(runs)
var seen = ""
for c in "a😀bç"
    if c == 'b'
        continue
    end
    seen = seen + "[{c}]"
    if c == 'ç'
        break
    end
end
println(seen)
var s = "xyz"
var out = ""
for c in s
    s = "changed"
    out = out + c.to_text()
end
println(out, s)"#,
        "97 231 227 111 
0
[a][😀][ç]
xyz changed
",
    );
}

#[test]
fn invalid_bytes_decode_as_replacement_characters() {
    prints(
        "use c",
        r#"var bad = ""
unsafe
    let p = c.alloc(3)
    p[0] = 97
    p[1] = 255
    p[2] = 98
    bad = c.from_bytes(p, 3)
end
var codes = ""
for ch in bad
    codes = codes + "{ch.code} "
end
println(codes, bad.char_count(), bad.len)
let cut = "aç".__raw_slice(0, 2)
var after = ""
for ch in cut
    after = after + "{ch.code} "
end
println(after, cut.char_count())"#,
        "97 65533 98  3 3\n97 65533  2\n",
    );
}

#[test]
fn defer_and_loops_over_text_nest() {
    prints(
        r#"func walk(s: text) -> int
    var n = 0
    for a in s
        for b in s
            if a == b
                n += 1
            end
        end
    end
    return n
end

func tidy(s: text)
    for c in s
        defer println("bye {c}")
    end
    println("done")
end"#,
        r#"println(walk("açã"))
tidy("ab")"#,
        "3\ndone\nbye b\nbye a\n",
    );
}

#[test]
fn from_bytes_validates_utf8() {
    prints(
        "use text",
        r#"let good: []byte = [97, 98]
println(text.from_bytes(good) ?? "bad")
let accents = "ação".bytes()
println(text.from_bytes(accents) ?? "bad")
let cases: [][]byte = [[97, 255], [195], [192, 128], [237, 160, 128], [240, 159, 152], [244, 144, 128, 128], [128], [226, 130, 172]]
for b in cases
    match text.from_bytes(b)
        ok(t) -> println("ok {t}")
        err(e) -> println(e.message)
    end
end"#,
        "ab\nação\nnot valid UTF-8 at byte 1\nnot valid UTF-8 at byte 0\nnot valid UTF-8 at byte 0\nnot valid UTF-8 at byte 0\nnot valid UTF-8 at byte 0\nnot valid UTF-8 at byte 0\nnot valid UTF-8 at byte 0\nok €\n",
    );
}

#[test]
fn the_text_library_works_on_characters() {
    prints(
        "use text",
        r#"println(text.reverse("ação"), text.reverse("São Paulo"), text.reverse("😀a"), text.reverse(""))
for w in ["ação", "coração", "São Paulo", "não", "😀", ""]
    if text.reverse(text.reverse(w)) != w or text.from_chars(text.chars(w)) != w
        println("broken {w}")
    end
end
println(text.chars("ação").len, text.from_chars(text.chars("ação")))
println(text.upper("Straße 1"), text.lower("AÇÃO"), text.upper("coração"))
println(text.pad_left("7", 3, "0"), text.pad_right("ab", 4, "."), text.pad_right("ação", 6, "."), text.pad_left("ação", 2, "."))
println("[" + text.pad_left("São", 5, " ") + "]", "[" + text.pad_right("😀", 3, "·") + "]")
println(text.pad_left("x", 3, "ç"))"#,
        "oãça oluaP oãS a😀 \n4 ação\nSTRAßE 1 ação CORAÇÃO\n007 ab.. ação.. ação\n[  São] [😀··]\nççx\n",
    );
}

#[test]
fn padding_needs_a_one_character_fill() {
    aborts("use text", "println(text.pad_left(\"a\", 4, \"ab\"))", "the padding fill must be exactly one character");
    aborts("use text", "println(text.pad_right(\"a\", 4, \"\"))", "the padding fill must be exactly one character");
}

#[test]
fn nothing_here_leaks() {
    prints(
        "use text\nuse list\n\nfunc c_of(n: int) -> char\n    return char(97 + n % 26)\nend",
        r#"var before = 0
for round in 0..1000
    let s = "coração {round} 😀"
    let b = s.bytes()
    let n = s.char_count()
    let piece = s.slice(0, 3)
    var k = 0
    for c in s
        k += c.len_utf8()
    end
    let cs: []char = ['a', 'ç', c_of(round)]
    let t = char(231).to_text() + 'x' + cs[1] + "{cs[2]}"
    let words = text.split("São Paulo, não, ação", ",")
    let joined = text.join(words, "|")
    let rev = text.reverse(joined)
    let padded = text.pad_left(rev, 40, "·")
    let again = text.from_bytes(s.bytes()) ?? ""
    let rep = text.replace(padded, "ç", "c")
    let tr = text.trim("  ação  ")
    let chs = text.chars(s)
    let back = text.from_chars(chs)
    if b.len != s.len or k != s.len or n < 5 or piece.len != 3 or t.len < 5 or again != s or back != s or tr != "ação" or rep.len == 0
        println("bad")
    end
    if round == 1
        before = live_objects()
    end
    if round == 999
        println("grew {live_objects() - before}")
    end
end"#,
        "grew 0\n",
    );
}

#[test]
fn slicing_does_not_depend_on_the_text_length() {
    // 100000 slices of a 1 MB text must cost about as much as 100000 slices of a short one
    let body = |text: &str| {
        format!(
            "var s = \"ação \"\nfor i in 0..{}\n    s = s + s\nend\nvar total = 0\nfor i in 0..100000\n    total += s.slice(0, 3).len\nend\nprintln(total, s.len > 1)\n",
            text
        )
    };
    let time = |n: &str| {
        let start = std::time::Instant::now();
        let r = build_run(&main_of("", &body(n)));
        r.map(|r| {
            assert_eq!(r.out, "300000 true\n", "{}", r.err);
            start.elapsed()
        })
    };
    // 2^17 * 7 bytes is about 0.9 MB: the doubling loop dominates both runs, so the slice loop must not add much
    if let (Some(short), Some(big)) = (time("1"), time("17")) {
        assert!(big < short * 20 + std::time::Duration::from_secs(2), "short {:?} big {:?}", short, big);
    }
}

#[test]
fn text_building_compiles_for_linux_too() {
    let src = main_of(
        "use text",
        "let s = \"ação\"\nvar n = 0\nfor c in s\n    n += c.code\nend\nprintln(n, s.bytes().len, s.char_count(), s.slice(0, 3), char(231).to_text(), text.reverse(s), s.byte(0))\n",
    );
    let file = parse_source(&src).unwrap();
    let out = std::env::temp_dir().join(format!("zila-textchar-linux-{}", std::process::id()));
    let opts = zila::codegen_x64::CompileOptions { optimize: true, test_mode: false, debug: false };
    zila::codegen_x64::compile_to_linux(&file, out.to_str().unwrap(), &opts).unwrap_or_else(|e| panic!("linux build failed: {}", e));
    let _ = std::fs::remove_file(&out);
}

#[test]
fn the_unicode_example_prints_what_the_docs_say() {
    let src = std::fs::read_to_string("examples/unicode.zila").unwrap();
    let Some(r) = build_run(&src) else { return };
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        r.out,
        "ação: len 6, char_count 4, reversed oãça
  a U+97 (1 bytes)
  ç U+231 (2 bytes)
  ã U+227 (2 bytes)
  o U+111 (1 bytes)
coração: len 9, char_count 7, reversed oãçaroc
  c U+99 (1 bytes)
  o U+111 (1 bytes)
  r U+114 (1 bytes)
  a U+97 (1 bytes)
  ç U+231 (2 bytes)
  ã U+227 (2 bytes)
  o U+111 (1 bytes)
São Paulo: len 10, char_count 9, reversed oluaP oãS
  S U+83 (1 bytes)
  ã U+227 (2 bytes)
  o U+111 (1 bytes)
    U+32 (1 bytes)
  P U+80 (1 bytes)
  a U+97 (1 bytes)
  u U+117 (1 bytes)
  l U+108 (1 bytes)
  o U+111 (1 bytes)
slice(0, 3) = aç
first byte 97, bytes 6
pad: [ação..]
"
        .replace('\r', "")
    );
}

#[test]
fn split_over_a_megabyte_text_stays_linear() {
    let start = std::time::Instant::now();
    prints(
        "use text",
        r#"var s = "alpha,beta,gamma,delta,"
for i in 0..16
    s = s + s
end
let parts = text.split(s, ",")
println(s.len > 1000000, parts.len, parts[0], parts[parts.len - 2])"#,
        "true 262145 alpha delta\n",
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(30), "took {:?}", start.elapsed());
}
