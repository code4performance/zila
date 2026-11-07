//! The `sorted` library (sorted maps and sets). Programs are compiled to native executables and
//! run (skipped when the Visual Studio build tools are missing).

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static N: AtomicUsize = AtomicUsize::new(0);

fn native_timed(src: &str) -> Option<(String, Duration)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-sorted-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") => {
            eprintln!("no native toolchain: skipping: {}", e);
            return None;
        }
        Err(e) => panic!("compile error: {}", e),
    }
    let started = Instant::now();
    let out = Command::new(&exe).output().unwrap();
    let took = started.elapsed();
    let _ = std::fs::remove_file(&exe);
    assert_eq!(out.status.code(), Some(0), "exit code; output: {}", String::from_utf8_lossy(&out.stdout));
    Some((String::from_utf8_lossy(&out.stdout).replace('\r', ""), took))
}

fn native(src: &str) -> Option<String> {
    native_timed(src).map(|(o, _)| o)
}

const HELPERS: &str = r#"
type Task = record
    prio: int
    name: text
end

func show(xs: []int) -> text
    var out = "["
    var i = 0
    while i < xs.len
        if i > 0
            out += ","
        end
        out += "{xs[i]}"
        i += 1
    end
    return out + "]"
end

func shows(xs: []text) -> text
    var out = "["
    var i = 0
    while i < xs.len
        if i > 0
            out += ","
        end
        out += xs[i]
        i += 1
    end
    return out + "]"
end
"#;

fn program(extra: &str, body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nuse sorted\n{}\n{}\nfunc main()\n{}end\n", HELPERS, extra, indented)
}

/// Runs the body natively and checks the output.
fn expect(body: &str, want: &str) {
    let src = program("", body);
    if let Some(out) = native(&src) {
        assert_eq!(out, want, "native output");
    }
}

#[test]
fn put_get_has_and_replace() {
    expect(
        "let m: sorted.SortedMap<int, text> = sorted.int_map()\nprint(m.len)\nprint(m.put(2, \"b\"))\nprint(m.put(1, \"a\"))\nprint(m.put(2, \"B\"))\nprint(m.len)\nprint(m.get(2, \"?\"))\nprint(m.get(9, \"?\"))\nprint(m.has(1))\nprint(m.has(7))\n",
        "0\ntrue\ntrue\nfalse\n2\nB\n?\ntrue\nfalse\n",
    );
}

#[test]
fn remove_and_clear() {
    expect(
        "let m: sorted.SortedMap<int, int> = sorted.int_map()\nfor i in 0..10\nm.put(i, i)\nend\nprint(m.remove(3))\nprint(m.remove(3))\nprint(m.remove(0))\nprint(m.remove(9))\nprint(m.len)\nprint(show(m.keys()))\nm.clear()\nprint(m.len)\nprint(show(m.keys()))\nprint(m.put(5, 5))\nprint(m.len)\n",
        "true\nfalse\ntrue\ntrue\n7\n[1,2,4,5,6,7,8]\n0\n[]\ntrue\n1\n",
    );
}

#[test]
fn entries_come_out_in_key_order_whatever_the_insertion_order() {
    expect(
        "let m: sorted.SortedMap<int, text> = sorted.int_map()\nm.put(5, \"e\")\nm.put(1, \"a\")\nm.put(4, \"d\")\nm.put(2, \"b\")\nm.put(3, \"c\")\nprint(show(m.keys()))\nprint(shows(m.values()))\nfor e in m.entries()\nprint(\"{e.key}={e.value}\")\nend\n",
        "[1,2,3,4,5]\n[a,b,c,d,e]\n1=a\n2=b\n3=c\n4=d\n5=e\n",
    );
}

#[test]
fn text_keys_are_ordered_by_bytes_and_sets_can_be_ordered_by_a_function() {
    expect(
        "let t: sorted.SortedSet<text> = sorted.text_set()\nt.add(\"pear\")\nt.add(\"apple\")\nt.add(\"fig\")\nt.add(\"Zed\")\nprint(shows(t.to_list()))\nlet d: sorted.SortedSet<int> = sorted.set_by(func(a, b) a > b end)\nfor x in [3, 9, 1, 7]\nd.add(x)\nend\nprint(show(d.to_list()))\nprint(d.min() ?? 0)\nprint(d.max() ?? 0)\n",
        "[Zed,apple,fig,pear]\n[9,7,3,1]\n9\n1\n",
    );
}

