//! `zila-parse --test`: finds the `#[test]` functions of a file, builds the file with a
//! generated entry point that runs them in order, runs it and reports.

use crate::ast::*;
use crate::codegen_x64::{compile_to_native_with, CompileOptions};
use crate::parser::parse_source;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static BUILD: AtomicUsize = AtomicUsize::new(0);

/// A test function and whether it returns `error` (otherwise it returns nothing).
struct Test {
    name: String,
    returns_error: bool,
}

/// How one file went.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Passed,
    Failed,
    NoTests,
}

/// The tests of `file`, in order, or a message about the first function that is not a valid test.
fn find_tests(file: &File) -> Result<Vec<Test>, String> {
    let mut tests = Vec::new();
    for d in &file.decls {
        let DeclKind::Func(fd) = &d.kind else { continue };
        if !d.attrs.iter().any(|a| a.name == "test") {
            continue;
        }
        let bad = |why: &str| Err(format!("line {}: test '{}' {}", d.line, fd.name, why));
        if !fd.params.is_empty() {
            return bad("cannot take parameters");
        }
        if fd.is_async || !fd.generics.is_empty() || fd.receiver.is_some() {
            return bad("cannot be async, generic or a method");
        }
        let returns_error = match &fd.ret {
            None => false,
            Some(RetType { ty: None, fallible: true }) => true,
            Some(_) => return bad("must return nothing or `error`"),
        };
        tests.push(Test { name: fd.name.clone(), returns_error });
    }
    Ok(tests)
}

/// The text of the entry point that runs `tests` and prints the results.
fn entry_point(tests: &[Test]) -> String {
    let mut s = String::from("package main\n\nfunc main() -> error\n    var passed = 0\n    var failed = 0\n");
    for t in tests {
        if t.returns_error {
            s.push_str(&format!(
                "    match {n}()\n        ok ->\n            print(\"ok {n}\")\n            passed += 1\n        err(e) ->\n            print(\"FAIL {n}: {{e.message}}\")\n            failed += 1\n    end\n",
                n = t.name
            ));
        } else {
            s.push_str(&format!("    {n}()\n    print(\"ok {n}\")\n    passed += 1\n", n = t.name));
        }
    }
    s.push_str("    print(\"test result: {passed} passed, {failed} failed\")\n    if failed > 0\n        return error(\"{failed} test(s) failed\")\n    end\n    return\nend\n");
    s
}

/// Builds and runs the tests of one file.
fn run_file(path: &str, optimize: bool) -> Result<Outcome, String> {
    let mut src = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path, e))?;
    crate::package::install_for(std::path::Path::new(path))?;
    // a `_test.br` file of a package is tested together with the package's own files, as one
    // module (so its tests see the package's private declarations); the module is built as main
    if path.ends_with("_test.br") {
        let pkg = src.lines().find_map(|l| l.trim().strip_prefix("package ").map(|p| p.trim().to_string()));
        if let Some(pkg) = pkg.filter(|p| p != "main") {
            let p = std::path::Path::new(path);
            let dir = match p.parent() {
                Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
                _ => std::path::PathBuf::from("."),
            };
            let merged = crate::package::merged_dir_source(&dir, &pkg, &[p.to_path_buf()])?;
            src = merged.replacen(&format!("package {}
", pkg), "package main
", 1);
        }
    }
    crate::comptime::register_siblings(std::path::Path::new(path), &src);
    let mut file = parse_source(&src).map_err(|d| format!("{}:{}:{}: {}", path, d.line, d.col, d.msg))?;
    let tests = find_tests(&file).map_err(|e| format!("{}: {}", path, e))?;
    if tests.is_empty() {
        println!("no tests in {}", path);
        return Ok(Outcome::NoTests);
    }
    if file.package != "main" {
        return Err(format!("{}: files with tests must be in `package main`", path));
    }
    // the entry point replaces the file's own `main`
    file.decls.retain(|d| !matches!(&d.kind, DeclKind::Func(f) if f.name == "main" && f.receiver.is_none()));
    let generated = parse_source(&entry_point(&tests)).map_err(|d| format!("internal error in the generated entry point: {}", d.msg))?;
    file.decls.extend(generated.decls);

    println!("running {} test(s) in {}", tests.len(), path);
    let exe = std::env::temp_dir().join(format!("zila-test-{}-{}.exe", std::process::id(), BUILD.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native_with(&file, exe.to_str().unwrap(), &CompileOptions { optimize, test_mode: true }) {
        Ok(()) => {}
        Err(e) => return Err(format!("{}: {}", path, e)),
    }
    let out = Command::new(&exe).output().map_err(|e| format!("cannot run the tests of {}: {}", path, e));
    let _ = std::fs::remove_file(&exe);
    let out = out?;
    let stdout = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    print!("{}", stdout);
    eprint!("{}", String::from_utf8_lossy(&out.stderr).replace('\r', ""));
    let summary = stdout.lines().rev().find(|l| l.starts_with("test result:"));
    match summary {
        Some(line) if line.ends_with(" 0 failed") => Ok(Outcome::Passed),
        Some(_) => Ok(Outcome::Failed),
        None => {
            let code = out.status.code().unwrap_or(-1);
            // Windows exception codes (a crash) read better in hexadecimal
            let shown = if code < 0 || code > 255 { format!("0x{:X}", code as u32) } else { code.to_string() };
            let how = if code == 101 { "by an abort, " } else { "" };
            println!("run stopped early ({}exit code {})", how, shown);
            Ok(Outcome::Failed)
        }
    }
}

/// Runs the tests of every file; the exit status of the command.
pub fn run(files: &[String], optimize: bool) -> i32 {
    let mut failed = false;
    for f in files {
        match run_file(f, optimize) {
            Ok(Outcome::Passed) | Ok(Outcome::NoTests) => {}
            Ok(Outcome::Failed) => failed = true,
            Err(e) => {
                eprintln!("error: {}", e);
                failed = true;
            }
        }
    }
    if failed {
        1
    } else {
        0
    }
}
