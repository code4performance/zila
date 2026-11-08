//! The Windows executable writer: the assembler accepts all generated Windows text, imports come
//! from `#[link]`, and the PE image is complete without any external tool.

use zila::codegen_x64::{generate_asm_for, CompileOptions, Target};
use zila::parse_source;

/// Programs that together use the whole standard library for Windows.
fn programs() -> Vec<(String, String)> {
    let mut v = Vec::new();
    let ex = std::fs::read_dir("examples").unwrap();
    let mut names: Vec<String> = ex.filter_map(|e| e.ok()).map(|e| format!("examples/{}", e.file_name().to_string_lossy())).filter(|p| p.ends_with(".zila")).collect();
    names.sort();
    for p in names {
        v.push((p.clone(), std::fs::read_to_string(&p).unwrap()));
    }
    for (name, uses, body) in [
        ("ui", "ui", "    ui.run()\n"),
        ("net", "net\nuse task", "    let l = try await net.listen(0)\n    await l.close()\n    return\n"),
        ("tls", "tls\nuse task", "    let c = try await tls.connect(\"example.com\", 443)\n    await c.close()\n    return\n"),
        ("task", "task", "    await sleep(1)\n    return\n"),
        ("float", "float", "    let x = 1.5 * 2.0\n    print(\"{x}\")\n"),
        ("format", "format", "    print(\"{1.5}\")\n"),
        ("sorted", "sorted", "    var s = sorted.map()\n"),
        ("fs", "fs\nuse task", "    let f = try await fs.read(\"x\")\n    return\n"),
    ] {
        let async_main = body.contains("await") || body.contains("try");
        let src = if async_main {
            format!("package main\nuse {}\nfunc main() -> error\n{}end\n", uses, body)
        } else {
            format!("package main\nuse {}\nfunc main()\n{}end\n", uses, body)
        };
        v.push((name.to_string(), src));
    }
    v
}

