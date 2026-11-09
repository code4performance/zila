//! The formatter (`--fmt`, `--fmt-check`) and the block layout the parser records for it.

use zila::fmt::format_source;
use zila::lexer::Tok;
use zila::parser::parse_with_layout;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn fmt(src: &str) -> String {
    format_source(src).unwrap_or_else(|e| panic!("format error: {}", e))
}

// ---- the layout the parser records ----

#[test]
fn the_parser_records_every_kind_of_block_and_no_function_types() {
    let src = r#"package t

type Shape = interface
    func area(self) -> float
end

type Color = enum
    Red, Green
end

type Box = record
    cb: func(int) -> bool
    n: int
end

extern "C" func puts(s: *u8) -> i32

func f(n: int)
    if n > 0
        print(1)
    else if n < 0
        print(2)
    else
        print(3)
    end
    while n > 0
        n -= 1
    end
    for i in 0..3
        print(i)
    end
    unsafe
        print(0)
    end
    let g = func(a) a + 1 end
    match n
        1 -> print(1)
        2 ->
            print(2)
            print(3)
        _ -> print(4)
    end
end
"#;
    let (_, layout, toks, _) = parse_with_layout(src).unwrap();
    let openers: Vec<&str> = layout
        .blocks
        .keys()
        .map(|i| match &toks[*i].tok {
            Tok::Kw(k) => *k,
            other => panic!("block opens at {:?}", other),
        })
        .collect();
    assert_eq!(openers, ["interface", "enum", "record", "func", "if", "while", "for", "unsafe", "func", "match"]);
    for end in layout.blocks.values() {
        assert_eq!(toks[*end].tok, Tok::Kw("end"));
    }
    assert_eq!(layout.elses.len(), 2);
    for e in &layout.elses {
        assert_eq!(toks[*e].tok, Tok::Kw("else"));
    }
    assert_eq!(layout.arms.len(), 1, "one match arm has its body on the following lines");
}

#[test]
fn asm_blocks_are_recorded() {
    let src = "package t\nfunc f()\n    asm\n        cli\n        hlt\n    end\nend\n";
    let (_, layout, toks, _) = parse_with_layout(src).unwrap();
    assert!(layout.blocks.keys().any(|i| toks[*i].tok == Tok::Kw("asm")));
}

// ---- the style ----

#[test]
fn nested_blocks_are_indented_and_end_and_else_line_up() {
    let src = "package t\nfunc f(n: int)\nwhile n > 0\nif n % 2 == 0\nprint(\"even\")\nelse\nprint(\"odd\")\nend\nn -= 1\nend\nend\n";
    let want = "package t\nfunc f(n: int)\n    while n > 0\n        if n % 2 == 0\n            print(\"even\")\n        else\n            print(\"odd\")\n        end\n        n -= 1\n    end\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn function_types_and_lambdas_do_not_change_the_indentation() {
    let src = "package t\ntype H = record\ncb: func(int) -> bool\nn: int\nend\nfunc main()\nlet xs = sort(xs, func(a, b) a < b end)\nprint(1)\nend\n";
    let want = "package t\ntype H = record\n    cb: func(int) -> bool\n    n: int\nend\nfunc main()\n    let xs = sort(xs, func(a, b) a < b end)\n    print(1)\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn match_arms_indent_their_bodies() {
    let src = "package t\nfunc f(n: int)\nmatch n\n1 -> print(1)\n2 ->\nprint(2)\nprint(3)\n_ -> print(4)\nend\nend\n";
    let want = "package t\nfunc f(n: int)\n    match n\n        1 -> print(1)\n        2 ->\n            print(2)\n            print(3)\n        _ -> print(4)\n    end\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn continuation_lines_are_one_level_deeper() {
    let src = "package t\nfunc main()\nlet r = call(1,\n2,\n3\n)\nlet s = 1 +\n2\nfoo.bar()\n.baz()\nend\n";
    let want = "package t\nfunc main()\n    let r = call(1,\n        2,\n        3\n    )\n    let s = 1 +\n        2\n    foo.bar()\n        .baz()\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn a_block_that_ends_a_call_is_not_pushed_in_twice() {
    let src = "package t\nfunc main()\nlet ys = map(xs, func(x)\nx * 2\nend)\nprint(1)\nend\n";
    let want = "package t\nfunc main()\n    let ys = map(xs, func(x)\n        x * 2\n    end)\n    print(1)\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn generic_types_at_the_end_of_a_line_do_not_continue_it() {
    let src = "package t\ntype U<T> = record\na: Map<T, bool>\nend\ntype Table = Map<text, List<i32>>\nfunc f()\nreturn\nend\n";
    let want = "package t\ntype U<T> = record\n    a: Map<T, bool>\nend\ntype Table = Map<text, List<i32>>\nfunc f()\n    return\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn commas_and_brackets_are_spaced() {
    let src = "package t\nfunc main()\n    foo( a ,b )\n    let xs = [ 1,2 , 3 ]\nend\n";
    let want = "package t\nfunc main()\n    foo(a, b)\n    let xs = [1, 2, 3]\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn spacing_used_for_alignment_is_kept() {
    let src = "package t\nfunc f(s: Shape)\n    match s\n        Circle(r)  -> print(1)\n        Empty      -> print(2)\n    end\nend\n";
    assert_eq!(fmt(src), src);
}

