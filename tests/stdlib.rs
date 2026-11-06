//! The text and list libraries. Programs are
//! compiled to native executables and run (skipped when the Visual Studio build tools are
//! missing); the interpreter is exercised through the command-line tool.

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn run(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-stdlib-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") => {
            eprintln!("no native toolchain: skipping: {}", e);
            return None;
        }
        Err(e) => panic!("compile error: {}", e),
    }
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "");
    Some((text, out.status.code().unwrap_or(-1)))
}

/// The compile error of a program that must not compile.


fn expect(src: &str, want: &str) {
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, want, "program output");
    assert_eq!(code, 0);
}

fn main_of(uses: &str, body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}
", l)).collect();
    format!("package main

{}
type P = record
    name: text
    age: int
end

func main()
{}end
", uses, indented)
}

fn prints(uses: &str, body: &str, want: &str) {
    expect(&main_of(uses, body), want);
}

#[test]
fn lambda_results_are_inferred() {
    prints(
        "use list",
        r#"let doubled = list.map([1, 2, 3], func(n) n * 2 end)
let names = list.map([1, 2, 3], func(n) "n{n}" end)
let halves = list.map(doubled, func(n) float(n) / 2.0 end)
println(doubled[2], names[0], halves[1])
let odd = list.filter([1, 2, 3, 4, 5], func(n) n % 2 == 1 end)
println(odd.len, list.reduce(odd, 0, func(a, n) a + n end))
let joined = list.reduce(["a", "b"], "", func(a, s) a + s end)
println(joined)"#,
        "6 n1 2.0
3 9
ab
",
    );
}

fn compile_err(uses: &str, body: &str) -> String {
    let file = parse_source(&main_of(uses, body)).unwrap();
    let exe = std::env::temp_dir().join("brasa-stdlib-err.exe");
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => panic!("expected a compile error"),
        Err(e) => e,
    }
}

#[test]
fn lambda_inference_errors_are_clear() {
    let e = compile_err("use list", "let xs = list.map([1, 2], func(n) println(n) end)");
    assert!(e.contains("would be 'nothing'") && e.contains("map"), "{}", e);
}

#[test]
fn list_searching_and_building() {
    prints(
        "use list",
        r#"let none: []int = []
let hit = list.any([1, 2, 3], func(n) n == 2 end)
println(hit, list.any([1, 2], func(n) n > 5 end), list.any(none, func(n) true end))
println(list.all([2, 4], func(n) n % 2 == 0 end), list.all([2, 3], func(n) n % 2 == 0 end), list.all(none, func(n) false end))
println(list.find([5, 8, 12], func(n) n > 6 end), list.find([5, 8, 12], func(n) n > 60 end))
let r = list.reverse([1, 2, 3])
println(r[0], r[1], r[2])
let a = list.slice([1, 2, 3, 4], 0 - 5, 2)
let b = list.slice([1, 2, 3, 4], 3, 99)
let c = list.slice([1, 2, 3, 4], 3, 1)
println(a.len, a[1], b.len, b[0], c.len)
let d = list.concat([1, 2], [3])
println(d.len, d[2], list.range(2, 5).len, list.range(5, 2).len, list.range(2, 5)[2])"#,
        "true false false\ntrue false true\n1 -1\n3 2 1\n2 2 1 4 0\n3 3 3 0 4\n",
    );
}

#[test]
fn any_stops_at_the_first_match() {
    expect(
        r#"package main

use list

var calls = 0

func bump(n: int) -> bool
    calls += 1
    return n == 2
end

func main()
    let found = list.any([1, 2, 3, 4], func(n) bump(n) end)
    println(found, calls)
end
"#,
        "true 2
",
    );
}

#[test]
fn sorting_is_stable_and_scales() {
    prints(
        "use list
use text",
        r#"let nums = list.sort_by([3, 1, 2, 9, 0, 5, 5, 7], func(a, b) a < b end)
var out = ""
for n in nums
    out = out + "{n} "
end
println(out)
let people = [P { name: "a", age: 30 }, P { name: "b", age: 25 }, P { name: "c", age: 30 }, P { name: "d", age: 25 }]
let sorted = list.sort_by(people, func(x, y) x.age < y.age end)
var names = ""
for p in sorted
    names = names + p.name
end
println(names, people[0].name)
let words = list.sort_by(["pear", "apple", "fig"], func(a, b) text.compare(a, b) < 0 end)
println(words[0], words[1], words[2])
var seed = 12345
let big: []int = []
for i in 0..100000
    seed = (seed * 1103515245 + 12345) % 2147483648
    big.add(seed)
end
let s = list.sort_by(big, func(a, b) a < b end)
var ok = s.len == 100000
for i in 1..s.len
    if s[i - 1] > s[i]
        ok = false
    end
end
println(ok)"#,
        "0 1 2 3 5 5 7 9 \nbdac a\napple fig pear\ntrue\n",
    );
}

