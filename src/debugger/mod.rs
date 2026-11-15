//! The debugger core: starts a program under the operating system's debugging interface and lets
//! the caller set breakpoints, run, step one instruction and read the process.
//!
//! One worker thread owns the process (the debugging interfaces of both Windows and Linux require
//! that every wait and resume come from the thread that started it) and does the platform's part
//! through a `Backend`; this file does the rest, which is the same everywhere: breakpoints as
//! `int3` bytes that memory reads do not show, stepping over a breakpoint, and turning what the
//! platform reports into `Stop`s.
//!
//! Output of the program (standard output and error) comes through `Debugger::output`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::launch_backend;

#[cfg(unix)]
mod linux;
#[cfg(unix)]
use linux::launch_backend;

#[cfg(not(any(windows, unix)))]
fn launch_backend(_: &Path, _: &[String], _: Option<&Path>, _: Sender<Output>) -> Result<Box<dyn Backend>, String> {
    Err("debugging is not supported on this platform".into())
}

type R<T> = Result<T, String>;

/// The registers of the thread that stopped.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Regs {
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub eflags: u64,
}

/// Why the program stopped.
#[derive(Debug, Clone, PartialEq)]
pub enum Stop {
    /// A breakpoint at this address was reached (the program counter is back on it).
    Breakpoint(u64),
    /// One instruction was executed; the program counter is at this address.
    Step(u64),
    /// `pause` was asked for.
    Paused,
    /// The program raised an exception it does not handle (a crash); `code` is the system's code.
    Fault { code: u32, addr: u64 },
    /// The program ended with this status.
    Exited(i32),
}

/// A piece of the program's standard output or error.
#[derive(Debug, Clone)]
pub struct Output {
    pub stderr: bool,
    pub data: Vec<u8>,
}

/// What the platform reports when the program stops.
pub(crate) enum Raw {
    /// A breakpoint instruction or a single step finished.
    Trap,
    Exited(i32),
    Fault { code: u32, addr: u64 },
}

/// The platform's part of the debugger. All calls come from the worker thread.
pub(crate) trait Backend {
    /// The address the program's image is loaded at.
    fn base(&self) -> u64;
    fn read(&mut self, addr: u64, len: usize) -> R<Vec<u8>>;
    fn write(&mut self, addr: u64, data: &[u8]) -> R<()>;
    fn regs(&mut self) -> R<Regs>;
    fn set_regs(&mut self, regs: &Regs) -> R<()>;
    /// Lets the program run, for one instruction when `step`. Does not wait.
    fn resume(&mut self, step: bool) -> R<()>;
    /// Waits up to `ms` milliseconds for the program to stop.
    fn wait(&mut self, ms: u32) -> R<Option<Raw>>;
    /// Makes a running program stop (it is then reported by `wait`).
    fn interrupt(&mut self) -> R<()>;
    fn kill(&mut self);
}

enum Cmd {
    Read(u64, usize, Sender<R<Vec<u8>>>),
    Regs(Sender<R<Regs>>),
    SetRegs(Regs, Sender<R<()>>),
    SetBreakpoint(u64, Sender<R<()>>),
    ClearBreakpoint(u64, Sender<R<()>>),
    Resume { step: bool },
    Pause,
    Kill,
}

/// A program running under the debugger, stopped at its start until `resume` or `step`.
pub struct Debugger {
    cmd: Sender<Cmd>,
    events: Receiver<Stop>,
    output: Receiver<Output>,
    base: u64,
}

impl Debugger {
    /// Starts `exe` under the debugger, with its output captured. The program has not run any of
    /// its own code yet.
    pub fn launch(exe: &Path, args: &[String], cwd: Option<&Path>) -> R<Debugger> {
        let (cmd_tx, cmd_rx) = channel();
        let (ev_tx, ev_rx) = channel();
        let (out_tx, out_rx) = channel();
        let (ready_tx, ready_rx) = channel::<R<u64>>();
        let (exe, args, cwd) = (exe.to_path_buf(), args.to_vec(), cwd.map(|c| c.to_path_buf()));
        std::thread::spawn(move || match launch_backend(&exe, &args, cwd.as_deref(), out_tx) {
            Ok(be) => {
                let _ = ready_tx.send(Ok(be.base()));
                work(be, cmd_rx, ev_tx);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        });
        let base = ready_rx.recv().map_err(|_| "the debugger thread stopped".to_string())??;
        Ok(Debugger { cmd: cmd_tx, events: ev_rx, output: out_rx, base })
    }

    /// The address the image was loaded at.
    pub fn base(&self) -> u64 {
        self.base
    }

    fn ask<T>(&self, make: impl FnOnce(Sender<R<T>>) -> Cmd) -> R<T> {
        let (tx, rx) = channel();
        self.cmd.send(make(tx)).map_err(|_| "the program has ended".to_string())?;
        rx.recv().map_err(|_| "the program has ended".to_string())?
    }

    pub fn read_memory(&self, addr: u64, len: usize) -> R<Vec<u8>> {
        self.ask(|tx| Cmd::Read(addr, len, tx))
    }

    pub fn regs(&self) -> R<Regs> {
        self.ask(Cmd::Regs)
    }

    pub fn set_regs(&self, regs: Regs) -> R<()> {
        self.ask(|tx| Cmd::SetRegs(regs, tx))
    }

    /// Puts a breakpoint at `addr`; the program must be stopped.
    pub fn set_breakpoint(&self, addr: u64) -> R<()> {
        self.ask(|tx| Cmd::SetBreakpoint(addr, tx))
    }

    pub fn clear_breakpoint(&self, addr: u64) -> R<()> {
        self.ask(|tx| Cmd::ClearBreakpoint(addr, tx))
    }

    /// Runs until the next `Stop`.
    pub fn resume(&self) {
        let _ = self.cmd.send(Cmd::Resume { step: false });
    }

    /// Executes one instruction and stops.
    pub fn step(&self) {
        let _ = self.cmd.send(Cmd::Resume { step: true });
    }

    /// Stops a running program.
    pub fn pause(&self) {
        let _ = self.cmd.send(Cmd::Pause);
    }

    /// Ends the program.
    pub fn kill(&self) {
        let _ = self.cmd.send(Cmd::Kill);
    }

    /// The next stop, waiting at most `wait`.
    pub fn next_stop(&self, wait: Duration) -> Option<Stop> {
        self.events.recv_timeout(wait).ok()
    }

    /// What the program has written to its standard output and error so far.
    pub fn take_output(&self) -> Vec<Output> {
        self.output.try_iter().collect()
    }
}

impl Drop for Debugger {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Kill);
    }
}