#[test]
fn records_can_be_keys_ordered_by_a_field() {
    let src = program(
        "",
        "let m: sorted.SortedMap<Task, int> = sorted.map_by(func(a, b) a.prio < b.prio end)\nprint(m.put(Task { prio: 3, name: \"c\" }, 30))\nprint(m.put(Task { prio: 1, name: \"a\" }, 10))\nprint(m.put(Task { prio: 3, name: \"other\" }, 33))\nprint(m.len)\nfor e in m.entries()\nprint(\"{e.key.prio} {e.value}\")\nend\nprint(m.get(Task { prio: 1, name: \"?\" }, 0))\n",
    );
    let want = "true\ntrue\nfalse\n2\n1 10\n3 33\n10\n";
    if let Some(out) = native(&src) {
        assert_eq!(out, want, "native output");
    }
}

#[test]
fn nearest_keys_and_ranges() {
    expect(
        "let m: sorted.SortedMap<int, int> = sorted.int_map()\nm.put(20, 0)\nm.put(10, 0)\nm.put(30, 0)\nprint(m.min() ?? 0 - 1)\nprint(m.max() ?? 0 - 1)\nprint(m.floor(25) ?? 0 - 1)\nprint(m.ceil(25) ?? 0 - 1)\nprint(m.floor(20) ?? 0 - 1)\nprint(m.ceil(20) ?? 0 - 1)\nprint(m.floor(5) ?? 0 - 1)\nprint(m.ceil(35) ?? 0 - 1)\nprint(show(m.range(10, 30)))\nprint(show(m.range(11, 100)))\nprint(show(m.range(30, 10)))\nprint(show(m.range(0, 5)))\n",
        "10\n30\n20\n30\n20\n20\n-1\n-1\n[10,20]\n[20,30]\n[]\n[]\n",
    );
}

#[test]
fn missing_keys_are_errors_with_messages() {
    let src = program(
        "",
        "let m: sorted.SortedMap<int, int> = sorted.int_map()\nmatch m.min()\nok(v) -> print(\"min {v}\")\nerr(e) -> print(\"error: {e.message}\")\nend\nm.put(10, 1)\nmatch m.floor(5)\nok(v) -> print(\"floor {v}\")\nerr(e) -> print(\"error: {e.message}\")\nend\nmatch m.ceil(11)\nok(v) -> print(\"ceil {v}\")\nerr(e) -> print(\"error: {e.message}\")\nend\nmatch m.max()\nok(v) -> print(\"max {v}\")\nerr(e) -> print(\"error: {e.message}\")\nend\nlet s: sorted.SortedSet<int> = sorted.int_set()\nprint(show(s.to_list()))\nprint(s.min() ?? 0 - 1)\nprint(s.max() ?? 0 - 1)\n",
    );
    if let Some(out) = native(&src) {
        assert_eq!(
            out,
            "error: the collection is empty\nerror: no key at or below the given key\nerror: no key at or above the given key\nmax 10\n[]\n-1\n-1\n"
        );
    }
}

#[test]
fn set_operations_keep_the_order() {
    expect(
        "let a: sorted.SortedSet<int> = sorted.int_set()\nfor x in [3, 1, 2]\na.add(x)\nend\nlet b: sorted.SortedSet<int> = sorted.int_set()\nb.add(2)\nb.add(4)\nprint(show(sorted.union(a, b).to_list()))\nprint(show(sorted.intersect(a, b).to_list()))\nprint(show(sorted.difference(a, b).to_list()))\nprint(sorted.is_subset(sorted.intersect(a, b), a))\nprint(sorted.is_subset(a, b))\nprint(show(a.to_list()))\nprint(show(b.to_list()))\nprint(a.add(2))\nprint(a.remove(2))\nprint(a.remove(2))\nprint(a.has(1))\nprint(show(a.range(1, 3)))\nprint(a.floor(5) ?? 0 - 1)\nprint(a.ceil(2) ?? 0 - 1)\n",
        "[1,2,3,4]\n[2]\n[1,3]\ntrue\nfalse\n[1,2,3]\n[2,4]\nfalse\ntrue\nfalse\ntrue\n[1]\n3\n3\n",
    );
}

