//! The debugger core: a program built with `--debug` is started under it, stopped at a
//! breakpoint, stepped, read and let run to its end. The programs only print.
#![cfg(windows)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use zila::codegen_x64::{compile_to_native_with, debug_info_path, CompileOptions};
use zila::debugger::{Debugger, Stop};
use zila::lsp::{parse_json, Json};
use zila::parse_source;

static N: AtomicUsize = AtomicUsize::new(0);

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!("zila_dbgr_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const PROGRAM: &str = "package main\n\nfunc add(a: int, b: int) -> int\n    let s = a + b\n    return s\nend\n\nfunc main()\n    let x = add(3, 4)\n    print(\"sum {x}\")\nend\n";

/// Builds `src` with debug information; the executable and the parsed `.zdbg`.
fn build(src: &str) -> (PathBuf, Json, PathBuf) {
    let dir = scratch();
    let exe = dir.join("p.exe");
    compile_to_native_with(&parse_source(src).unwrap(), exe.to_str().unwrap(), &CompileOptions { optimize: true, test_mode: false, debug: true }).unwrap();
    let info = parse_json(&std::fs::read_to_string(debug_info_path(exe.to_str().unwrap())).unwrap()).unwrap();
    (exe, info, dir)
}

fn num(j: &Json) -> f64 {
    match j {
        Json::Num(n) => *n,
        other => panic!("{:?}", other),
    }
}

/// The address (as built) of the first row of the line table for `line` of `main`.
fn address_of(info: &Json, line: u32) -> u64 {
    let Json::Arr(rows) = info.get("lines") else { panic!() };
    rows.iter()
        .find_map(|r| {
            let Json::Arr(v) = r else { return None };
            (v[1].str() == Some("main") && num(&v[2]) as u32 == line).then(|| num(&v[0]) as u64)
        })
        .unwrap_or_else(|| panic!("line {} has no address", line))
}

/// Starts the program, or `None` (with a note) when the system's virus scanner blocks it: the
/// executables of a compiler are sometimes taken for malware, which is not what these tests check.
fn launch(exe: &std::path::Path) -> Option<Debugger> {
    match Debugger::launch(exe, &[], None) {
        Ok(d) => Some(d),
        Err(e) if e.contains("225") => {
            eprintln!("skipped: {}", e);
            None
        }
        Err(e) => panic!("{}", e),
    }
}

fn wait_stop(d: &Debugger) -> Stop {
    d.next_stop(Duration::from_secs(20)).expect("the program did not stop in time")
}

fn all_output(d: &Debugger) -> String {
    let mut s = String::new();
    for _ in 0..40 {
        for o in d.take_output() {
            s.push_str(&String::from_utf8_lossy(&o.data));
        }
        if s.contains('\n') {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    s
}

#[test]
fn a_program_stops_at_a_breakpoint_steps_and_runs_to_its_end() {
    let (exe, info, dir) = build(PROGRAM);
    let Some(d) = launch(&exe) else { return };
    // the addresses of the file are for the image base it was built with; the system may move it
    let delta = d.base().wrapping_sub(num(info.get("base")) as u64);
    let at = |line| address_of(&info, line).wrapping_add(delta);

    // a breakpoint on `let s = a + b` (line 4), inside `add`
    let bp = at(4);
    d.set_breakpoint(bp).unwrap();
    d.resume();
    assert_eq!(wait_stop(&d), Stop::Breakpoint(bp));
    assert_eq!(d.regs().unwrap().rip, bp, "the program counter is back on the breakpoint");
    // the arguments of the call are the parameters of `add`: 3 and 4 in their slots (a at [rbp-8], b at [rbp-16])
    let regs = d.regs().unwrap();
    let a = u64::from_le_bytes(d.read_memory(regs.rbp - 8, 8).unwrap().try_into().unwrap());
    let b = u64::from_le_bytes(d.read_memory(regs.rbp - 16, 8).unwrap().try_into().unwrap());
    assert_eq!((a, b), (3, 4));
    // memory shows the program's own bytes, not the breakpoint instruction
    let shown = d.read_memory(bp, 1).unwrap()[0];
    assert_ne!(shown, 0xCC);

    // one instruction later the program counter has moved
    d.step();
    match wait_stop(&d) {
        Stop::Step(pc) => assert!(pc > bp && pc < bp + 64, "pc {:#x} after {:#x}", pc, bp),
        other => panic!("{:?}", other),
    }
    // the breakpoint is not hit again by running on; the program ends
    d.clear_breakpoint(bp).unwrap();
    d.resume();
    assert_eq!(wait_stop(&d), Stop::Exited(0));
    assert_eq!(all_output(&d).trim(), "sum 7");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_breakpoint_in_a_loop_is_hit_each_time_and_stepping_over_it_works() {
    let src = "package main\n\nfunc main()\n    var n = 0\n    for i in 0..3\n        n += i\n    end\n    print(\"n {n}\")\nend\n";
    let (exe, info, dir) = build(src);
    let Some(d) = launch(&exe) else { return };
    let delta = d.base().wrapping_sub(num(info.get("base")) as u64);
    let bp = address_of(&info, 6).wrapping_add(delta); // `n += i`
    d.set_breakpoint(bp).unwrap();
    for _ in 0..3 {
        d.resume();
        assert_eq!(wait_stop(&d), Stop::Breakpoint(bp));
    }
    d.resume();
    assert_eq!(wait_stop(&d), Stop::Exited(0));
    assert_eq!(all_output(&d).trim(), "n 3");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_running_program_can_be_paused_and_killed() {
    let src = "package main\n\nfunc main()\n    var n = 0\n    while true\n        n += 1\n    end\nend\n";
    let (exe, _info, dir) = build(src);
    let Some(d) = launch(&exe) else { return };
    d.resume();
    assert!(d.next_stop(Duration::from_millis(300)).is_none(), "an endless loop does not stop by itself");
    d.pause();
    assert_eq!(wait_stop(&d), Stop::Paused);
    assert!(d.regs().unwrap().rip != 0);
    d.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_is_reported_with_the_system_code() {
    // a null pointer read: access violation (0xC0000005)
    let src = "package main\n\nfunc main()\n    unsafe\n        let p: *int = 0\n        print(\"{p^}\")\n    end\nend\n";
    let (exe, _info, dir) = build(src);
    let Some(d) = launch(&exe) else { return };
    d.resume();
    match wait_stop(&d) {
        Stop::Fault { code, .. } => assert_eq!(code, 0xC000_0005),
        other => panic!("{:?}", other),
    }
    d.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_program_that_is_not_there_fails_to_launch_with_a_message() {
    let e = Debugger::launch(std::path::Path::new("C:\\no\\such\\program.exe"), &[], None).err().unwrap();
    assert!(e.contains("cannot start"), "{}", e);
}