#[test]
fn text_searching() {
    prints(
        "use text",
        r#"println(text.index_of("hello world", "o w"), text.index_of("abc", "z"), text.index_of("abc", ""))
println(text.contains("abc", ""), text.contains("abc", "bc"), text.contains("abc", "cb"))
println(text.starts_with("report.txt", "rep"), text.ends_with("report.txt", ".md"), text.ends_with("report.txt", ".txt"))
println(text.starts_with("ab", "abc"), text.index_of("aaa", "aa"), text.compare("a", "b"), text.compare("b", "a"), text.compare("ab", "ab"), text.compare("a", "ab"))"#,
        "4 -1 0\ntrue true false\ntrue false true\nfalse 0 -1 1 0 -1\n",
    );
}

#[test]
fn text_splitting_and_joining() {
    prints(
        "use text",
        r#"let parts = text.split("a,b,,c", ",")
println(parts.len, "[{parts[2]}]", parts[3])
println(text.split("abc", ",").len, text.split("", ",").len, text.split("a--b--c", "--").len)
let none: []text = []
println(text.join(["x", "y", "z"], "-"), "[{text.join(none, "-")}]", text.join(["solo"], ", "))
let ls = text.lines("one\r\ntwo\nthree\n")
println(ls.len, ls[0], ls[1], ls[2])
println(text.lines("").len, text.lines("a\n\nb").len, text.lines("x").len)"#,
        "4 [] c\n1 1 3\nx-y-z [] solo\n3 one two three\n0 3 1\n",
    );
}

#[test]
fn empty_separator_is_a_panic() {
    let src = main_of("use text", "let p = text.split(\"abc\", \"\")\nprintln(p.len)");
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "panic: split needs a separator\n");
    assert_eq!(code, 101);
}

#[test]
fn text_trimming_replacing_and_padding() {
    prints(
        "use text",
        r#"println("[{text.trim("  \t hi there \n")}]", "[{text.trim_start("  x ")}]", "[{text.trim_end(" x  ")}]", "[{text.trim("   ")}]")
println(text.replace("aaaa", "aa", "b"), text.replace("hello", "l", "L"), text.replace("abc", "", "x"), "[{text.replace("abc", "abc", "")}]")
println(text.repeat("ab", 3), "[{text.repeat("ab", 0)}]", "[{text.repeat("x", 0 - 2)}]")
println(text.pad_left("7", 3, "0"), text.pad_right("ab", 4, "."), text.pad_left("long", 2, "0"))"#,
        "[hi there] [x ] [ x] []\nbb heLLo abc []\nababab [] []\n007 ab.. long\n",
    );
}

#[test]
fn text_case_and_reverse() {
    prints(
        "use text",
        r#"println(text.upper("Straße 1 abc"), text.lower("HeLLo WORLD"), text.reverse("abc"), "[{text.reverse("")}]")"#,
        "STRAßE 1 ABC hello world cba []\n",
    );
}

#[test]
fn parsing_whole_numbers() {
    prints(
        "use text",
        r#"println(text.parse_int("42") ?? 0 - 1, text.parse_int("-7") ?? 0, text.parse_int("+5") ?? 0)
println(text.parse_int("12x") ?? 0 - 1, text.parse_int("") ?? 0 - 1, text.parse_int("-") ?? 0 - 1)
println(text.parse_int("9223372036854775807") ?? 0, text.parse_int("-9223372036854775808") ?? 5)
println(text.parse_int("9223372036854775808") ?? 0 - 1, text.parse_int("oops") ?? 0)
match text.parse_int("12x")
    ok(n) -> println("ok {n}")
    err(e) -> println(e.message)
end"#,
        "42 -7 5\n-1 -1 -1\n9223372036854775807 -9223372036854775808\n-1 0\nnot a whole number: \"12x\"\n",
    );
}

#[test]
fn joining_many_pieces_is_fast() {
    let started = std::time::Instant::now();
    prints(
        "use text\nuse list",
        r#"let pieces = list.map(list.range(0, 100000), func(n) "{n}" end)
let joined = text.join(pieces, ",")
let back = text.split(joined, ",")
println(joined.len, back.len, back[99999])
println(text.replace(joined, ",", ";").len)"#,
        "588889 100000 99999\n588889\n",
    );
    assert!(started.elapsed().as_secs() < 60, "took {:?}", started.elapsed());
}