#[test]
fn blank_lines_are_limited_and_removed_at_block_edges() {
    let src = "\n\npackage t\n\n\n\nfunc a()\n\nprint(1)\n\n\nprint(2)\n\nend\n\n\n\nfunc main()\nend\n\n\n";
    let want = "package t\n\nfunc a()\n    print(1)\n\n    print(2)\nend\n\nfunc main()\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn trailing_spaces_and_tabs_are_removed() {
    let src = "package t   \nfunc main()  \n\tprint(1)  \nend";
    let want = "package t\nfunc main()\n    print(1)\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn comments_are_kept_and_aligned_with_the_code() {
    let src = "package t\n// top\nfunc main()\n// own line\nlet x = 1 // trailing\nlet y = 2          // aligned\n        // before end\nend\n";
    let want = "package t\n// top\nfunc main()\n    // own line\n    let x = 1  // trailing\n    let y = 2          // aligned\n    // before end\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn block_comments_keep_their_inside() {
    let src = "package t\nfunc main()\n/* one\n   two\n*/\nprint(1)\nend\n";
    let want = "package t\nfunc main()\n    /* one\n   two\n*/\n    print(1)\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn literals_are_copied_exactly() {
    let src = "package t\nfunc main()\n    let a = 0xFF\n    let b = \"a\\tb {x}\"\n    let c = 1_000\n    let d = 'x'\nend\n";
    assert_eq!(fmt(src), src);
}

#[test]
fn interpolations_survive_reindenting() {
    let src = "package t\nfunc main()\nprint(\"a {x} b {y + 1}\")\nend\n";
    let want = "package t\nfunc main()\n    print(\"a {x} b {y + 1}\")\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn asm_lines_sit_inside_the_block_and_keep_their_relative_indentation() {
    let src = "package t\nfunc f()\nasm\nstart:\n    cli\n    in al, dx\n    // a comment\n    mov ax, 1 // note\nend\nend\n";
    let want = "package t\nfunc f()\n    asm\n        start:\n            cli\n            in al, dx\n            // a comment\n            mov ax, 1 // note\n    end\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn asm_operands_and_clobbers_are_indented_with_the_block() {
    let src = "package t\nfunc f(p: int)\nunsafe\nasm\nout dx, al\nin rdx = p\nclobber cc\nend\nend\nend\n";
    let want = "package t\nfunc f(p: int)\n    unsafe\n        asm\n            out dx, al\n            in rdx = p\n            clobber cc\n        end\n    end\nend\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn the_files_line_ending_is_kept() {
    let src = "package t\r\nfunc main()\r\nprint(1)\r\nend\r\n";
    assert_eq!(fmt(src), "package t\r\nfunc main()\r\n    print(1)\r\nend\r\n");
}

#[test]
fn a_file_with_a_syntax_error_is_reported_and_not_formatted() {
    let e = format_source("package t\nfunc f(\n").unwrap_err();
    assert!(e.starts_with("2:") || e.starts_with("3:"), "{}", e);
}

#[test]
fn formatting_twice_changes_nothing() {
    let messy = "package t\nfunc main()\nlet r = call(1,\n2)\nif r\nprint( r )\nelse\n// no\nprint(0)\nend\nend\n";
    let once = fmt(messy);
    assert_eq!(fmt(&once), once);
}

// ---- the repository's own files ----

