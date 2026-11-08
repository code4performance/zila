//! The `set` library. Programs are compiled to native executables and run (skipped when the
//! Visual Studio build tools are missing).

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn native(src: &str) -> Result<Option<String>, String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-sets-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") => {
            eprintln!("no native toolchain: skipping: {}", e);
            return Ok(None);
        }
        Err(e) => return Err(e),
    }
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    assert_eq!(out.status.code(), Some(0), "exit code; output: {}", String::from_utf8_lossy(&out.stdout));
    Ok(Some(String::from_utf8_lossy(&out.stdout).replace('\r', "")))
}

fn main_of(extra: &str, body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nuse set\n\n{}\nfunc main()\n{}end\n", extra, indented)
}

fn expect(body: &str, want: &str) {
    let src = main_of("", body);
    if let Some(out) = native(&src).unwrap_or_else(|e| panic!("compile error: {}", e)) {
        assert_eq!(out, want, "native output");
    }
}

#[test]
fn add_has_and_len() {
    expect(
        "let a: set.Set<text> = set.empty()\nprint(a.len)\nprint(a.add(\"a\"))\nprint(a.add(\"a\"))\nprint(a.add(\"b\"))\nprint(a.len)\nprint(a.has(\"a\"))\nprint(a.has(\"z\"))\nlet b = set.from_list([3, 1, 3])\nprint(b.len)\n",
        "0\ntrue\nfalse\ntrue\n2\ntrue\nfalse\n2\n",
    );
}

#[test]
fn remove_and_clear() {
    expect(
        "let a = set.from_list([\"a\", \"b\", \"c\"])\nprint(a.remove(\"a\"))\nprint(a.remove(\"a\"))\nprint(a.len)\nprint(a.has(\"a\"))\na.clear()\nprint(a.len)\nprint(a.add(\"a\"))\nprint(a.len)\n",
        "true\nfalse\n2\nfalse\n0\ntrue\n1\n",
    );
}

#[test]
fn union_intersection_difference_and_subset() {
    expect(
        "let a = set.from_list([1, 2, 3])\nlet b = set.from_list([3, 4])\nprint(set.union(a, b).len)\nlet i = set.intersect(a, b)\nprint(i.len)\nprint(i.has(3))\nlet d = set.difference(a, b)\nprint(d.len)\nprint(d.has(1))\nprint(d.has(3))\nprint(set.is_subset(i, a))\nprint(set.is_subset(a, b))\nlet e: set.Set<int> = set.empty()\nprint(set.is_subset(e, a))\nprint(a.len)\nprint(b.len)\n",
        "4\n1\ntrue\n2\ntrue\nfalse\ntrue\nfalse\ntrue\n3\n2\n",
    );
}

#[test]
fn to_list_keeps_the_order_of_first_insertion() {
    expect(
        "let a: set.Set<text> = set.empty()\na.add(\"c\")\na.add(\"a\")\na.add(\"b\")\na.add(\"a\")\nvar out = \"\"\nfor x in a.to_list()\n    out = out + x\nend\nprint(out)\na.remove(\"c\")\na.add(\"c\")\nvar again = \"\"\nfor x in a.to_list()\n    again = again + x\nend\nprint(again)\n",
        "cab\nabc\n",
    );
}

#[test]
fn sets_are_values_and_ref_shares_them() {
    let src = main_of(
        "func fill(s: set.Set<int>)
    s.add(7)
end
func fill_ref(ref s: set.Set<int>)
    s.add(8)
end
",
        "let s: set.Set<int> = set.empty()
fill(s)
print(s.has(7))
fill_ref(ref s)
print(s.has(8))
",
    );
    if let Some(out) = native(&src).unwrap() {
        assert_eq!(out, "false
true
");
    }
}

#[test]
fn a_hundred_thousand_elements() {
    expect(
        "let s: set.Set<int> = set.empty()\nvar i = 0\nwhile i < 100000\n    s.add(i * 3)\n    i += 1\nend\nvar found = 0\nvar j = 0\nwhile j < 300000\n    if s.has(j)\n        found += 1\n    end\n    j += 1\nend\nprint(s.len)\nprint(found)\n",
        "100000\n100000\n",
    );
}

#[test]
fn sets_do_not_leak() {
    let src = r#"package main
use set

func work(n: int) -> int
    let a: set.Set<text> = set.empty()
    for i in 0..20
        a.add("k {i}")
    end
    a.remove("k 3")
    let b = set.from_list(["k 1", "k 2", "x {n}"])
    let u = set.union(a, b)
    let c = set.intersect(a, b)
    let d = set.difference(a, b)
    var total = u.len + c.len + d.len
    for x in u.to_list()
        total += x.len
    end
    a.clear()
    return total
end

func main()
    let warm = work(1)
    let before = live_objects()
    var total = 0
    for i in 0..5000
        total += work(i)
    end
    let growth = live_objects() - before
    print("{growth}")
end
"#;
    if let Some(out) = native(src).unwrap() {
        assert_eq!(out, "0
");
    }
}

#[test]
fn only_hash_types_are_elements() {
    let src = main_of("", "let a: set.Set<float> = set.empty()\nprint(a.len)\n");
    match native(&src) {
        Err(e) => assert!(e.contains("float cannot be a map key because it is not Hash"), "{}", e),
        Ok(None) => {}
        Ok(Some(_)) => panic!("a set of floats must not compile"),
    }
}

#[test]
fn record_elements() {
    let src = main_of(
        "type P = record\n    x: int\n    y: int\nend\n",
        "let s = set.from_list([P { x: 1, y: 2 }, P { x: 1, y: 2 }, P { x: 2, y: 1 }])\nprint(s.len)\nprint(s.has(P { x: 2, y: 1 }))\nlet t = set.from_list([P { x: 2, y: 1 }, P { x: 9, y: 9 }])\nprint(set.union(s, t).len)\nprint(set.intersect(s, t).len)\nprint(set.difference(s, t).len)\n",
    );
    if let Some(out) = native(&src).unwrap_or_else(|e| panic!("compile error: {}", e)) {
        assert_eq!(out, "2\ntrue\n3\n1\n1\n");
    }
}

#[test]
fn the_sets_example_runs() {
    let src = std::fs::read_to_string("examples/sets.br").unwrap();
    if let Some(out) = native(&src).unwrap_or_else(|e| panic!("compile error: {}", e)) {
        assert!(out.contains("in both: the lazy dog") && out.contains("new: 3\nnew: 1\nnew: 2\n"), "{}", out);
    }
}
