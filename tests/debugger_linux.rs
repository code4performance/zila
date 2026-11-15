//! The debugger core on Linux (`ptrace`): the same checks as `tests/debugger.rs`. A program built
//! with `--debug` is started under it, stopped at a breakpoint, stepped, read and let run to its
//! end. The programs only print. Runs only on a Linux host.
#![cfg(unix)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use zila::codegen_x64::{compile_to_linux, debug_info_path, CompileOptions};
use zila::debugger::{Debugger, Stop};
use zila::lsp::{parse_json, Json};
use zila::parse_source;

static N: AtomicUsize = AtomicUsize::new(0);

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!("zila_dbgl_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const PROGRAM: &str = "package main\n\nfunc add(a: int, b: int) -> int\n    let s = a + b\n    return s\nend\n\nfunc main()\n    let x = add(3, 4)\n    print(\"sum {x}\")\nend\n";

fn build(src: &str) -> (PathBuf, Json, PathBuf) {
    let dir = scratch();
    let exe = dir.join("p");
    compile_to_linux(&parse_source(src).unwrap(), exe.to_str().unwrap(), &CompileOptions { optimize: true, test_mode: false, debug: true }).unwrap();
    let info = parse_json(&std::fs::read_to_string(debug_info_path(exe.to_str().unwrap())).unwrap()).unwrap();
    (exe, info, dir)
}

fn num(j: &Json) -> f64 {
    match j {
        Json::Num(n) => *n,
        other => panic!("{:?}", other),
    }
}

fn address_of(info: &Json, line: u32) -> u64 {
    let Json::Arr(rows) = info.get("lines") else { panic!() };
    rows.iter()
        .find_map(|r| {
            let Json::Arr(v) = r else { return None };
            (v[1].str() == Some("main") && num(&v[2]) as u32 == line).then(|| num(&v[0]) as u64)
        })
        .unwrap_or_else(|| panic!("line {} has no address", line))
}

fn wait_stop(d: &Debugger) -> Stop {
    d.next_stop(Duration::from_secs(20)).expect("the program did not stop in time")
}

#[test]
fn a_program_stops_at_a_breakpoint_steps_and_runs_to_its_end() {
    let (exe, info, dir) = build(PROGRAM);
    let d = Debugger::launch(&exe, &[], None).unwrap();
    let delta = d.base().wrapping_sub(num(info.get("base")) as u64);
    let bp = address_of(&info, 4).wrapping_add(delta);
    d.set_breakpoint(bp).unwrap();
    d.resume();
    assert_eq!(wait_stop(&d), Stop::Breakpoint(bp));
    let regs = d.regs().unwrap();
    assert_eq!(regs.rip, bp);
    let a = u64::from_le_bytes(d.read_memory(regs.rbp - 8, 8).unwrap().try_into().unwrap());
    let b = u64::from_le_bytes(d.read_memory(regs.rbp - 16, 8).unwrap().try_into().unwrap());
    assert_eq!((a, b), (3, 4));
    assert_ne!(d.read_memory(bp, 1).unwrap()[0], 0xCC);
    d.step();
    match wait_stop(&d) {
        Stop::Step(pc) => assert!(pc > bp && pc < bp + 64),
        other => panic!("{:?}", other),
    }
    d.clear_breakpoint(bp).unwrap();
    d.resume();
    assert_eq!(wait_stop(&d), Stop::Exited(0));
    let mut out = String::new();
    for _ in 0..40 {
        for o in d.take_output() {
            out.push_str(&String::from_utf8_lossy(&o.data));
        }
        if out.contains('\n') {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(out.trim(), "sum 7");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_running_program_can_be_paused_and_killed() {
    let (exe, _info, dir) = build("package main\n\nfunc main()\n    var n = 0\n    while true\n        n += 1\n    end\nend\n");
    let d = Debugger::launch(&exe, &[], None).unwrap();
    d.resume();
    assert!(d.next_stop(Duration::from_millis(300)).is_none());
    d.pause();
    assert_eq!(wait_stop(&d), Stop::Paused);
    d.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_is_reported_with_the_signal() {
    let (exe, _info, dir) = build("package main\n\nfunc main()\n    unsafe\n        let p: *int = 0\n        print(\"{p^}\")\n    end\nend\n");
    let d = Debugger::launch(&exe, &[], None).unwrap();
    d.resume();
    match wait_stop(&d) {
        Stop::Fault { code, .. } => assert_eq!(code, 11),
        other => panic!("{:?}", other),
    }
    d.kill();
    let _ = std::fs::remove_dir_all(&dir);
}