#[test]
fn the_libraries_do_not_leak() {
    prints(
        "use text\nuse list",
        r#"var before = 0
for round in 0..500
    let words = text.split("alpha, beta,gamma ,delta", ",")
    let trimmed = list.map(words, func(w) text.trim(w) end)
    let joined = text.join(trimmed, "-")
    let up = text.upper(text.replace(joined, "-", "+"))
    let sorted = list.sort_by(trimmed, func(a, b) text.compare(a, b) < 0 end)
    let rev = list.reverse(list.filter(sorted, func(w) w.len > 4 end))
    let people = [P { name: up, age: round + 5 }, P { name: joined, age: 1 }]
    let by_age = list.sort_by(people, func(a, b) a.age < b.age end)
    let sl = text.lines("a\r\nb\n")
    if rev.len != 3 or by_age[0].age != 1 or sl.len != 2
        println("bad")
    end
    if round == 1
        before = live_objects()
    end
    if round == 499
        println("grew {live_objects() - before}")
    end
end"#,
        "grew 0\n",
    );
}

#[test]
fn parsing_floats() {
    prints(
        "use text",
        r#"println(text.parse_float("1.5") ?? 0.0, text.parse_float("-2e3") ?? 0.0, text.parse_float("0.1") ?? 0.0, text.parse_float(".5") ?? 0.0, text.parse_float("5.") ?? 0.0)
println(text.parse_float("abc") ?? 7.5, text.parse_float("") ?? 7.5, text.parse_float("1e") ?? 7.5, text.parse_float("1.2.3") ?? 7.5, text.parse_float("--1") ?? 7.5)
println(text.parse_float("inf") ?? 0.0, text.parse_float("-inf") ?? 0.0, text.parse_float("NaN") ?? 0.0, text.parse_float("-0") ?? 1.0)
println(text.parse_float("1e400") ?? 0.0, text.parse_float("1e-400") ?? 1.0, text.parse_float("5e-324") ?? 0.0, text.parse_float("1.7976931348623157e308") ?? 0.0)
println(text.parse_float("123456789.123456789") ?? 0.0, text.parse_float("0.30000000000000004") ?? 0.0, text.parse_float("  1") ?? 7.5)"#,
        "1.5 -2000.0 0.1 0.5 5.0\n7.5 7.5 7.5 7.5 7.5\ninf -inf NaN -0.0\ninf 0.0 5e-324 1.7976931348623157e308\n123456789.12345679 0.30000000000000004 7.5\n",
    );
}

/// `parse_float` against Rust's correctly rounded parser: the shortest forms of random bit
/// patterns, long random decimals, and decimals that sit exactly between two floats.
#[test]
fn parse_float_matches_the_reference_parser() {
    let mut inputs: Vec<String> = Vec::new();
    let mut x: u64 = 0x2545F4914F6CDD1D;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..400 {
        let f = f64::from_bits(next());
        if f.is_finite() {
            inputs.push(format!("{:e}", f));
        }
    }
    for _ in 0..400 {
        let r = next();
        let digits: String = (0..(1 + r % 30)).map(|i| char::from(b'0' + ((r >> (i % 50)) % 10) as u8)).collect();
        let point = (next() % (digits.len() as u64 + 1)) as usize;
        let exp = (next() % 700) as i64 - 350;
        inputs.push(format!("{}.{}e{}", &digits[..point], &digits[point..], exp));
    }
    // exact halfway points between neighbouring floats: (2m + 1) / 2^s written out in decimal
    for _ in 0..150 {
        let m = (1u128 << 52) + (next() as u128 % (1u128 << 52));
        let s = 1 + (next() % 20) as u32;
        let n = (2 * m + 1) * 5u128.pow(s);
        let digits = n.to_string();
        let digits = format!("{:0>width$}", digits, width = s as usize + 1);
        inputs.push(format!("{}.{}", &digits[..digits.len() - s as usize], &digits[digits.len() - s as usize..]));
    }
    inputs.extend(["0.1", "9007199254740993", "9007199254740992.5", "2.2250738585072011e-308", "4.9406564584124654e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "1.7976931348623158e308", "1.7976931348623159e308", "123456789012345678901234567890"].iter().map(|s| s.to_string()));
    let list: String = inputs.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(", ");
    let body = format!("let inputs = [{}]\nfor s in inputs\n    println(text.parse_float(s) ?? 0.0 - 1.0)\nend", list);
    let expected: String = inputs.iter().map(|s| format!("{}\n", brasa::float_text(s.parse::<f64>().unwrap()))).collect();
    let Some((out, code)) = run(&main_of("use text", &body)) else { return };
    assert_eq!(code, 0);
    for (i, (a, b)) in out.lines().zip(expected.lines()).enumerate() {
        assert_eq!(a, b, "input #{} {:?}", i, inputs[i]);
    }
    assert_eq!(out.lines().count(), inputs.len());
}


#[test]
fn the_interpreter_agrees_with_the_native_compiler() {
    let src = std::fs::read_to_string("examples/words.br").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_brasa-parse")).arg("examples/words.br").arg("--run").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let interp = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    assert!(interp.contains("top three: THE, DOG, FOX") && interp.contains("42 -1 39.98"), "{}", interp);
    if let Some((native, code)) = run(&src) {
        assert_eq!(code, 0);
        assert_eq!(native, interp);
    }
}
