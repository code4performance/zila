//! The Linux target: static ELF executables built by the compiler alone. The executables are run
//! through WSL (the tests skip, with a message, when WSL is not usable); the ELF writer and the
//! option parsing are tested without it.

use zila::codegen_x64::{compile_to_linux, compile_to_native, elf_image, generate_asm_for, CompileOptions, Target};
use zila::parse_source;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("zila-linux-test-{}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), name))
}

/// `C:\Users\x\file` as the path WSL sees.
fn wsl_path(p: &std::path::Path) -> String {
    let s = p.to_str().unwrap().replace('\\', "/");
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(d), Some(':')) => format!("/mnt/{}{}", d.to_ascii_lowercase(), &s[2..]),
        _ => s,
    }
}

/// Whether WSL can run Linux programs here.
fn wsl_works() -> bool {
    static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CHECK.get_or_init(|| match Command::new("wsl.exe").args(["-e", "sh", "-c", "echo ready"]).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("ready"),
        Err(_) => false,
    })
}

/// Builds `src` for Linux and runs it under WSL: (standard output followed by standard error,
/// exit status). `None` without WSL.
fn run_linux(src: &str) -> Option<(String, i32)> {
    let (out, err, code) = run_linux_split(src)?;
    Some((out + &err, code))
}

/// Like `run_linux`, with the two streams kept apart: (standard output, standard error, status).
fn run_linux_split(src: &str) -> Option<(String, String, i32)> {
    if !wsl_works() {
        eprintln!("WSL is not available: skipping");
        return None;
    }
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("prog");
    compile_to_linux(&file, exe.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("compile error: {}", e));
    let name = exe.file_name().unwrap().to_str().unwrap().to_string();
    let script = format!("cp '{}' /tmp/{n} && chmod +x /tmp/{n} && /tmp/{n} 2>/tmp/{n}.err; echo \"__EXIT:$?\"; cat /tmp/{n}.err; rm -f /tmp/{n} /tmp/{n}.err", wsl_path(&exe), n = name);
    let out = Command::new("wsl.exe").args(["-e", "sh", "-c", &script]).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    let (body, rest) = text.rsplit_once("__EXIT:").unwrap_or_else(|| panic!("no exit marker in: {}", text));
    let (code, err) = rest.split_once('\n').unwrap_or((rest, ""));
    Some((body.to_string(), err.to_string(), code.trim().parse().unwrap()))
}

/// The same program built for Windows and run: its output. `None` without the Windows toolchain.
fn run_windows(src: &str) -> Option<String> {
    let file = parse_source(src).unwrap();
    let exe = temp("win.exe");
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") => return None,
        Err(e) => panic!("windows compile error: {}", e),
    }
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    Some(String::from_utf8_lossy(&out.stdout).replace('\r', ""))
}

fn main_of(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nfunc main()\n{}end\n", indented)
}

// ---- without WSL ----

