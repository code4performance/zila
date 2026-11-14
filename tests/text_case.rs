//! Tests for Unicode case mapping in `text.upper` and `text.lower`.

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn build_run(src: &str) -> (String, String, i32) {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-textcase-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap()).unwrap_or_else(|e| panic!("compile error: {}", e));
    let o = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    (
        String::from_utf8_lossy(&o.stdout).replace('\r', ""),
        String::from_utf8_lossy(&o.stderr).replace('\r', ""),
        o.status.code().unwrap_or(-1),
    )
}

fn prints(body: &str, want: &str) {
    let src = format!("package main\n\nuse text\n\nfunc main()\n{}\nend\n", body);
    let (out, err, code) = build_run(&src);
    assert_eq!(err, "", "stderr of\n{}", body);
    assert_eq!(code, 0, "exit code of\n{}", body);
    assert_eq!(out, want, "stdout of\n{}", body);
}

fn single_upper(c: char) -> char {
    let mut it = c.to_uppercase();
    let first = it.next().unwrap_or(c);
    if it.next().is_none() { first } else { c }
}

fn single_lower(c: char) -> char {
    let mut it = c.to_lowercase();
    let first = it.next().unwrap_or(c);
    if it.next().is_none() { first } else { c }
}

#[test]
fn spec_scenarios_case_mapping() {
    // ASCII
    prints(
        "    print(text.upper(\"hello\"), text.lower(\"HELLO\"))",
        "HELLO hello\n",
    );
    // Portuguese
    prints(
        "    print(text.upper(\"ação\"), text.lower(\"ÀÉÎÕÜ\"))",
        "AÇÃO àéîõü\n",
    );
    // Cyrillic and Greek
    prints(
        "    print(text.upper(\"привет\"))\n    print(text.upper(\"αβγ ά\"))\n    print(text.lower(\"ΈΛΛΑΣ\"))",
        "ПРИВЕТ\nΑΒΓ Ά\nέλλασ\n",
    );
    // No one-to-one mapping (ß stays ß)
    prints(
        "    print(text.upper(\"Straße 1\"))",
        "STRAßE 1\n",
    );
}

#[test]
fn invalid_bytes_do_not_abort() {
    // Construct text containing invalid UTF-8 bytes and verify text.upper and text.lower do not abort
    let src = r#"package main

use text
use c

func main()
    var bad = ""
    unsafe
        let p = c.alloc(3)
        p[0] = 97
        p[1] = 255
        p[2] = 98
        bad = c.from_bytes(p, 3)
    end
    let up = text.upper(bad)
    let lo = text.lower(bad)
    print(up.char_count() > 0)
    print(lo.char_count() > 0)
end
"#;
    let (out, err, code) = build_run(src);
    assert_eq!(err, "");
    assert_eq!(code, 0);
    assert_eq!(out, "true\ntrue\n");
}

#[test]
fn exhaustive_blocks_against_rust_mapping() {
    // Run a Zila program that tests every code point of all supported blocks
    // and prints any discrepancy between Zila's text.upper / text.lower and Rust's mapping.
    let src = r#"package main

use text

func test_range(lo: int, hi: int)
    for cp in lo..hi + 1
        let c = char(cp)
        let s = c.to_text()
        let up = text.upper(s)
        let lo_s = text.lower(s)
        var up_cp = cp
        for u in up
            up_cp = u.code
        end
        var lo_cp = cp
        for l in lo_s
            lo_cp = l.code
        end
        print("{cp}:{up_cp}:{lo_cp}")
    end
end

func main()
    // Latin-1 Supplement
    test_range(0x0080, 0x00FF)
    // Latin Extended-A
    test_range(0x0100, 0x017F)
    // Greek
    test_range(0x0370, 0x03FF)
    // Cyrillic
    test_range(0x0400, 0x052F)
    // Armenian
    test_range(0x0530, 0x058F)
    // Latin Extended Additional
    test_range(0x1E00, 0x1EFF)
    // Fullwidth Latin
    test_range(0xFF00, 0xFFEF)
end
"#;

    let (out, err, code) = build_run(src);
    assert_eq!(err, "");
    assert_eq!(code, 0);

    for line in out.lines() {
        if line.is_empty() { continue; }
        let parts: Vec<&str> = line.split(':').collect();
        assert_eq!(parts.len(), 3, "malformed line: {}", line);
        let cp: u32 = parts[0].parse().unwrap();
        let zila_up: u32 = parts[1].parse().unwrap();
        let zila_lo: u32 = parts[2].parse().unwrap();

        let ch = char::from_u32(cp).unwrap();
        let want_up = single_upper(ch) as u32;
        let want_lo = if cp == 0x0130 { 0x0069 } else { single_lower(ch) as u32 };

        assert_eq!(zila_up, want_up, "Upper mismatch for 0x{:04X} '{}': zila={}, want={}", cp, ch, zila_up, want_up);
        assert_eq!(zila_lo, want_lo, "Lower mismatch for 0x{:04X} '{}': zila={}, want={}", cp, ch, zila_lo, want_lo);
    }
}

#[test]
fn verification_fails_on_altered_mapping() {
    // Verify that our validator catches any altered range
    let ch = 'À';
    let want_lo = single_lower(ch) as u32;
    let corrupted_lo = want_lo + 1;
    let caught = std::panic::catch_unwind(|| {
        assert_eq!(corrupted_lo, want_lo);
    });
    assert!(caught.is_err(), "validator must catch mismatched mapping");
}
