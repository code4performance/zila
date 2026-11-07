//! `#[allocator]`: a program replaces the runtime heap. Declarations are checked, the generated
//! runtime is inspected on every target, and programs with counting allocators are built and run
//! on Windows and (through WSL) on Linux; a machine without the toolchain skips the runs.

use brasa::codegen_x64::{compile_to_linux, compile_to_native_with, generate_asm, generate_asm_for, CompileOptions, Target};
use brasa::noalloc::check_program;
use brasa::parse_source;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    out: String,
    err: String,
    code: i32,
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("brasa-allocator-test-{}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), name))
}

fn windows(src: &str) -> Option<Ran> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("prog.exe");
    match compile_to_native_with(&file, exe.to_str().unwrap(), &CompileOptions::default()) {
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

fn wsl_path(p: &std::path::Path) -> String {
    let s = p.to_str().unwrap().replace('\\', "/");
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(d), Some(':')) => format!("/mnt/{}{}", d.to_ascii_lowercase(), &s[2..]),
        _ => s,
    }
}

fn wsl_works() -> bool {
    static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CHECK.get_or_init(|| match Command::new("wsl.exe").args(["-e", "sh", "-c", "echo ready"]).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("ready"),
        Err(_) => false,
    })
}

fn linux(src: &str) -> Option<Ran> {
    if !wsl_works() {
        eprintln!("WSL is not available: skipping");
        return None;
    }
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("prog");
    compile_to_linux(&file, exe.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("compile error: {}", e));
    let n = exe.file_name().unwrap().to_str().unwrap().to_string();
    let script = format!("cp '{}' /tmp/{n} && chmod +x /tmp/{n} && /tmp/{n} 2>/tmp/{n}.err; echo \"__EXIT:$?\"; cat /tmp/{n}.err; rm -f /tmp/{n} /tmp/{n}.err", wsl_path(&exe), n = n);
    let o = Command::new("wsl.exe").args(["-e", "sh", "-c", &script]).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let text = String::from_utf8_lossy(&o.stdout).replace('\r', "");
    let (body, rest) = text.rsplit_once("__EXIT:").unwrap_or_else(|| panic!("no exit marker in: {}", text));
    let (code, err) = rest.split_once('\n').unwrap_or((rest, ""));
    Some(Ran { out: body.to_string(), err: err.to_string(), code: code.trim().parse().unwrap() })
}

fn check_err(src: &str, want: &str) {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let err = generate_asm(&file).unwrap_err();
    assert!(err.contains(want), "expected error containing {:?}, got {:?}", want, err);
}

/// The allocator functions of a program that gets its memory from the operating system, one
/// request at a time, counting the calls in `allocs`, `frees` and `reallocs`.
const WINDOWS_ALLOCATOR: &str = r#"
#[link("kernel32")]
extern func VirtualAlloc(addr: usize, size: usize, kind: u32, prot: u32) -> *u8
#[link("kernel32")]
extern func VirtualFree(addr: *u8, size: usize, kind: u32) -> i32

var allocs = 0
var frees = 0
var reallocs = 0
var fail = false
var last_size: u64 = 0
var last_align: u64 = 0
var freed_size: u64 = 0
var freed_align: u64 = 0

#[allocator(alloc)]
func my_alloc(size: u64, align: u64) -> *u8
    if fail
        return nil
    end
    allocs += 1
    last_size = size
    last_align = align
    var p: *u8 = nil
    unsafe
        p = VirtualAlloc(0, size, 0x3000, 4)
        for i in 0..8
            p[i] = 255
        end
    end
    return p
end

#[allocator(free)]
func my_free(ptr: *u8, size: u64, align: u64)
    frees += 1
    freed_size = size
    freed_align = align
    unsafe
        VirtualFree(ptr, 0, 0x8000)
    end
end
"#;

/// The same on Linux: `mmap` and `munmap` as system calls.
const LINUX_ALLOCATOR: &str = r#"
var allocs = 0
var frees = 0
var reallocs = 0
var fail = false
var last_size: u64 = 0
var last_align: u64 = 0
var freed_size: u64 = 0
var freed_align: u64 = 0

func sys_mmap(size: u64) -> *u8
    var r: *u8 = nil
    unsafe
        asm
            xor edi, edi
            mov edx, 3
            mov r10d, 34
            mov r8, -1
            xor r9d, r9d
            mov eax, 9
            syscall
            in rsi = size
            out rax = r
        end
    end
    return r
end

func sys_munmap(ptr: *u8, size: u64)
    unsafe
        asm
            mov eax, 11
            syscall
            in rdi = ptr
            in rsi = size
        end
    end
end

#[allocator(alloc)]
func my_alloc(size: u64, align: u64) -> *u8
    if fail
        return nil
    end
    allocs += 1
    last_size = size
    last_align = align
    let p = sys_mmap(size)
    unsafe
        for i in 0..8
            p[i] = 255
        end
    end
    return p