#[test]
fn the_elf_header_describes_one_loadable_segment() {
    let img = elf_image(&[0x90, 0xC3], 0x400078);
    assert_eq!(&img[..4], b"\x7FELF");
    assert_eq!((img[4], img[5], img[6]), (2, 1, 1), "64-bit, little-endian, version 1");
    assert_eq!(u16::from_le_bytes([img[16], img[17]]), 2, "an executable");
    assert_eq!(u16::from_le_bytes([img[18], img[19]]), 0x3E, "x86-64");
    assert_eq!(u64::from_le_bytes(img[24..32].try_into().unwrap()), 0x400078, "entry point");
    assert_eq!(u64::from_le_bytes(img[32..40].try_into().unwrap()), 64, "program headers follow the header");
    assert_eq!(u16::from_le_bytes([img[56], img[57]]), 1, "one program header");
    // the program header: PT_LOAD, rwx, offset 0, address 0x400000, the file, and memory for the heap
    assert_eq!(u32::from_le_bytes(img[64..68].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(img[68..72].try_into().unwrap()), 7);
    assert_eq!(u64::from_le_bytes(img[80..88].try_into().unwrap()), 0x400000);
    assert_eq!(u64::from_le_bytes(img[96..104].try_into().unwrap()), img.len() as u64, "p_filesz is the whole file");
    assert!(u64::from_le_bytes(img[104..112].try_into().unwrap()) >= img.len() as u64 + 256 * 1024 * 1024, "p_memsz leaves room for the heap");
    assert_eq!(&img[120..], &[0x90, 0xC3], "the code follows the headers");
}

#[test]
fn the_linux_assembly_uses_system_calls_and_no_windows_api() {
    let file = parse_source(&main_of("print(\"hi\")")).unwrap();
    let asm = generate_asm_for(&file, &CompileOptions::default(), Target::Linux).unwrap();
    assert!(asm.contains("syscall") && asm.contains("zila_start"), "{}", &asm[..200.min(asm.len())]);
    for api in ["kernel32", "ExitProcess", "WriteFile", "HeapAlloc", "SetConsoleOutputCP", "extern "] {
        assert!(!asm.contains(api), "the Linux assembly mentions {}", api);
    }
}

#[test]
fn the_target_option_is_checked() {
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_zila-parse")).args(args).output().unwrap();
    let out = run(&["x.zila", "--target", "solaris"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("unknown target 'solaris'") && err.contains("windows") && err.contains("linux"), "{}", err);
    let out = run(&["x.zila", "--target"]);
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("--target needs a name"));
}

#[test]
fn ws_is_internal_to_net_on_both_targets() {
    for target in [Target::Linux, Target::Windows] {
        let file = parse_source("package main

use ws

func main()
end
").unwrap();
        let e = generate_asm_for(&file, &CompileOptions::default(), target).unwrap_err();
        assert!(e.contains("'ws' is internal to 'net'"), "{}", e);
    }
    // `net` itself still builds on Windows, where it uses `ws`
    let file = parse_source("package main

use net

func main()
end
").unwrap();
    assert!(generate_asm_for(&file, &CompileOptions::default(), Target::Windows).is_ok());
}

#[test]
fn the_pure_libraries_are_accepted() {
    let src = "package main\n\nuse text\nuse list\nuse format\nuse sorted\nuse set\nuse math\nuse bufio\n\nfunc main()\n    print(text.upper(\"x\"))\nend\n";
    let file = parse_source(src).unwrap();
    assert!(generate_asm_for(&file, &CompileOptions::default(), Target::Linux).is_ok());
}

#[test]
fn the_windows_build_and_the_boot_image_are_unchanged_by_the_new_target() {
    // the Windows assembly still imports from kernel32 and still has no system calls
    let file = parse_source(&main_of("print(\"hi\")")).unwrap();
    let asm = generate_asm_for(&file, &CompileOptions::default(), Target::Windows).unwrap();
    assert!(asm.contains("GOT_ExitProcess dq 0") && !asm.contains("syscall"));
}

// ---- under WSL ----

#[test]
fn hello_prints_and_exits_with_zero() {
    let Some((out, code)) = run_linux(&main_of("print(\"hello from linux\")\nprint(1 + 2)")) else { return };
    assert_eq!((out.as_str(), code), ("hello from linux\n3\n", 0));
}

#[test]
fn an_error_from_main_and_an_abort_set_the_exit_status() {
    let src = "package main\n\nfunc main() -> error\n    print(\"before\")\n    return error(\"boom\")\nend\n";
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!((out.as_str(), code), ("before\nerror: boom\n", 1));
    let Some((out, code)) = run_linux(&main_of("print(\"x\")\nabort(\"stop\")")) else { return };
    assert_eq!((out.as_str(), code), ("x\nabort: main.zila:5: stop\n", 101));
}

#[test]
fn division_by_zero_stops_the_program() {
    for body in ["var z = 0\nprint(10 / z)", "var z = 0\nprint(10 % z)", "print(10 / 0)"] {
        let Some((out, code)) = run_linux(&main_of(body)) else { return };
        assert_eq!(code, 101, "{}: {}", body, out);
        assert!(out.contains("abort: main.zila:") && out.contains(": division by zero"), "{}: {}", body, out);
    }
}

#[test]
fn memory_is_given_back_and_big_values_work() {
    let src = main_of(
        "var big = \"0123456789\"\nfor i in 0..14\n    big = big + big\nend\nlet before = live_objects()\nvar total = 0\nfor round in 0..300\n    let copy = big + \"x\"\n    let xs: []int = []\n    for i in 0..2000\n        xs.add(i)\n    end\n    total += copy.len + xs.len\nend\nlet grew = live_objects() - before\nprint(\"{big.len} {total > 0} {grew}\")\n",
    );
    let Some((out, code)) = run_linux(&src) else { return };
    assert_eq!((out.as_str(), code), ("163840 true 0\n", 0));
}

#[test]
fn a_large_list_and_many_small_objects() {
    let src = main_of(
        "let xs: []int = []\nfor i in 0..300000\n    xs.add(i)\nend\nvar total: long = 0\nfor x in xs\n    total += x\nend\nlet names: []text = []\nfor i in 0..20000\n    names.add(\"name {i}\")\nend\nprint(\"{total} {names.len} {names[19999]}\")\n",
    );
    let Some((out, code)) = run_linux(&src) else { return };
    assert_eq!((out.as_str(), code), ("44999850000 20000 name 19999\n", 0));
}

#[test]
fn maps_floats_text_and_the_pure_libraries_work() {
    let src = "package main\n\nuse text\nuse list\nuse format\nuse sorted\n\nfunc main()\n    let m: Map<text, int> = {}\n    m[\"b\"] = 2\n    m[\"a\"] = 1\n    print(m.len)\n    print(2.5 * 2.0)\n    print(text.upper(\"abc ação\"))\n    print(format.fmt(\"%(08x)|%(>8.2f)\", 255, 3.14159) ?? \"?\")\n    let s: sorted.SortedSet<int> = sorted.set()\n    for x in [3, 1, 2]\n        s.add(x)\n    end\n    print(s.min() ?? 0)\n    print(list.range(0, 3).len)\nend\n";
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!(code, 0, "{}", out);
    assert_eq!(out, "2\n5.0\nABC AçãO\n000000ff|    3.14\n1\n3\n");
}

#[test]
fn maps_with_record_keys_work() {
    let src = "package main\n\ntype P = record\n    x: int\n    y: int\nend\n\nfunc main()\n    let m: Map<P, text> = {}\n    m[P { x: 1, y: 2 }] = \"a\"\n    m[P { x: 1, y: 2 }] = \"b\"\n    m[P { x: 2, y: 1 }] = \"c\"\n    print(\"{m.len} {m[P { x: 1, y: 2 }]} {P { x: 1, y: 2 } < P { x: 2, y: 0 }}\")\nend\n";
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!(code, 0, "{}", out);
    assert_eq!(out, "2 b true\n");
}

#[test]
fn programs_that_do_not_use_the_windows_api_print_the_same_on_both_targets() {
    let examples = ["async", "defer", "floats", "generics", "hello", "lines", "maps", "report", "sets", "sorted", "udp", "values", "words"];
    let mut compared = 0;
    for name in examples {
        let src = std::fs::read_to_string(format!("examples/{}.zila", name)).unwrap();
        let Some((linux, code)) = run_linux(&src) else { return };
        let Some(windows) = run_windows(&src) else { return };
        assert_eq!(code, 0, "{} exited with {}: {}", name, code, linux);
        assert_eq!(linux, windows, "{}: the output differs between Linux and Windows", name);
        compared += 1;
    }
    assert_eq!(compared, examples.len());
    // the examples that use files leave them in the current directory
    for leftover in ["zila-example.txt", "lines-example.txt"] {
        let _ = std::fs::remove_file(leftover);
    }
}

#[test]
fn running_out_of_memory_is_an_abort() {
    let src = main_of("let xs: []text = []\nvar i = 0\nwhile true\n    xs.add(\"some text number {i}\")\n    i += 1\nend\n");
    let Some((out, code)) = run_linux(&src) else { return };
    assert_eq!(code, 101, "{}", out);
    assert!(out.contains("abort: out of memory"), "{}", out);
}

// ---- library layers ----

/// `name(params) -> result` for every function, and the name of every type, of a library source.
fn declarations(src: &str) -> std::collections::BTreeMap<String, String> {
    use zila::ast::DeclKind;
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let mut out = std::collections::BTreeMap::new();
    for d in &file.decls {
        match &d.kind {
            DeclKind::Func(f) => {
                let recv = f
                    .receiver
                    .as_ref()
                    .map(|r| match &r.ty {
                        zila::ast::Type::Named { path, .. } => format!("{}.", path.join(".")),
                        other => format!("{:?}.", other),
                    })
                    .unwrap_or_default();
                let params: Vec<String> = f.params.iter().map(|p| format!("{:?}", p.ty)).collect();
                out.insert(format!("{}{}", recv, f.name), format!("async={} ({}) -> {:?}", f.is_async, params.join(", "), f.ret));
            }
            DeclKind::Type(t) => {
                out.insert(format!("type {}", t.name), String::new());
            }
            _ => {}
        }
    }
    out
}

/// The names a Linux layer must declare with the same signature as the Windows file: what the other
/// libraries and programs use.
fn assert_same_api(windows: &str, linux: &str, names: &[&str]) {
    let (w, l) = (declarations(windows), declarations(linux));
    for n in names {
        let (a, b) = (w.get(*n), l.get(*n));
        assert!(a.is_some(), "std Windows layer has no '{}'", n);
        assert_eq!(a, b, "'{}' differs between the Windows and the Linux layer", n);
    }
}

#[test]
fn the_sys_layers_share_their_api() {
    assert_same_api(
        include_str!("../std/sys.zila"),
        include_str!("../std/sys.linux.zila"),
        &["type Buf", "type Scratch", "buffer", "trim_end", "describe", "Buf.drop"],
    );
}

#[test]
fn a_library_prefers_its_linux_layer_on_linux() {
    use zila::codegen_x64::std_source_for;
    let win = std_source_for("sys", Target::Windows).unwrap();
    let lin = std_source_for("sys", Target::Linux).unwrap();
    assert!(win.contains("FormatMessageA") && !lin.contains("FormatMessageA"));
    assert_eq!(std_source_for("text", Target::Linux), std_source_for("text", Target::Windows));
}

#[test]
fn system_calls_and_error_messages_work_on_linux() {
    let src = "package main\n\nuse sys\n\nfunc main()\n    print(sys.syscall(39, 0, 0, 0, 0, 0, 0) > 0)\n    let r = sys.syscall(2, 0, 0, 0, 0, 0, 0)\n    print(sys.failed(r))\n    print(sys.describe(-2, \"x\"))\n    print(sys.message(999))\nend\n";
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!((out.as_str(), code), ("true\ntrue\nx: No such file or directory\nUnknown error 999\n", 0));
}

// ---- tasks, timers and channels ----

/// The same program on Linux (under WSL) and on Windows: both outputs must be equal.
fn same_on_both(src: &str) -> Option<(String, i32)> {
    let (out, code) = run_linux(src)?;
    if let Some(win) = run_windows(src) {
        assert_eq!(out, win, "Linux and Windows differ");
    }
    Some((out, code))
}

#[test]
fn sleeps_overlap_and_fire_in_deadline_order() {
    let src = r#"package main
use task
async func nap(ms: int) -> int
    await sleep(ms)
    return ms
end
async func tell(c: Channel<int>, ms: int)
    await sleep(ms)
    c.send(ms)
end
func main()
    let t0 = task.now()
    await sleep(80)
    print("slept {task.now() - t0 >= 80}")
    let s = task.now()
    let a = nap(100)
    let b = nap(100)
    let c = nap(100)
    print(await a + await b + await c)
    print("parallel {task.now() - s < 250}")
    let ch = channel(int)
    tell(ch, 30)
    tell(ch, 10)
    tell(ch, 20)
    print("{ch.receive()} {ch.receive()} {ch.receive()}")
end
"#;
    let Some((out, code)) = same_on_both(src) else { return };
    assert_eq!((out.as_str(), code), ("slept true\n300\nparallel true\n10 20 30\n", 0));
}

#[test]
fn spawned_tasks_yield_and_share_a_channel() {
    let src = r#"package main
use task
func worker(name: text, n: int, out: Channel<text>)
    for i in 0..n
        out.send("{name}{i}")
        task.yield()
    end
end
func main()
    let out = channel(text)
    spawn worker("a", 3, out)
    spawn worker("b", 3, out)
    var all = ""
    for i in 0..6
        all = all + out.receive() + " "
    end
    print(all)
end
"#;
    let Some((out, code)) = same_on_both(src) else { return };
    assert_eq!((out.as_str(), code), ("a0 b0 a1 b1 a2 b2 \n", 0));
}

#[test]
fn a_stuck_program_reports_a_deadlock_and_a_timer_is_not_one() {
    let stuck = "package main\nfunc main()\n    let c = channel(int)\n    c.receive()\nend\n";
    let Some((out, code)) = same_on_both(stuck) else { return };
    assert!(out.contains("deadlock"), "{}", out);
    assert_eq!(code, 2);
    let timer = "package main\nfunc main()\n    await sleep(100)\n    print(\"woke\")\nend\n";
    let Some((out, code)) = same_on_both(timer) else { return };
    assert_eq!((out.as_str(), code), ("woke\n", 0));
}

#[test]
fn ten_thousand_tasks_run_and_their_stacks_are_reused() {
    let src = r#"package main
use task
func add(total: Channel<int>, n: int)
    total.send(n)
end
func main()
    let total = channel(int)
    var sum = 0
    for round in 0..10
        for i in 0..1000
            spawn add(total, i)
        end
        for i in 0..1000
            sum += total.receive()
        end
    end
    print(sum)
end
"#;
    let Some((out, code)) = same_on_both(src) else { return };
    assert_eq!((out.as_str(), code), ("4995000\n", 0));
}

#[test]
fn many_tasks_alive_at_once() {
    let src = r#"package main
use task
func waiter(gate: Channel<int>, done: Channel<int>)
    gate.receive()
    done.send(1)
end
func main()
    let gate = channel(int)
    let done = channel(int)
    for i in 0..10000
        spawn waiter(gate, done)
    end
    task.yield()
    for i in 0..10000
        gate.send(0)
    end
    var n = 0
    for i in 0..10000
        n += done.receive()
    end
    print(n)
end
"#;
    let Some((out, code)) = same_on_both(src) else { return };
    assert_eq!((out.as_str(), code), ("10000\n", 0));
}

// ---- files ----

/// A program that works on a file called `name` in the current directory; the file is removed
/// afterwards. Runs on both targets.
fn with_local_file(name: &str, src: &str) -> Option<(String, i32)> {
    let src = src.replace("PATH", name);
    let result = same_on_both(&src);
    let _ = std::fs::remove_file(name);
    result
}

/// `with_local_file` for programs whose timing differs between the targets: Linux only.
fn with_linux_file(name: &str, src: &str) -> Option<(String, i32)> {
    let result = run_linux(&src.replace("PATH", name));
    let _ = std::fs::remove_file(name);
    result
}

#[test]
fn files_round_trip_missing_files_and_large_files() {
    let src = r#"package main
use fs
use task
use text
var ticks = 0
async func ticker()
    while true
        await sleep(2)
        ticks += 1
    end
end
func main() -> error
    try await fs.write("PATH", "hello, files: acentuação ok")
    print(try await fs.read("PATH"))
    match await fs.read("PATH.missing")
        ok(t) -> print("unexpected")
        err(e) -> print("starts right {text.starts_with(e.message, "PATH.missing: ")}")
    end
    var big = "0123456789abcdef"
    for i in 0..18
        big = big + big
    end
    let t = ticker()
    try await fs.write("PATH", big)
    let again = try await fs.read("PATH")
    print("identical {again == big} {again.len}")
    print("ticker ran {ticks > 0}")
    return
end
"#;
    let Some((out, code)) = with_local_file("zila-parity-roundtrip.txt", src) else { return };
    assert_eq!(out, "hello, files: acentuação ok\nstarts right true\nidentical true 4194304\nticker ran true\n");
    assert_eq!(code, 0);
}

#[test]
fn a_missing_file_gives_the_reason_linux_gives() {
    let src = "package main\nuse fs\nfunc main() -> error\n    match await fs.read(\"/no/such/dir/file.txt\")\n        ok(t) -> print(\"unexpected\")\n        err(e) -> print(e.message)\n    end\n    match await fs.create(\"/proc/nope/x\")\n        ok(t) -> print(\"unexpected\")\n        err(e) -> print(e.message)\n    end\n    return\nend\n";
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!(out, "/no/such/dir/file.txt: No such file or directory\n/proc/nope/x: No such file or directory\n");
    assert_eq!(code, 0);
}

#[test]
fn buffered_readers_and_writers_work_on_files() {
    let src = r#"package main
use fs

func main() -> error
    let w = try await fs.create("PATH")
    try await w.write_line("alpha")
    try await w.write_line("beta")
    try await w.write_line("ação")
    try await w.close()
    let a = try await fs.append("PATH")
    try await a.write("tail")
    try await a.close()

    let r = try await fs.open("PATH")
    var count = 0
    while not try await r.at_end()
        let line = try await r.read_line()
        count += 1
        print("{count}: {line}")
    end
    print(try await r.read_all() == "")
    await r.close()
    return
end
"#;
    let Some((out, code)) = with_local_file("zila-parity-lines.txt", src) else { return };
    assert_eq!((out.as_str(), code), ("1: alpha\n2: beta\n3: ação\n4: tail\ntrue\n", 0));
}

#[test]
fn other_tasks_run_while_a_big_file_is_written() {
    let src = r#"package main
use fs
use task
var ticks = 0
async func ticker()
    while true
        await sleep(1)
        ticks += 1
    end
end
func main() -> error
    var big = "0123456789abcdef"
    for i in 0..20
        big = big + big
    end
    let t = ticker()
    try await fs.write("PATH", big)
    print(ticks > 0)
    return
end
"#;
    let Some((out, code)) = with_linux_file("zila-parity-big.txt", src) else { return };
    assert_eq!((out.as_str(), code), ("true\n", 0));
}

// ---- sockets and names ----

#[test]
fn tcp_echo_moves_large_transfers_and_sees_the_peer_close() {
    let src = r#"package main
use net
async func server(l: net.Listener, total: int) -> error
    let conn = try await l.accept()
    var got = 0
    while got < total
        let piece = try await conn.read(65536)
        if piece.len == 0
            break
        end
        got += piece.len
        try await conn.write(piece)
    end
    try await conn.write("bye")
    await conn.close()
    return
end
func main() -> error
    let l = try await net.listen(0)
    var big = "0123456789abcdef"
    for i in 0..14
        big = big + big
    end
    let s = server(l, big.len)
    let c = try await net.connect("127.0.0.1", l.port)
    try await c.write(big)
    // the echo and then `bye`; they may arrive in any number of reads
    var all = ""
    while true
        let piece = try await c.read(65536)
        if piece.len == 0
            break
        end
        all = all + piece
    end
    print("received {all.len == big.len + 3}")
    print(all.slice(all.len - 3, all.len))
    print("closed [{try await c.read(16)}]")
    await c.close()
    await l.close()
    return
end
"#;
    let Some((out, code)) = same_on_both(src) else { return };
    assert_eq!((out.as_str(), code), ("received true\nbye\nclosed []\n", 0));
}

#[test]
fn readers_and_writers_work_over_tcp() {
    let src = r#"package main
use net
async func server(l: net.Listener) -> error
    let conn = try await l.accept()
    let r = conn.reader()
    let w = conn.writer()
    while true
        match await r.read_line()
            ok(line) -> try await w.write_line("got {line}")
            err(e) -> break
        end
        try await w.flush()
    end
    await conn.close()
    return
end
func main() -> error
    let l = try await net.listen(0)
    let s = server(l)
    let conn = try await net.connect("127.0.0.1", l.port)
    let w = conn.writer()
    let r = conn.reader()
    try await w.write_line("one")
    try await w.flush()
    print(try await r.read_line())
    try await w.write_line("two")
    try await w.flush()
    print(try await r.read_line())
    await conn.close()
    return
end
"#;
    let Some((out, code)) = same_on_both(src) else { return };
    assert_eq!((out.as_str(), code), ("got one\ngot two\n", 0));
}

#[test]
fn a_refused_connection_and_a_busy_port_are_errors_that_name_what_failed() {
    let src = r#"package main
use net
use text
func main() -> error
    match await net.connect("127.0.0.1", 1)
        ok(c) -> print("unexpected")
        err(e) -> print("refused {text.starts_with(e.message, "connect to 127.0.0.1:1: ")}")
    end
    let l = try await net.listen(0)
    match await net.listen(l.port)
        ok(x) -> print("unexpected")
        err(e) -> print("busy {text.starts_with(e.message, "cannot listen on port ")}")
    end
    return
end
"#;
    let Some((out, code)) = same_on_both(src) else { return };
    assert_eq!((out.as_str(), code), ("refused true\nbusy true\n", 0));
}

#[test]
fn udp_datagrams_carry_their_sender_and_oversized_ones_are_an_error() {
    let src = r#"package main
use net
func main() -> error
    let a = try await net.udp(0)
    let b = try await net.udp(0)
    try await a.send_to("127.0.0.1", b.port, "ping")
    let p = try await b.recv()
    print("{p.data} from {p.host} {p.port == a.port}")
    try await a.send_to("127.0.0.1", b.port, "0123456789")
    match await b.recv(4)
        ok(x) -> print("unexpected")
        err(e) -> print(e.message)
    end
    try await a.send_to("localhost", b.port, "again")
    print((try await b.recv()).data)
    await a.close()
    match await a.recv()
        ok(x) -> print("unexpected")
        err(e) -> print(e.message)
    end
    return
end
"#;
    let Some((out, code)) = same_on_both(src) else { return };
    assert_eq!((out.as_str(), code), ("ping from 127.0.0.1 true\ndatagram larger than 4 bytes\nagain\nthe socket is closed\n", 0));
}

#[test]
fn local_and_unknown_names_are_looked_up() {
    let src = r#"package main
use net
func main() -> error
    print(try await net.lookup("localhost"))
    print(try await net.lookup("10.1.2.3"))
    print(try await net.resolve("127.0.0.1"))
    match await net.lookup("no-such-host.invalid")
        ok(ip) -> print("unexpected {ip}")
        err(e) -> print(e.message)
    end
    return
end
"#;
    let Some((out, code)) = same_on_both(src) else { return };
    assert_eq!((out.as_str(), code), ("127.0.0.1\n10.1.2.3\n2130706433\ncannot resolve 'no-such-host.invalid'\n", 0));
}

#[test]
fn a_real_name_resolves_when_a_resolver_answers() {
    let src = "package main\nuse net\nfunc main() -> error\n    match await net.lookup(\"example.com\")\n        ok(ip) -> print(\"ok {net.parse_ip(ip) > 0}\")\n        err(e) -> print(\"error {e.message}\")\n    end\n    return\nend\n";
    let Some((out, _)) = run_linux(src) else { return };
    if out.starts_with("error ") {
        eprintln!("no resolver answered: skipping ({})", out.trim());
        return;
    }
    assert_eq!(out, "ok true\n");
}

#[test]
fn dns_answers_are_read_with_compression_cnames_truncation_and_missing_names() {
    // A response is built by hand: header, the question `www.example.com A`, then the answers.
    let src = r#"package main
use net
use sys

func put(a: int, bytes: []int)
    for i in 0..bytes.len
        sys.put8(a + i, bytes[i])
    end
end

// id 0x1234, the given flags, one question, `answers` answers; the question starts at 12
func header(a: int, flags: int, answers: int)
    put(a, [0x12, 0x34, flags >> 8, flags bitand 255, 0, 1, 0, answers, 0, 0, 0, 0])
    put(a + 12, [3, 119, 119, 119, 7, 101, 120, 97, 109, 112, 108, 101, 3, 99, 111, 109, 0, 0, 1, 0, 1])
end

func show(a: net.Answer)
    print("[{a.value}] truncated={a.truncated} missing={a.missing}")
end

func main()
    let b = sys.buffer(512)
    let a = int(b.ptr)
    // a CNAME pointing back at the question name (compressed), then an A record
    header(a, 0x8180, 2)
    put(a + 33, [0xC0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xC0, 12])
    put(a + 47, [0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 93, 184, 216, 34])
    show(net.parse_answer(a, 63, 0x1234))
    // only the CNAME
    header(a, 0x8180, 1)
    put(a + 33, [0xC0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xC0, 12])
    show(net.parse_answer(a, 47, 0x1234))
    // cut short, a name that does not exist, an answer to another question, a query
    header(a, 0x8380, 0)
    show(net.parse_answer(a, 33, 0x1234))
    header(a, 0x8183, 0)
    show(net.parse_answer(a, 33, 0x1234))
    header(a, 0x8180, 0)
    show(net.parse_answer(a, 33, 0x9999))
    header(a, 0x0100, 0)
    show(net.parse_answer(a, 33, 0x1234))
    print(net.read_name(a, 33, 12))
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!(
        out,
        "[93.184.216.34] truncated=false missing=false\n[=www.example.com] truncated=false missing=false\n[] truncated=true missing=false\n[] truncated=false missing=true\n[] truncated=false missing=false\n[] truncated=false missing=false\nwww.example.com\n"
    );
    assert_eq!(code, 0);
}

#[test]
fn the_dns_client_asks_over_udp_and_falls_back_to_tcp() {
    // two fake name servers: one answers over UDP (once cut short, once whole), one over TCP
    let src = r#"package main
use c
use net
use sys

// The reply to `query` (a DNS query without the TCP length prefix): the same question, the given
// flags and, when `answer` is set, one A record 1.2.3.4. `prefix` adds the two length bytes.
func reply(query: text, flags: int, answer: bool, prefix: bool) -> text
    var extra = 0
    if answer
        extra = 16
    end
    var skip = 0
    if prefix
        skip = 2
    end
    let b = sys.buffer(query.len + extra + 2)
    let a = long(b.ptr)
    for i in 0..query.len
        sys.put8(a + skip + i, long(query.byte(i)))
    end
    sys.put8(a + skip + 2, long(flags >> 8))
    sys.put8(a + skip + 3, long(flags bitand 255))
    if answer
        sys.put8(a + skip + 7, 1)
        let rr = a + skip + query.len
        sys.poke(rr, 0x0000010001000CC0)
        sys.poke(rr + 8, 0x0403020104003C00)
    end
    var total = query.len + extra
    if prefix
        sys.put8(a, long(total >> 8))
        sys.put8(a + 1, long(total bitand 255))
        total += 2
    end
    return c.from_bytes(b.ptr, total)
end

async func udp_server(u: net.UdpSocket) -> error
    // the first question is answered "cut short", the second whole
    let p1 = try await u.recv()
    try await u.send_to(p1.host, p1.port, reply(p1.data, 0x8380, false, false))
    let p2 = try await u.recv()
    try await u.send_to(p2.host, p2.port, reply(p2.data, 0x8180, true, false))
    return
end

async func tcp_server(l: net.Listener) -> error
    let conn = try await l.accept()
    var q = ""
    while q.len < 2 or q.len < int(q.byte(0)) * 256 + int(q.byte(1)) + 2
        let piece = try await conn.read(512)
        if piece.len == 0
            break
        end
        q = q + piece
    end
    try await conn.write(reply(q.__raw_slice(2, q.len), 0x8180, true, true))
    return
end

func main() -> error
    let u = try await net.udp(0)
    let s1 = udp_server(u)
    let cut = try await net.ask_udp("127.0.0.1", "example.org", u.port)
    print("cut short: {cut.truncated} [{cut.value}]")
    let whole = try await net.ask_udp("127.0.0.1", "example.org", u.port)
    print("whole: {whole.truncated} [{whole.value}]")
    let l = try await net.listen(0)
    let s2 = tcp_server(l)
    let over_tcp = try await net.ask_tcp("127.0.0.1", "example.org", l.port)
    print("tcp: [{over_tcp.value}]")
    return
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!(out, "cut short: true []\nwhole: false [1.2.3.4]\ntcp: [1.2.3.4]\n");
    assert_eq!(code, 0);
}

#[test]
fn a_silent_name_server_times_out_without_stopping_other_tasks() {
    let src = r#"package main
use net
use task
var ticks = 0
async func ticker()
    while true
        await sleep(20)
        ticks += 1
    end
end
func main() -> error
    // a socket that never answers
    let silent = try await net.udp(0)
    let t = ticker()
    let t0 = task.now()
    match await net.ask_udp("127.0.0.1", "example.org", silent.port)
        ok(a) -> print("unexpected")
        err(e) -> print(e.message)
    end
    let took = task.now() - t0
    print("took {took >= 3900 and took < 5500}")
    print("ticker ran {ticks >= 100}")
    return
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!(out, format!("no answer from 127.0.0.1\ntook true\nticker ran true\n"));
    assert_eq!(code, 0);
}

#[test]
fn the_fs_net_and_task_layers_share_their_api() {
    assert_same_api(
        include_str!("../std/fs.zila"),
        include_str!("../std/fs.linux.zila"),
        &[
            "type File", "type Reader", "type Writer", "read", "write", "open", "create", "append", "File.close", "Reader.read_line", "Reader.read",
            "Reader.read_all", "Reader.at_end", "Reader.close", "Writer.write", "Writer.write_line", "Writer.flush", "Writer.close",
        ],
    );
    assert_same_api(
        include_str!("../std/net.zila"),
        include_str!("../std/net.linux.zila"),
        &[
            "type Conn", "type Listener", "type Reader", "type Writer", "type UdpSocket", "type Packet", "lookup", "resolve", "connect", "listen",
            "parse_ip", "Listener.accept", "Listener.close", "Conn.read", "Conn.write", "Conn.close", "Conn.reader", "Conn.writer", "Reader.read_line",
            "Reader.read", "Reader.read_all", "Reader.at_end", "Writer.write", "Writer.write_line", "Writer.flush", "Writer.close", "udp",
            "UdpSocket.send_to", "UdpSocket.recv", "UdpSocket.close", "Conn.cancel", "Listener.cancel",
        ],
    );
    assert_same_api(
        include_str!("../std/task.zila"),
        include_str!("../std/task.linux.zila"),
        &[
            "type Task", "type Chan", "init", "fatal", "now", "reap", "add_timer", "fire_timers", "poll_io", "wait_events", "switch_away", "park", "wake",
            "start", "yield", "chan_new", "wake_one", "chan_send", "chan_recv", "chan_close", "cur",
        ],
    );
}

// ---- calling C ----

/// The program headers of an ELF file: (type, flags) of each.
fn program_headers(file: &[u8]) -> Vec<(u32, u32)> {
    let phoff = u64::from_le_bytes(file[32..40].try_into().unwrap()) as usize;
    let phnum = u16::from_le_bytes(file[56..58].try_into().unwrap()) as usize;
    (0..phnum)
        .map(|i| {
            let at = phoff + 56 * i;
            (u32::from_le_bytes(file[at..at + 4].try_into().unwrap()), u32::from_le_bytes(file[at + 4..at + 8].try_into().unwrap()))
        })
        .collect()
}

fn build_file(src: &str) -> Vec<u8> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("headers");
    compile_to_linux(&file, exe.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("compile error: {}", e));
    let bytes = std::fs::read(&exe).unwrap();
    let _ = std::fs::remove_file(&exe);
    bytes
}

#[test]
fn ui_builds_on_linux_and_stays_a_static_executable() {
    let src = std::fs::read_to_string("examples/ui.zila").unwrap();
    let bytes = build_file(&src);
    assert_eq!(&bytes[..4], b"\x7fELF");
    // no interpreter and no dynamic section: the X11 protocol is spoken with system calls
    let kinds: Vec<u32> = program_headers(&bytes).iter().map(|h| h.0).collect();
    assert!(!kinds.contains(&3) && !kinds.contains(&2), "{:?}", kinds);
    // and the Windows build of the same file still works
    let file = parse_source(&src).unwrap();
    assert!(generate_asm_for(&file, &CompileOptions::default(), Target::Windows).is_ok());
}

#[test]
fn a_program_without_externs_stays_static_and_one_with_externs_is_dynamic() {
    let plain = build_file(&main_of("print(1)"));
    let kinds: Vec<u32> = program_headers(&plain).iter().map(|h| h.0).collect();
    assert_eq!(kinds, [1], "a static program has one loadable segment and nothing else");
    let c_src = "package main\n\n#[link(\"c\")]\nextern \"C\" func getpid() -> i32\n\nfunc main()\n    unsafe\n        print(getpid() > 0)\n    end\nend\n";
    let dynamic = build_file(c_src);
    let kinds: Vec<u32> = program_headers(&dynamic).iter().map(|h| h.0).collect();
    assert!(kinds.contains(&3) && kinds.contains(&2) && kinds.contains(&1), "{:?}", kinds);
    let text = String::from_utf8_lossy(&dynamic).to_string();
    assert!(text.contains("/lib64/ld-linux-x86-64.so.2") && text.contains("libc.so.6") && text.contains("getpid") && text.contains("exit"));
    // the stack is not executable
    assert!(program_headers(&dynamic).iter().any(|&(k, f)| k == 0x6474E551 && f == 6));
}

#[test]
fn c_functions_are_called_with_integers_pointers_floats_and_variadics() {
    let src = r#"package main
use c

#[link("c")]
extern "C" func getpid() -> i32
#[link("c")]
extern "C" func strlen(s: *u8) -> usize
#[link("c")]
extern "C" func atoi(s: *u8) -> i32
#[link("c")]
extern "C" func snprintf(buffer: *u8, size: usize, fmt: *u8, ...) -> i32
#[link("m")]
extern "C" func sqrt(x: f64) -> f64
#[link("m")]
extern "C" func pow(x: f64, y: f64) -> f64
#[link("m")]
extern "C" func floorf(x: f32) -> f32

func main()
    unsafe
        print(getpid() > 0)
        print(strlen(c.cstr("hello, world")))
        print(atoi(c.cstr("1234")) + 1)
        print(sqrt(2.0))
        print(pow(2.0, 10.0))
        print(floorf(2.75))
        // more than six integer arguments: the rest go on the stack
        let b = c.alloc(128)
        snprintf(b, 128, c.cstr("%d %d %d %d %d %d %d %d|%.3f|%s"), 1, 2, 3, 4, 5, 6, 7, 8, 3.14159, c.cstr("end"))
        print(c.from_cstr(b))
    end
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!((out.as_str(), code), ("true\n12\n1235\n1.4142135623730951\n1024.0\n2.0\n1 2 3 4 5 6 7 8|3.142|end\n", 0));
}

#[test]
fn a_function_can_be_given_to_c_as_a_callback() {
    let src = r#"package main
use c

#[link("c")]
extern "C" func qsort(base: *long, n: usize, size: usize, cmp: extern "C" func(*long, *long) -> i32)

func by_value(a: *long, b: *long) -> i32
    var x: long = 0
    var y: long = 0
    unsafe
        x = a[0]
        y = b[0]
    end
    if x < y
        return -1
    end
    if x > y
        return 1
    end
    return 0
end

func main()
    let data: *long = c.alloc(80)
    unsafe
        let values = [42, 7, 99, 1, 56, 3, 12, 8, 77, 0]
        for i in 0..10
            data[i] = values[i]
        end
        qsort(data, 10, 8, by_value)
        var line = ""
        for i in 0..10
            line = line + "{data[i]} "
        end
        print(line)
    end
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!((out.as_str(), code), ("0 1 3 7 8 12 42 56 77 99 \n", 0));
}

#[test]
fn what_c_buffered_is_written_in_order_with_what_zila_prints() {
    let src = r#"package main
use c

#[link("c")]
extern "C" func puts(s: *u8) -> i32

func main()
    unsafe
        puts(c.cstr("written by C"))
    end
    print("written by Zila Lang")
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    // C buffers its output when it is not a terminal; it is written before Zila Lang writes, in order
    assert_eq!((out.as_str(), code), ("written by C
written by Zila Lang
", 0));
}

#[test]
fn an_error_and_an_abort_still_exit_correctly_in_a_dynamic_program() {
    let head = "package main\nuse c\n\n#[link(\"c\")]\nextern \"C\" func getpid() -> i32\n\n";
    let src = format!("{}func main() -> error\n    unsafe\n        getpid()\n    end\n    return error(\"boom\")\nend\n", head);
    let Some((out, code)) = run_linux(&src) else { return };
    assert_eq!((out.as_str(), code), ("error: boom\n", 1));
    let src = format!("{}func main()\n    unsafe\n        getpid()\n    end\n    abort(\"stop\")\nend\n", head);
    let Some((out, code)) = run_linux(&src) else { return };
    assert_eq!((out.as_str(), code), ("abort: main.zila:11: stop\n", 101));
}

#[test]
fn every_library_is_in_the_compatibility_table() {
    let docs = std::fs::read_to_string("docs/native.html").unwrap();
    let section = &docs[docs.find("<section id=\"linux\">").unwrap()..];
    let section = &section[..section.find("</section>").unwrap()];
    for entry in std::fs::read_dir("std").unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let Some(lib) = name.strip_suffix(".zila") else { continue };
        if lib.ends_with(".linux") {
            continue;
        }
        assert!(section.contains(&format!("<code>{}</code>", lib)), "the Linux compatibility table does not mention '{}'", lib);
    }
    for entry in std::fs::read_dir("packages").unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let lib = path.file_name().unwrap().to_string_lossy().to_string();
            assert!(section.contains(&format!("<code>{}</code>", lib)), "the Linux compatibility table does not mention '{}'", lib);
        }
    }
}

// ---- tls ----

fn online() -> bool {
    use std::net::ToSocketAddrs;
    "example.com:443".to_socket_addrs().map(|mut a| a.next().is_some()).unwrap_or(false)
}

#[test]
fn tls_makes_an_https_request_and_refuses_a_bad_certificate() {
    if !online() {
        eprintln!("offline: skipping");
        return;
    }
    let src = r#"package main

use tls

func main() -> error
    let conn = try await tls.connect("example.com", 443)
    try await conn.write("GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
    var all = ""
    while true
        let part = try await conn.read(4096)
        if part.len == 0
            break
        end
        all = all + part
    end
    print(all.slice(0, 9))
    print(all.len > 500)
    match await tls.connect("expired.badssl.com", 443)
        ok(c) -> print("unexpected success")
        err(e) -> print("refused")
    end
    return
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!((out.as_str(), code), ("HTTP/1.1 \ntrue\nrefused\n", 0));
}

#[test]
fn tls_upgrade_alpn_and_a_private_ca_work_the_same_on_linux() {
    // needs the TLS servers of tests/docker (ZILA_TEST_CA names the development CA)
    let Ok(ca_win) = std::env::var("ZILA_TEST_CA") else {
        eprintln!("skipped: set ZILA_TEST_CA (see tests/docker/README.md) to run this test");
        return;
    };
    let p = ca_win.replace('\\', "/");
    let ca = format!("/mnt/{}{}", p[..1].to_ascii_lowercase(), &p[2..]);
    let src = r#"package main

use net
use tls
use crypto

func main() -> error
    let a = tls.Options{ alpn: ["tds/8.0", "http/1.1"], require_alpn: false, ca_file: "@CA@" }
    let conn = try await tls.connect_with("localhost", 55444, a)
    print("protocol=" + conn.protocol())
    let none = tls.Options{ alpn: ["tds/8.0"], require_alpn: true, ca_file: "@CA@" }
    match await tls.connect_with("localhost", 55443, none)
        ok(c) -> print("unexpected success")
        err(e) -> print(e.message)
    end
    match await tls.connect("localhost", 55443)
        ok(c) -> print("unexpected success")
        err(e) -> print("untrusted refused")
    end
    let bad = tls.Options{ alpn: [], require_alpn: false, ca_file: "/no/such/ca.pem" }
    match await tls.connect_with("localhost", 55443, bad)
        ok(c) -> print("unexpected success")
        err(e) -> print(e.message)
    end
    let raw = try await net.connect("127.0.0.1", 55432)
    try await raw.write(crypto.from_bytes([0, 0, 0, 8, 4, 210, 22, 47]))
    print(try await raw.read(1))
    let opts = tls.Options{ alpn: [], require_alpn: false, ca_file: "@CA@" }
    let up = try await tls.upgrade_with(raw, "localhost", opts)
    let startup = crypto.from_bytes([0, 0, 0, 32, 0, 3, 0, 0]) + "user" + crypto.from_bytes([0]) + "app" + crypto.from_bytes([0]) + "database" + crypto.from_bytes([0]) + "shop" + crypto.from_bytes([0, 0])
    try await up.write(startup)
    print(try await up.read(1))
    let raw2 = try await net.connect("127.0.0.1", 55432)
    try await raw2.write(crypto.from_bytes([0, 0, 0, 8, 4, 210, 22, 47]))
    print(try await raw2.read(1))
    match await tls.upgrade_with(raw2, "wrong.example", opts)
        ok(c) -> print("unexpected success")
        err(e) -> print("wrong name refused")
    end
    return
end
"#
    .replace("@CA@", &ca);
    let Some((out, code)) = run_linux(&src) else { return };
    let expected = "protocol=http/1.1\nTLS handshake with localhost failed: the server selected none of the offered protocols (tds/8.0)\nuntrusted refused\nTLS: cannot read the CA file /no/such/ca.pem\nS\nR\nS\nwrong name refused\n";
    assert_eq!((out.as_str(), code), (expected, 0));
}

#[test]
fn crypto_db_and_the_database_drivers_behave_the_same_on_linux() {
    // crypto and db (with a fake driver) always run; the drivers run when the Docker servers of
    // tests/docker are up (ZILA_TEST_POSTGRES / ZILA_TEST_SQLSERVER_STRICT, source env.ps1)
    let wsl_path = |p: String| {
        let p = p.replace('\\', "/");
        format!("/mnt/{}{}", p[..1].to_ascii_lowercase(), &p[2..])
    };
    let ca = std::env::var("ZILA_TEST_CA").ok().map(wsl_path);
    let mut body = String::from(
        "    print(crypto.hex(crypto.sha256(\"abc\")))\n    print(crypto.hex(crypto.md5(\"abc\")))\n    print((try crypto.random(8)).len)\n    let fake = db.connection(\n        func(sql, args)\n            var rs = db.rows([\"n\"], [\"int\"])\n            rs.add(\"41\", false)\n            rs.add(\"1\", false)\n            return rs\n        end,\n        func() return end,\n        func() return end,\n        func() return end,\n        func() end\n    )\n    let rows = try db.query(int, fake, \"select\")\n    print(rows[0] + rows[1])\n",
    );
    let mut expected = String::from("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n900150983cd24fb0d6963f7d28e17f72\n8\n42\n");
    if let (Some(ca), Ok(_)) = (&ca, std::env::var("ZILA_TEST_POSTGRES")) {
        body.push_str(&format!("    let pg = try postgres.open(\"postgres://app:Zila_app_dev_1@127.0.0.1:55432/shop?sslmode=require&sslrootcert={ca}\")\n    print(try db.one(int, pg, \"select count(*)::int from customers\"))\n"));
        expected.push_str("60\n");
    }
    if let (Some(ca), Ok(_)) = (&ca, std::env::var("ZILA_TEST_SQLSERVER_STRICT")) {
        body.push_str(&format!("    let ms = try sqlserver.open(\"sqlserver://sa:Zila_dev_Sql_1@127.0.0.1:51434/shop?encrypt=strict&cafile={ca}\")\n    print(try db.one(int, ms, \"select count(*) from customers\"))\n"));
        expected.push_str("60\n");
    }
    if ca.is_none() {
        eprintln!("the Linux half of the drivers is skipped: set ZILA_TEST_CA (see tests/docker/README.md)");
    }
    let src = format!("package main\n\nuse crypto\nuse db\nuse postgres\nuse sqlserver\n\nfunc main() -> error\n{body}    return\nend\n");
    let Some((out, code)) = run_linux(&src) else { return };
    assert_eq!((out.as_str(), code), (expected.as_str(), 0));
}

#[test]
fn ai_environment_and_schema_behave_the_same_on_linux() {
    // sys.env reads /proc/self/environ on Linux; the schema is made at build time
    let src = r#"package main

use ai
use sys

type Item = record
    #[desc("a name")] name: text
    count: int
    tag: text?
end

func main() -> error
    print((sys.env("PATH") ?? "").len > 0, sys.env("ZILA_SURELY_UNSET_7") == nil)
    print(ai.schema(Item))
    match ai.key_from_env("ZILA_SURELY_UNSET_7")
        ok(k) -> print("unexpected")
        err(e) -> print(e.kind.name)
    end
    return
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!(out.lines().next(), Some("true true"));
    assert!(out.contains("\"description\":\"a name\"") && out.contains("\"required\":[\"name\",\"count\"]") && out.ends_with("std.InvalidInput
"), "{}", out);
    assert_eq!(code, 0);
}

#[test]
fn mcp_handles_messages_the_same_on_linux() {
    let src = r#"package main

use mcp
use ai
use json
use text

type Echo = record
    message: text
end

func main() -> error
    let s = mcp.server("demo", "1.0")
    s.tool(ai.tool(Echo, "echo", "Echoes", func(e)
        return "echo: " + e.message
    end))
    let session = s.new_session()
    print(s.handle(session, "not json") ?? "-")
    print(text.contains(s.handle(session, "\{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"\}") ?? "", "-32600"))
    let init = s.handle(session, "\{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\",\"params\":\{\"protocolVersion\":\"2025-06-18\"\}\}") ?? ""
    print(text.contains(init, "\"protocolVersion\":\"2025-06-18\""))
    let call = s.handle(session, "\{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":\{\"name\":\"echo\",\"arguments\":\{\"message\":\"hi\"\}\}\}") ?? ""
    print(text.contains(call, "echo: hi"), text.contains(call, "\"isError\":false"))
    return
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!(out.lines().next().map(|l| l.contains("-32700")), Some(true), "{}", out);
    assert!(out.ends_with("true
true
true true
"), "{}", out);
    assert_eq!(code, 0);
}

#[test]
fn rag_chunks_and_searches_the_same_on_linux() {
    let src = r#"package main

use rag
use text

func main() -> error
    let cs = try rag.chunk("Hello world. This is a test of chunking.\n\nSecond paragraph here with more words in it.", rag.ChunkOptions { by: rag.Size, max_chars: 30, overlap: 5 })
    for c in cs
        print(c.index, c.start, c.stop, c.text)
    end
    let st = rag.memory_store()
    var m: Map<text, text> = {}
    try st.add([rag.Item { id: "a", vector: [1.0, 0.0], text: "ta", meta: m }, rag.Item { id: "b", vector: [0.0, 1.0], text: "tb", meta: m }, rag.Item { id: "c", vector: [0.9, 0.1], text: "tc", meta: m }])
    let hits = try st.search([1.0, 0.0], 2, m)
    print(hits[0].id, hits[1].id)
    return
end
"#;
    let Some((out, code)) = run_linux(src) else { return };
    assert_eq!(out, "0 0 27 Hello world. This is a test\n1 23 40 test of chunking.\n2 42 68 Second paragraph here with\n3 64 86 with more words in it.\na c\n");
    assert_eq!(code, 0);
}

// ---- the limits that were lifted ----

#[test]
fn a_literal_divisor_needs_no_zero_check_and_a_variable_one_does() {
    let asm = |body: &str| {
        let file = parse_source(&main_of(body)).unwrap();
        generate_asm_for(&file, &CompileOptions::default(), Target::Linux).unwrap()
    };
    assert!(!asm("var x = 7\nprint(x / 3)\nprint(x % 4)").contains("jz rt_div_zero"));
    assert!(asm("var x = 7\nvar z = 0\nprint(x / z)").contains("jz rt_div_zero"));
    assert!(asm("var x = 7\nprint(x / 0)").contains("jz rt_div_zero"));
}

#[test]
fn a_callback_with_six_arguments_and_floats_gets_a_thunk_for_each_register() {
    let src = r#"package main
#[link("c")]
extern "C" func qsort(base: *int, n: usize, size: usize, cmp: extern "C" func(int, int, int, int, int, f64) -> f64)
func cb(a: int, b: int, c: int, d: int, e: int, f: f64) -> f64
    return f
end
func main()
    unsafe
        qsort(0, 0, 8, cb)
    end
end
"#;
    let file = parse_source(src).unwrap();
    let asm = generate_asm_for(&file, &CompileOptions::default(), Target::Linux).unwrap();
    let thunk = &asm[asm.find("CB_B_main_cb proc").unwrap()..asm.find("CB_B_main_cb endp").unwrap()];
    // five integers from rdi, rsi, rdx, rcx, r8; the float from xmm0, as bits; the result in xmm0
    for needle in ["rdi", "rsi", "rdx", "rcx", "r8", "movq rax, xmm0", "movq xmm0, rax", "[rsp + 32]"] {
        assert!(thunk.contains(needle), "the thunk lacks {}: {}", needle, thunk);
    }
}

/// The `pub` names a library file declares: functions, methods (receiver type included), types,
/// globals and externs.
fn pub_names(path: &str) -> std::collections::BTreeSet<String> {
    let src = std::fs::read_to_string(path).unwrap();
    let mut names = std::collections::BTreeSet::new();
    for line in src.lines() {
        let Some(rest) = line.strip_prefix("pub ") else { continue };
        let rest = rest.trim_start_matches("async ").trim_start_matches("comptime ");
        let rest = rest.strip_prefix("extern ").map(|r| r.trim_start_matches(|c: char| c != 'f')).unwrap_or(rest);
        let (kind, tail) = rest.split_once(' ').unwrap_or((rest, ""));
        let name: String = match kind {
            "func" if tail.starts_with('(') => {
                let close = tail.find(')').unwrap();
                let recv = tail[1..close].split(':').nth(1).unwrap_or("").trim().to_string();
                let after = tail[close + 1..].trim_start();
                format!("({}) {}", recv, after.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap())
            }
            "func" | "type" | "let" | "var" => tail.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap().to_string(),
            _ => continue,
        };
        names.insert(format!("{} {}", kind, name));
    }
    names
}

#[test]
fn a_library_and_its_linux_variant_export_the_same_names() {
    let mut checked = 0;
    // the layers of the core libraries are in std/, those of the other libraries in packages/<name>/linux/
    let mut layers: Vec<(String, String, String)> = Vec::new();
    for entry in std::fs::read_dir("std").unwrap() {
        let file = entry.unwrap().file_name().to_string_lossy().to_string();
        if let Some(base) = file.strip_suffix(".linux.zila") {
            layers.push((base.to_string(), format!("std/{}.zila", base), format!("std/{}.linux.zila", base)));
        }
    }
    for entry in std::fs::read_dir("packages").unwrap() {
        let path = entry.unwrap().path();
        let base = path.file_name().unwrap().to_string_lossy().to_string();
        if path.join("linux").is_dir() {
            layers.push((base.clone(), format!("packages/{}/{}.zila", base, base), format!("packages/{}/linux/{}.zila", base, base)));
        }
    }
    for (base, win_path, lin_path) in layers {
        if base == "sys" || base == "task" {
            continue; // the operating-system layer and the scheduler: each platform has its own calls
        }
        let win = pub_names(&win_path);
        let lin = pub_names(&lin_path);
        let only_win: Vec<_> = win.difference(&lin).collect();
        // a program written against the Windows variant must find every public name on Linux; the
        // Linux variants may export extra names (the resolver of `net` and the X protocol of `ui`
        // are written in Zila Lang, and tests reach into them)
        assert!(only_win.is_empty(), "{}: public in {}.zila but missing from {}.linux.zila: {:?}", base, base, base, only_win);
        checked += 1;
    }
    assert!(checked >= 4, "expected the Linux variants, found {}", checked);
}