#[test]
fn the_tree_stays_balanced_for_ascending_and_descending_keys() {
    let body = "let m: sorted.SortedMap<int, int> = sorted.int_map()\nfor i in 0..100000\nm.put(i, i * 2)\nend\nvar i = 0\nwhile i < 100000\nm.remove(i)\ni += 2\nend\nvar ok = true\nfor j in 0..100000\nif m.has(j) != (j % 2 == 1)\nok = false\nend\nend\nlet ks = m.keys()\nfor j in 1..ks.len\nif ks[j - 1] >= ks[j]\nok = false\nend\nend\nprint(\"{m.len} {ok} {m.get(99999, 0)}\")\nlet d: sorted.SortedMap<int, int> = sorted.int_map()\nfor k in 0..100000\nd.put(100000 - k, k)\nend\nprint(\"{d.len} {d.min() ?? 0} {d.max() ?? 0}\")\n";
    let Some((out, took)) = native_timed(&program("", body)) else { return };
    assert_eq!(out, "50000 true 199998\n100000 1 100000\n");
    assert!(took < Duration::from_secs(20), "took {:?}", took);
}

#[test]
fn random_operations_agree_with_a_simple_model() {
    let body = "let m: sorted.SortedMap<int, int> = sorted.int_map()\nlet present: []bool = []\nfor i in 0..300\npresent.add(false)\nend\nvar seed = 12345\nvar bad = 0\nfor step in 0..4000\nseed = (seed * 1103515245 + 12345) % 2147483648\nlet k = seed / 16 % 300\nlet op = seed / 8 % 4\nif op < 2\nlet fresh = m.put(k, step)\nif fresh == present[k]\nbad += 1\nend\npresent[k] = true\nelse if op == 2\nlet gone = m.remove(k)\nif gone != present[k]\nbad += 1\nend\npresent[k] = false\nelse\nif m.has(k) != present[k]\nbad += 1\nend\nend\nend\nvar count = 0\nlet want: []int = []\nfor i in 0..300\nif present[i]\ncount += 1\nwant.add(i)\nend\nend\nprint(\"{bad} {m.len == count} {show(m.keys()) == show(want)}\")\n";
    expect(body, "0 true true\n");
}

#[test]
fn sorted_collections_do_not_leak() {
    let src = format!(
        "package main\n\nuse sorted\n\nfunc work(n: int) -> int\n    let m: sorted.SortedMap<text, int> = sorted.text_map()\n    for i in 0..40\n        m.put(\"k {{i}}\", i)\n    end\n    m.remove(\"k 3\")\n    m.remove(\"k 30\")\n    let s: sorted.SortedSet<text> = sorted.text_set()\n    for i in 0..20\n        s.add(\"s {{i}}\")\n    end\n    let t = sorted.union(s, sorted.intersect(s, s))\n    var total = m.len + t.len + m.keys().len + m.entries().len\n    for x in t.range(\"s 1\", \"s 5\")\n        total += x.len\n    end\n    if n % 2 == 0\n        m.clear()\n    end\n    return total\nend\n\nfunc main()\n    let warm = work(1)\n    let before = live_objects()\n    var total = 0\n    for i in 0..3000\n        total += work(i)\n    end\n    print(\"grew {{live_objects() - before}} {{total > 0}}\")\nend\n"
    );
    if let Some(out) = native(&src) {
        assert_eq!(out, "grew 0 true\n");
    }
}

#[test]
fn the_sorted_example_runs() {
    let src = std::fs::read_to_string("examples/sorted.br").unwrap();
    if let Some(out) = native(&src) {
        assert!(out.contains("leaderboard"), "{}", out);
    }
}