end

#[allocator(free)]
func my_free(ptr: *u8, size: u64, align: u64)
    frees += 1
    freed_size = size
    freed_align = align
    sys_munmap(ptr, size)
end
"#;

/// `realloc` for either allocator above: allocate, copy, free (and count).
const REALLOC: &str = r#"
#[allocator(realloc)]
func my_realloc(ptr: *u8, old: u64, size: u64, align: u64) -> *u8
    reallocs += 1
    let q = my_alloc(size, align)
    unsafe
        var i: u64 = 0
        while i < old
            q[i] = ptr[i]
            i += 1
        end
    end
    my_free(ptr, old, align)
    return q
end
"#;

fn with_allocator(allocator: &str, body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n{}\nfunc main()\n{}end\n", allocator, indented)
}

/// Runs the body on every target available here with that target's allocator.
fn everywhere(body: &str) -> Vec<(&'static str, Ran)> {
    let mut all = Vec::new();
    if let Some(r) = windows(&with_allocator(WINDOWS_ALLOCATOR, body)) {
        all.push(("windows", r));
    }
    if let Some(r) = linux(&with_allocator(LINUX_ALLOCATOR, body)) {
        all.push(("linux", r));
    }
    all
}

// ---- declarations ----

#[test]
fn wrong_signature_shows_the_expected_one() {
    let src = "package main\n\n#[allocator(alloc)]\nfunc a(size: u64) -> *u8\n    return nil\nend\n\nfunc main()\nend\n";
    check_err(src, "(size: u64, align: u64) -> *u8");
}

#[test]
fn duplicate_role_names_both() {
    let src = "package main\n\n#[allocator(alloc)]\nfunc a(size: u64, align: u64) -> *u8\n    return nil\nend\n\n#[allocator(alloc)]\nfunc b(size: u64, align: u64) -> *u8\n    return nil\nend\n\nfunc main()\nend\n";
    let file = parse_source(src).unwrap();
    let e = generate_asm(&file).unwrap_err();
    assert!(e.contains("already declared at line 4 ('a')"), "{}", e);
}

#[test]
fn alloc_without_free_is_rejected() {
    let src = "package main\n\n#[allocator(alloc)]\nfunc a(size: u64, align: u64) -> *u8\n    return nil\nend\n\nfunc main()\nend\n";
    check_err(src, "missing 'free'");
}

#[test]
fn realloc_needs_both() {
    let src = "package main\n\n#[allocator(realloc)]\nfunc r(ptr: *u8, old: u64, size: u64, align: u64) -> *u8\n    return nil\nend\n\nfunc main()\nend\n";
    check_err(src, "requires both 'alloc' and 'free'");
}

#[test]
fn the_standard_library_cannot_declare_one() {
    let main = parse_source("package main\n\nfunc main()\nend\n").unwrap();
    let lib = parse_source("package text\n\n#[allocator(alloc)]\nfunc a(size: u64, align: u64) -> *u8\n    return nil\nend\n").unwrap();
    let modules = vec![("main".to_string(), main), ("text".to_string(), lib)];
    let e = check_program(&modules, &HashSet::new()).unwrap_err();
    assert!(e.contains("not in the standard library"), "{}", e);
}

#[test]
fn an_allocator_that_allocates_is_rejected() {
    let src = "package main\n\n#[allocator(alloc)]\nfunc a(size: u64, align: u64) -> *u8\n    let s = \"size {size}\"\n    return nil\nend\n\n#[allocator(free)]\nfunc f(ptr: *u8, size: u64, align: u64)\nend\n\nfunc main()\nend\n";
    check_err(src, "text interpolation");
}

#[test]
fn heap_bytes_needs_the_built_in_heap() {
    let src = format!("package main\n{}\nfunc main()\n    let n = heap_bytes()\nend\n", WINDOWS_ALLOCATOR);
    check_err(&src, "heap_bytes() needs the built-in heap");
}

// ---- the generated runtime ----

#[test]
fn windows_does_not_import_the_process_heap() {
    let file = parse_source(&with_allocator(WINDOWS_ALLOCATOR, "let xs = [1, 2, 3]\nprintln(xs[0])")).unwrap();
    let asm = generate_asm(&file).unwrap();
    for s in ["HeapAlloc", "HeapFree", "GetProcessHeap", "rt_heap"] {
        assert!(!asm.contains(s), "{} is still in the assembly", s);
    }
    assert!(asm.contains("call B_main_my_alloc") && asm.contains("call B_main_my_free"));
    // and without an allocator the process heap is still used
    let plain = generate_asm(&parse_source("package main\n\nfunc main()\n    println(1)\nend\n").unwrap()).unwrap();
    assert!(plain.contains("HeapAlloc"));
}