fn repo_files() -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs: Vec<String> = ["std", "examples", "os"].iter().map(|s| s.to_string()).collect();
    for entry in std::fs::read_dir("packages").unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            dirs.push(p.to_str().unwrap().to_string());
            if p.join("linux").is_dir() {
                dirs.push(p.join("linux").to_str().unwrap().to_string());
            }
        }
    }
    for dir in dirs {
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().is_some_and(|e| e == "zila") {
                out.push(p.to_str().unwrap().to_string());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn every_repository_file_formats_idempotently_with_the_same_tokens() {
    let files = repo_files();
    assert!(files.len() > 30, "only {} files", files.len());
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        let once = format_source(&src).unwrap_or_else(|e| panic!("{}: {}", f, e));
        let twice = format_source(&once).unwrap_or_else(|e| panic!("{} (second pass): {}", f, e));
        assert_eq!(once, twice, "{} is not idempotent", f);
    }
}

#[test]
fn scrambled_indentation_formats_back_to_the_same_text() {
    let mut tested = 0;
    for f in repo_files() {
        let src = std::fs::read_to_string(&f).unwrap();
        // asm lines and block forms (raw text) depend on their own indentation
        if src.contains("asm\n") || src.contains("asm\r\n") || src.contains("/*") || !zila::comptime::block_names(&src).is_empty() {
            continue;
        }
        // so do the lines of a `sql` block
        if zila::lexer::Lexer::new(&src).tokenize().is_ok_and(|t| t.iter().any(|t| matches!(t.tok, zila::lexer::Tok::Sql { .. }))) {
            continue;
        }
        let formatted = format_source(&src).unwrap();
        let scrambled: String = formatted
            .lines()
            .enumerate()
            .map(|(i, l)| format!("{}{}\n", " ".repeat((i * 5) % 9), l.trim_start()))
            .collect();
        // only files whose text has no line that a changed indentation could alter
        let toks = |s: &str| zila::lexer::Lexer::new(s).tokenize().unwrap().into_iter().map(|t| t.tok).collect::<Vec<_>>();
        let (a, b) = (toks(&scrambled), toks(&formatted));
        assert!(a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| zila::fmt::tok_eq(x, y)), "scrambling changed the tokens of {}", f);
        assert_eq!(format_source(&scrambled).unwrap().replace("\r\n", "\n"), formatted.replace("\r\n", "\n"), "{}", f);
        tested += 1;
    }
    assert!(tested > 25, "only {} files tested", tested);
}

#[test]
fn the_repository_is_formatted() {
    let unformatted: Vec<String> =
        repo_files().into_iter().filter(|f| format_source(&std::fs::read_to_string(f).unwrap()).unwrap() != std::fs::read_to_string(f).unwrap()).collect();
    assert!(unformatted.is_empty(), "not formatted: {:?} (run `zila --fmt std/*.zila examples/*.zila os/*.zila`)", unformatted);
}

// ---- the command line ----

