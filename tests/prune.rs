//! Reachability pruning (spec `static-linking`): a `use` that nothing reaches adds nothing to the
//! program, what the program calls is kept, and `--no-prune` brings everything back.

use zila::codegen_x64::{generate_asm, generate_asm_for, Target};
use zila::parse_source;
use std::process::Command;

const HELLO: &str = "package main\n\nfunc main()\n    print(\"hi\")\nend\n";

fn asm(src: &str) -> String {
    generate_asm(&parse_source(src).unwrap()).unwrap()
}

/// The program without its numbers and in a fixed order: label and string numbers count every
/// declaration the compiler met, pruned or not, and strings are laid out in that order, so they
/// differ between two programs that are the same.
fn shape(asm: &str) -> String {
    let mut lines: Vec<String> = asm.lines().map(|l| l.chars().filter(|c| !c.is_ascii_digit()).collect()).collect();
    lines.sort();
    lines.join("
")
}

fn with_use(lib: &str) -> String {
    format!("package main\n\nuse {}\n\nfunc main()\n    print(\"hi\")\nend\n", lib)
}

/// Every library a program can name on Windows, except `ws` (internal to `net`).
const LIBRARIES: [&str; 28] = [
    "ai", "bufio", "crypto", "db", "derive", "errors", "float", "format", "fs", "future", "json", "list", "math", "mcp", "net", "postgres", "rag", "reflect", "set", "sorted", "sqlserver", "sync", "sys", "task", "text", "thread", "tls", "ui",
];

#[test]
fn an_unused_import_adds_nothing_to_the_program() {
    let base = shape(&asm(HELLO));
    for lib in LIBRARIES {
        assert!(shape(&asm(&with_use(lib))) == base, "`use {}` changed the program although nothing of it is called", lib);
    }
    assert!(shape(&asm(&with_use("web"))) == base, "`use web` changed the program");
}

#[test]
fn an_unused_import_adds_nothing_on_linux() {
    let f = |src: &str| shape(&generate_asm_for(&parse_source(src).unwrap(), &Default::default(), Target::Linux).unwrap());
    let base = f(HELLO);
    for lib in ["fs", "net", "sys", "task", "text", "tls", "ui", "json"] {
        assert!(f(&with_use(lib)) == base, "`use {}` changed the Linux program", lib);
    }
}

#[test]
fn a_called_function_and_what_it_calls_are_kept_and_the_rest_is_not() {
    let a = asm("package main\n\nuse text\n\nfunc main()\n    print(text.upper(\"a\"))\nend\n");
    assert!(a.contains("B_text_upper proc"));
    assert!(a.contains("B_text_shift_ascii proc"), "what `upper` calls must be kept");
    assert!(!a.contains("B_text_lower proc"), "an unused function must not be emitted");
    assert!(!a.contains("B_text_split proc"));
}

#[test]
fn an_unused_window_library_does_not_import_the_window_system() {
    let a = asm(&with_use("ui"));
    assert!(!a.contains("GOT_CreateWindowExA"));
    let b = asm("package main\n\nuse ui\n\nfunc main()\n    var win = ui.Window(\"t\", 64, 48)\n    win.layout(ui.vstack(ui.TextBox()))\n    win.show()\n    ui.run()\nend\n");
    assert!(b.contains("GOT_CreateWindowExA dq 0"));
}

#[test]
fn a_global_initialiser_with_an_effect_still_runs() {
    let a = asm("package main\n\nfunc note() -> int\n    print(\"start\")\n    return 1\nend\n\nvar unused = note()\n\nfunc main()\n    print(\"main\")\nend\n");
    assert!(a.contains("B_main_note proc"), "an initialiser that prints must be kept");
}

#[test]
fn a_global_initialiser_nothing_reads_and_that_only_computes_is_dropped() {
    let a = asm("package main\n\nfunc make() -> int\n    return 41 + 1\nend\n\nvar unused = make()\n\nfunc main()\n    print(\"main\")\nend\n");
    assert!(!a.contains("G_main_unused"));
}

#[test]
fn no_prune_keeps_everything() {
    let dir = std::env::temp_dir().join(format!("zila-prune-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("a.zila");
    std::fs::write(&src, with_use("json")).unwrap();
    let run = |extra: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_zila")).arg(&src).arg("--asm").args(extra).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let pruned = run(&[]);
    let all = run(&["--no-prune"]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!pruned.contains("B_json_parse_value proc"));
    assert!(all.contains("B_json_parse_value proc"));
}

#[test]
fn explain_link_lists_each_package_and_adds_up() {
    let dir = std::env::temp_dir().join(format!("zila-link-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("a.zila");
    std::fs::write(&src, "package main\n\nuse text\n\nfunc main()\n    print(text.upper(\"a\"))\nend\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_zila")).arg(&src).arg("--explain-link").current_dir(&dir).output().unwrap();
    let report = String::from_utf8_lossy(&out.stdout).to_string();
    let wrote = dir.join("a.exe").exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!wrote, "no executable is written without -o");
    let row = |name: &str| report.lines().find(|l| l.starts_with(name)).unwrap_or_else(|| panic!("no line for {}: {}", name, report)).to_string();
    assert!(row("main").contains("program") && row("text").contains("std") && row("(runtime)").contains('-'));
    // each package shows what its reachable code can do
    assert!(report.lines().next().unwrap().contains("effects") && row("text").trim_end().ends_with("none"), "{}", report);
    // the lines add up to the total
    let nums = |l: &str| -> (u64, u64) {
        let v: Vec<u64> = l.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        (v[v.len() - 2], v[v.len() - 1])
    };
    let (mut c, mut d) = (0, 0);
    for l in report.lines().skip(1).take_while(|l| !l.starts_with("total")) {
        let (a, b) = nums(l);
        c += a;
        d += b;
    }
    assert_eq!(nums(&row("total")), (c, d));
    // a library nothing uses is not in the report
    assert!(!report.contains("json"));
}