#[test]
fn linux_leaves_out_free_lists_and_mmap() {
    let file = parse_source(&with_allocator(LINUX_ALLOCATOR, "let xs = [1, 2, 3]\nprintln(xs[0])")).unwrap();
    let asm = generate_asm_for(&file, &CompileOptions::default(), Target::Linux).unwrap();
    for s in ["rt_freelists", "rt_brk", "rt_heap_end", "rt_alloc_big", "rt_free_small"] {
        assert!(!asm.contains(s), "{} is still in the assembly", s);
    }
    // the only mmap is the program's own one
    assert_eq!(asm.matches("mov eax, 9\n").count(), 1);
    let plain = generate_asm_for(&parse_source("package main\n\nfunc main()\n    println(1)\nend\n").unwrap(), &CompileOptions::default(), Target::Linux).unwrap();
    assert!(plain.contains("rt_alloc_big"));
}

#[test]
fn bare_metal_keeps_the_built_in_heap_without_an_allocator() {
    let boot = parse_source(&std::fs::read_to_string("os/boot.br").unwrap()).unwrap();
    let kernel = "package main\nfunc console_write(s: text)\nend\nfunc main()\n    let n = heap_bytes()\nend\n";
    brasa::codegen_x64::build_os(&boot, &parse_source(kernel).unwrap()).unwrap();
}

// ---- running ----

#[test]
fn lists_come_from_the_program_and_go_back_to_it() {
    let body = "let before = live_objects()
var total = 0
for i in 0..1000
    let xs = [i, i + 1, i + 2]
    total += xs[0] + xs[2]
end
let a = allocs
let f = frees
let diff = live_objects() - before
println(total)
println(a > 1000)
println(a - f)
println(diff)
println(freed_size == last_size and freed_align == 16)";
    let all = everywhere(body);
    for (label, r) in all {
        assert_eq!((r.out.as_str(), r.code), ("1001000\ntrue\n0\n0\ntrue\n", 0), "{}: {}", label, r.err);
    }
}

#[test]
fn objects_are_zeroed_even_when_the_allocator_returns_garbage() {
    let body = "type Pair = record
    a: int
    b: int
end
let p = Pair{}
println(p.a + p.b)";
    // the allocator fills the first 8 bytes with 255 (the header's size word is overwritten by
    // the runtime, the rest of the block is zeroed by it)
    let src_w = with_allocator(WINDOWS_ALLOCATOR, "let p = Pair{}\nprintln(p.a + p.b)").replace("func main()", "type Pair = record\n    a: int\n    b: int\nend\n\nfunc main()");
    if let Some(r) = windows(&src_w) {
        assert_eq!((r.out.as_str(), r.code), ("0\n", 0), "{}", r.err);
    }
    let _ = body;
}

#[test]
fn out_of_memory_ends_the_program() {
    let body = "fail = true\nlet xs = [1, 2, 3]\nprintln(xs[0])";
    for (label, r) in everywhere(body) {
        assert_eq!(r.out, "", "{}", label);
        assert!(r.err.contains("out of memory"), "{}: {}", label, r.err);
        assert_ne!(r.code, 0, "{}", label);
    }
}

#[test]
fn list_growth_uses_realloc_when_declared() {
    let body = "var xs: List<int> = []
for i in 0..200
    xs.push(i)
end
var total = 0
for x in xs
    total += x
end
let r = reallocs
xs = []
let a = allocs
let f = frees
println(total)
println(r > 0)
println(a - f)";
    let mut all = Vec::new();
    if let Some(r) = windows(&with_allocator(&format!("{}{}", WINDOWS_ALLOCATOR, REALLOC), body)) {
        all.push(("windows", r));
    }
    if let Some(r) = linux(&with_allocator(&format!("{}{}", LINUX_ALLOCATOR, REALLOC), body)) {
        all.push(("linux", r));
    }
    for (label, r) in all {
        assert_eq!((r.out.as_str(), r.code), ("19900
true
1
", 0), "{}: {}", label, r.err);
    }
}

#[test]
fn list_growth_without_realloc_allocates_copies_and_frees() {
    let body = "var xs: List<int> = []
for i in 0..200
    xs.push(i)
end
let r = reallocs
let a = allocs
println(r)
println(a > 4)";
    for (label, r) in everywhere(body) {
        assert_eq!((r.out.as_str(), r.code), ("0
true
", 0), "{}: {}", label, r.err);
    }
}

#[test]
fn the_allocator_example_runs() {
    let src = std::fs::read_to_string("examples/allocator.br").unwrap();
    let Some(r) = windows(&src) else { return };
    assert_eq!((r.out.as_str(), r.code), ("bump allocator arena \nallocations came from the arena: true\nfrees were counted: true\n", 0), "{}", r.err);
}