fn temp_file(name: &str, text: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("zila-fmt-test-{}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), name));
    std::fs::write(&p, text).unwrap();
    p
}

fn run(args: &[&std::path::Path], flag: &str) -> (String, String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_zila")).arg(flag).args(args).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).replace('\r', ""), String::from_utf8_lossy(&out.stderr).replace('\r', ""), out.status.code().unwrap())
}

#[test]
fn fmt_rewrites_files_and_check_lists_them() {
    let bad = temp_file("bad.zila", "package t\nfunc main()\nprint(1)\nend\n");
    let good = temp_file("good.zila", "package t\nfunc main()\n    print(1)\nend\n");
    let (out, _, code) = run(&[&bad, &good], "--fmt-check");
    assert_eq!(code, 1);
    assert!(out.contains("not formatted") && out.contains("bad.zila") && !out.contains("good.zila"), "{}", out);
    assert_eq!(std::fs::read_to_string(&bad).unwrap(), "package t\nfunc main()\nprint(1)\nend\n", "--fmt-check must not write");
    let (out, _, code) = run(&[&bad, &good], "--fmt");
    assert_eq!(code, 0);
    assert!(out.contains("formatted") && out.contains("bad.zila") && !out.contains("good.zila"), "{}", out);
    assert_eq!(std::fs::read_to_string(&bad).unwrap(), "package t\nfunc main()\n    print(1)\nend\n");
    let (out, _, code) = run(&[&bad, &good], "--fmt-check");
    assert_eq!((out.as_str(), code), ("", 0));
    let (out, _, code) = run(&[&bad], "--fmt");
    assert_eq!((out.as_str(), code), ("", 0), "formatting again changes nothing");
    let _ = (std::fs::remove_file(&bad), std::fs::remove_file(&good));
}

#[test]
fn fmt_leaves_a_broken_file_alone() {
    let broken = temp_file("broken.zila", "package t\nfunc f(\n");
    let (_, err, code) = run(&[&broken], "--fmt");
    assert_eq!(code, 1);
    assert!(err.contains("broken.zila"), "{}", err);
    assert_eq!(std::fs::read_to_string(&broken).unwrap(), "package t\nfunc f(\n");
    let _ = std::fs::remove_file(&broken);
}

// ---- block forms ----

const BLOCK_PKG: &str = "package fmtblocks_pkg\n#[block]\ncomptime func raw_text(raw: text) -> text\n    return raw\nend\n";

#[test]
fn block_content_stays_byte_for_byte_and_only_the_name_and_end_are_aligned() {
    zila::comptime::register_package("fmtblocks_pkg", BLOCK_PKG);
    let src = "package t\nuse fmtblocks_pkg\n\nfunc f()\nlet s = raw_text   // note\n   <p>  {not zila ((  \n\t odd   indentation\n      end\n  print(s)\nend\n";
    let out = fmt(src);
    assert_eq!(
        out,
        "package t\nuse fmtblocks_pkg\n\nfunc f()\n    let s = raw_text   // note\n   <p>  {not zila ((  \n\t odd   indentation\n    end\n    print(s)\nend\n"
    );
    // idempotent
    assert_eq!(fmt(&out), out);
    // a block standing alone on its line
    let alone = "package t\nuse fmtblocks_pkg\nfunc f()\nif true\nraw_text\n x  y\n   end\nend\nend\n";
    assert_eq!(fmt(alone), "package t\nuse fmtblocks_pkg\nfunc f()\n    if true\n        raw_text\n x  y\n        end\n    end\nend\n");
}

#[test]
fn a_name_that_is_not_a_block_formats_as_ordinary_code() {
    let src = "package t\nfunc f()\nlet s = nothing_here\n  print(s)\nend\n";
    assert_eq!(fmt(src), "package t\nfunc f()\n    let s = nothing_here\n    print(s)\nend\n");
}

#[test]
fn pub_declarations_format_with_one_space_and_are_idempotent() {
    let src = "package t\n\npub func f()\nend\n\npub type X = record\n    pub a: int\n    b: int\nend\n\n#[export(\"f\")]\npub func g() -> int\n    return 1\nend\n\npub async func h()\nend\n\npub let k = 1\n\npub func (x: X) m()\nend\n";
    let once = fmt(src);
    assert!(once.contains("pub func f()"), "{}", once);
    assert!(once.contains("pub type X = record"), "{}", once);
    assert!(once.contains("    pub a: int\n    b: int"), "{}", once);
    assert!(once.contains("#[export(\"f\")]\npub func g()"), "{}", once);
    assert!(once.contains("pub async func h()"), "{}", once);
    assert!(once.contains("pub func (x: X) m()"), "{}", once);
    assert_eq!(fmt(&once), once);
}

#[test]
fn a_sql_block_moves_as_a_unit_and_keeps_its_relative_indentation() {
    let src = "package t\nuse db\nuse postgres\nfunc f() -> db.Sql\n        return sql postgres\n    select id\n        from t\n    where a = {x}\nend\nend\n";
    let want = "package t\nuse db\nuse postgres\nfunc f() -> db.Sql\n    return sql postgres\n        select id\n            from t\n        where a = {x}\n    end\nend\n";
    assert_eq!(fmt(src), want);
    // a second pass changes nothing
    assert_eq!(fmt(want), want);
}

#[test]
fn text_in_a_sql_block_that_looks_like_zila_stays_in_the_block() {
    let src = "package t\nuse db\nuse postgres\nfunc f() -> db.Sql\n    let q = sql postgres\n        select 1 where end_date is null -- end\n    end\n    return q\nend\n";
    assert_eq!(fmt(src), src);
}

#[test]
fn an_identifier_called_sql_is_formatted_as_code() {
    let src = "package t\nfunc f(sql: text) -> text\nreturn sql\nend\n";
    let want = "package t\nfunc f(sql: text) -> text\n    return sql\nend\n";
    assert_eq!(fmt(src), want);
}