const INT3: u8 = 0xCC;

/// The worker: owns the backend, the breakpoints and whether the program runs.
fn work(mut be: Box<dyn Backend>, cmds: Receiver<Cmd>, events: Sender<Stop>) {
    let mut bps: HashMap<u64, u8> = HashMap::new();
    let mut running = false;
    let mut stepping = false;
    loop {
        let cmd = if running {
            cmds.try_recv().ok()
        } else {
            match cmds.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            }
        };
        if let Some(c) = cmd {
            match c {
                Cmd::Read(addr, len, reply) => {
                    let mut data = be.read(addr, len);
                    // the program's own bytes, not the breakpoint instructions
                    if let Ok(d) = data.as_mut() {
                        for (a, orig) in &bps {
                            if *a >= addr && ((*a - addr) as usize) < d.len() {
                                d[(*a - addr) as usize] = *orig;
                            }
                        }
                    }
                    let _ = reply.send(data);
                }
                Cmd::Regs(reply) => {
                    let _ = reply.send(be.regs());
                }
                Cmd::SetRegs(r, reply) => {
                    let _ = reply.send(be.set_regs(&r));
                }
                Cmd::SetBreakpoint(addr, reply) => {
                    let r = match bps.entry(addr) {
                        std::collections::hash_map::Entry::Occupied(_) => Ok(()),
                        std::collections::hash_map::Entry::Vacant(slot) => be.read(addr, 1).and_then(|orig| {
                            be.write(addr, &[INT3])?;
                            slot.insert(orig[0]);
                            Ok(())
                        }),
                    };
                    let _ = reply.send(r);
                }
                Cmd::ClearBreakpoint(addr, reply) => {
                    let r = match bps.remove(&addr) {
                        Some(orig) => be.write(addr, &[orig]),
                        None => Ok(()),
                    };
                    let _ = reply.send(r);
                }
                Cmd::Resume { step } => {
                    if running {
                        continue;
                    }
                    match start(&mut *be, &bps, step) {
                        Ok(Started::Running) => {
                            running = true;
                            stepping = step;
                        }
                        Ok(Started::Stopped(stop)) => {
                            let ended = matches!(stop, Stop::Exited(_));
                            let _ = events.send(stop);
                            if ended {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = events.send(Stop::Fault { code: 0, addr: 0 });
                            eprintln!("debugger: {}", e);
                            break;
                        }
                    }
                }
                Cmd::Pause => {
                    if running {
                        let _ = be.interrupt();
                    }
                }
                Cmd::Kill => break,
            }
            continue;
        }
        if running {
            match be.wait(20) {
                Ok(Some(raw)) => {
                    running = false;
                    let stop = convert(&mut *be, &bps, raw, stepping);
                    let ended = matches!(stop, Stop::Exited(_));
                    let _ = events.send(stop);
                    if ended {
                        return;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("debugger: {}", e);
                    break;
                }
            }
        }
    }
    be.kill();
}

enum Started {
    Running,
    Stopped(Stop),
}

/// Lets the program go from where it stopped. At a breakpoint the original instruction is put
/// back, executed alone, and the breakpoint is set again, so that it is not hit at once.
fn start(be: &mut dyn Backend, bps: &HashMap<u64, u8>, step: bool) -> R<Started> {
    let pc = be.regs()?.rip;
    if let Some(&orig) = bps.get(&pc) {
        be.write(pc, &[orig])?;
        be.resume(true)?;
        let raw = loop {
            if let Some(r) = be.wait(1000)? {
                break r;
            }
        };
        if matches!(raw, Raw::Trap) {
            be.write(pc, &[INT3])?;
        }
        match raw {
            Raw::Trap if step => return Ok(Started::Stopped(Stop::Step(be.regs()?.rip))),
            Raw::Trap => {}
            other => return Ok(Started::Stopped(convert(be, bps, other, true))),
        }
    }
    be.resume(step)?;
    Ok(Started::Running)
}

/// What a stop of the platform means here.
fn convert(be: &mut dyn Backend, bps: &HashMap<u64, u8>, raw: Raw, stepping: bool) -> Stop {
    match raw {
        Raw::Exited(code) => Stop::Exited(code),
        Raw::Fault { code, addr } => Stop::Fault { code, addr },
        Raw::Trap => {
            let Ok(mut regs) = be.regs() else { return Stop::Paused };
            if stepping {
                return Stop::Step(regs.rip);
            }
            if bps.contains_key(&regs.rip.wrapping_sub(1)) {
                regs.rip -= 1;
                let at = regs.rip;
                let _ = be.set_regs(&regs);
                return Stop::Breakpoint(at);
            }
            Stop::Paused
        }
    }
}