#[test]
fn the_assembler_accepts_all_windows_text() {
    let mut problems = Vec::new();
    let mut built = 0;
    for (name, src) in programs() {
        let Ok(file) = parse_source(&src) else { continue };
        let Ok(asm) = generate_asm_for(&file, &CompileOptions::default(), Target::Windows) else { continue };
        built += 1;
        if let Err(e) = zila::asm_x86::assemble(&asm, 0x140001000, 64) {
            problems.push(format!("{}: {}", name, e));
        }
    }
    assert!(built >= 10, "only {} programs generated text", built);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---- helpers: build an executable and read its headers ----

use zila::codegen_x64::compile_to_native;
use std::path::PathBuf;

fn u16_at(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}
fn u32_at(b: &[u8], o: usize) -> usize {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize
}

/// The file offset of a virtual address, by the section table.
fn rva_to_offset(b: &[u8], rva: usize) -> usize {
    let pe = u32_at(b, 0x3C);
    let sections = u16_at(b, pe + 6);
    let opt = u16_at(b, pe + 20);
    let table = pe + 24 + opt;
    for i in 0..sections {
        let s = table + 40 * i;
        let (vsize, va, raw, ptr) = (u32_at(b, s + 8), u32_at(b, s + 12), u32_at(b, s + 16), u32_at(b, s + 20));
        if rva >= va && rva < va + vsize.max(raw) {
            return rva - va + ptr;
        }
    }
    panic!("rva {:x} is in no section", rva);
}

fn cstr(b: &[u8], o: usize) -> String {
    let end = b[o..].iter().position(|&c| c == 0).unwrap();
    String::from_utf8_lossy(&b[o..o + end]).to_string()
}

/// `(library, functions)` of the import directory.
fn read_imports(b: &[u8]) -> Vec<(String, Vec<String>)> {
    let pe = u32_at(b, 0x3C);
    let dir = pe + 24 + 112 + 8; // data directory 1
    let mut at = rva_to_offset(b, u32_at(b, dir));
    let mut out = Vec::new();
    while u32_at(b, at + 12) != 0 {
        let lib = cstr(b, rva_to_offset(b, u32_at(b, at + 12)));
        let mut t = rva_to_offset(b, u32_at(b, at));
        let mut names = Vec::new();
        loop {
            let v = u32_at(b, t);
            if v == 0 {
                break;
            }
            names.push(cstr(b, rva_to_offset(b, v) + 2));
            t += 8;
        }
        out.push((lib, names));
        at += 20;
    }
    out
}

fn subsystem(b: &[u8]) -> usize {
    u16_at(b, u32_at(b, 0x3C) + 24 + 68)
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zila-pe-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn build(src: &str, name: &str) -> Result<Vec<u8>, String> {
    let file = parse_source(src).map_err(|d| d.msg)?;
    let exe = scratch(name);
    compile_to_native(&file, exe.to_str().unwrap())?;
    Ok(std::fs::read(&exe).unwrap())
}

fn dlls(imports: &[(String, Vec<String>)]) -> Vec<&str> {
    imports.iter().map(|(d, _)| d.trim_end_matches(".dll")).collect()
}

const WINDOW_AND_SOCKETS: &str = "package main\nuse ui\nuse net\nuse task\nfunc main() -> error\n    let l = try await net.listen(0)\n    await l.close()\n    var win = ui.Window(\"t\", 64, 48)\n    win.layout(ui.vstack(ui.TextBox()))\n    win.show()\n    ui.run()\n    return\nend\n";

#[test]
fn imports_come_from_link_attributes() {
    let ui = build(WINDOW_AND_SOCKETS, "imp_ui.exe").unwrap();
    let imports = read_imports(&ui);
    let libs = dlls(&imports);
    for want in ["kernel32", "user32", "gdi32", "ws2_32"] {
        assert!(libs.contains(&want), "{:?}", libs);
    }
    let find = |lib: &str| imports.iter().find(|(d, _)| d.trim_end_matches(".dll") == lib).unwrap().1.clone();
    assert!(find("user32").contains(&"CreateWindowExA".to_string()));
    assert!(find("ws2_32").contains(&"WSAStartup".to_string()));
    assert!(find("kernel32").contains(&"ExitProcess".to_string()));
    assert_eq!(libs[0], "kernel32");
    // the import table does not depend on the run
    let again = build(WINDOW_AND_SOCKETS, "imp_ui2.exe").unwrap();
    assert!(imports == read_imports(&again));
}

#[test]
fn a_console_program_imports_only_what_it_uses() {
    let b = build("package main\nfunc main()\n    print(\"hi\")\nend\n", "imp_console.exe").unwrap();
    assert_eq!(dlls(&read_imports(&b)), ["kernel32"]);
    assert_eq!(subsystem(&b), 3, "console subsystem");
    let t = build("package main\nuse task\nfunc main() -> error\n    await sleep(1)\n    return\nend\n", "imp_task.exe").unwrap();
    let libs: Vec<String> = read_imports(&t).into_iter().map(|(d, _)| d.trim_end_matches(".dll").to_string()).collect();
    assert!(libs.iter().all(|l| l == "kernel32" || l == "ntdll"), "{:?}", libs);
}

#[test]
fn a_window_program_uses_the_windows_subsystem() {
    let b = build("package main\nuse ui\nfunc main()\n    ui.run()\nend\n", "gui.exe").unwrap();
    assert_eq!(subsystem(&b), 2, "windows subsystem");
    // the manifest is embedded
    let text = String::from_utf8_lossy(&b);
    assert!(text.contains("Microsoft.Windows.Common-Controls") && text.contains("PerMonitorV2") && text.contains("UTF-8"));
}

#[test]
fn an_extern_without_link_is_a_build_error_naming_it() {
    let src = "package main\nextern \"system\" func NoLibraryHere(x: u32) -> u32\nfunc main()\n    unsafe\n        NoLibraryHere(1)\n    end\nend\n";
    let e = build(src, "nolink.exe").unwrap_err();
    assert!(e.contains("NoLibraryHere") && e.contains("#[link"), "{}", e);
}

#[test]
fn an_unknown_function_in_a_known_library_still_builds() {
    let src = "package main\n#[link(\"kernel32\")]\nextern \"system\" func NoSuchFunction()\nfunc main()\n    unsafe\n        NoSuchFunction()\n    end\nend\n";
    let b = build(src, "unknown_fn.exe").unwrap();
    let imports = read_imports(&b);
    assert!(imports[0].1.contains(&"NoSuchFunction".to_string()));
}

#[test]
fn c_sources_are_an_error_on_windows() {
    let src = "package main\n#[link_src(\"helper.c\")]\nextern \"C\" func helper() -> u32\nfunc main()\nend\n";
    let e = build(src, "csrc.exe").unwrap_err();
    assert!(e.contains("link_src") && e.contains("C source"), "{}", e);
}

#[test]
fn a_bad_asm_block_names_the_file_and_line_and_leaves_nothing() {
    let temp_dirs = || {
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("zila-build-"))
            .count()
    };
    let before = temp_dirs();
    let src = "package main\nfunc main()\n    let x = 1\n    unsafe\n        asm\n            frobnicate rax\n        end\n    end\nend\n";
    let _ = std::fs::remove_file(scratch("badasm.exe"));
    let e = build(src, "badasm.exe").unwrap_err();
    assert!(e.starts_with("main.zila:5:"), "{}", e);
    assert!(e.contains("frobnicate"), "{}", e);
    assert!(!scratch("badasm.exe").exists(), "no output file is left");
    assert_eq!(temp_dirs(), before, "no temporary directory is left");
}

#[test]
fn executables_are_small() {
    for (p, limit) in [("examples/hello.zila", 8 * 1024), ("examples/ui.zila", 68 * 1024)] {
        let src = std::fs::read_to_string(p).unwrap();
        let name = format!("size_{}.exe", p.replace('/', "_"));
        let b = build(&src, &name).unwrap_or_else(|e| panic!("{}: {}", p, e));
        assert!(b.len() <= limit, "{} is {} bytes (limit {})", p, b.len(), limit);
    }
}

#[test]
fn the_compiler_needs_no_visual_studio() {
    // a build with an empty environment: no PATH, no vcvars, no Visual Studio variables
    let exe = scratch("clean_env.exe");
    let _ = std::fs::remove_file(&exe);
    let src = scratch("clean_env.zila");
    std::fs::write(&src, "package main\nfunc main()\n    print(\"hello\")\nend\n").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_zila"))
        .env_clear()
        .env("PATH", "")
        .arg(&src)
        .arg("--build")
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let run = std::process::Command::new(&exe).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hello
");
    assert!(run.status.success());
}
