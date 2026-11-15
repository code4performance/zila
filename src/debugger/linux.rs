//! The Linux backend: `ptrace`, `waitpid` and `/proc/<pid>/mem`.
//!
//! The program is started with `PTRACE_TRACEME` and stops at its first instruction. Breakpoints
//! are written through `/proc/<pid>/mem`, registers come with `PTRACE_GETREGS`, a pause is a
//! `SIGSTOP`, and a signal that is not the debugger's (a crash) is passed on when the program is
//! resumed. The tracee is killed if the debugger ends (`PTRACE_O_EXITKILL`).

use super::{Backend, Output, Raw, Regs, R};
use std::ffi::c_void;
use std::io::Read;
use std::os::unix::fs::FileExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

extern "C" {
    fn ptrace(request: i32, ...) -> i64;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

const PTRACE_TRACEME: i32 = 0;
const PTRACE_CONT: i32 = 7;
const PTRACE_SINGLESTEP: i32 = 9;
const PTRACE_GETREGS: i32 = 12;
const PTRACE_SETREGS: i32 = 13;
const PTRACE_SETOPTIONS: i32 = 0x4200;
const PTRACE_O_EXITKILL: u64 = 0x0010_0000;

const WNOHANG: i32 = 1;
const WALL: i32 = 0x4000_0000;

const SIGKILL: i32 = 9;
const SIGTRAP: i32 = 5;
const SIGSTOP: i32 = 19;
/// Signals that end the program unless it handles them: reported as a crash.
const FATAL: [i32; 5] = [4, 6, 7, 8, 11];

/// The order of the registers in `struct user_regs_struct` (x86-64).
const FIELDS: usize = 27;
const R15: usize = 0;
const R14: usize = 1;
const R13: usize = 2;
const R12: usize = 3;
const RBP: usize = 4;
const RBX: usize = 5;
const R11: usize = 6;
const R10: usize = 7;
const R9: usize = 8;
const R8: usize = 9;
const RAX: usize = 10;
const RCX: usize = 11;
const RDX: usize = 12;
const RSI: usize = 13;
const RDI: usize = 14;
const RIP: usize = 16;
const EFLAGS: usize = 18;
const RSP: usize = 19;

/// Where a static executable of this compiler is loaded.
const LINUX_BASE: u64 = 0x400000;

struct Linux {
    child: Child,
    pid: i32,
    mem: std::fs::File,
    /// A signal to hand to the program when it is resumed (the one that stopped it, if it was a crash).
    signal: i32,
    /// The last resume was a single step.
    stepping: bool,
    exited: bool,
}

fn pump<T: Read + Send + 'static>(mut from: T, stderr: bool, out: Sender<Output>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match from.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out.send(Output { stderr, data: buf[..n].to_vec() }).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

pub(super) fn launch_backend(exe: &Path, args: &[String], cwd: Option<&Path>, out: Sender<Output>) -> R<Box<dyn Backend>> {
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    // the child asks to be traced before it runs the program: it stops at its first instruction
    unsafe {
        cmd.pre_exec(|| {
            ptrace(PTRACE_TRACEME, 0i32, std::ptr::null_mut::<c_void>(), std::ptr::null_mut::<c_void>());
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {}", exe.display(), e))?;
    let pid = child.id() as i32;
    if let Some(o) = child.stdout.take() {
        pump(o, false, out.clone());
    }
    if let Some(e) = child.stderr.take() {
        pump(e, true, out);
    }
    let mut status = 0i32;
    let r = unsafe { waitpid(pid, &mut status, WALL) };
    if r != pid || (status & 0xff) != 0x7f || ((status >> 8) & 0xff) != SIGTRAP {
        return Err(format!("the program did not stop at its start (status {:#x})", status));
    }
    unsafe {
        ptrace(PTRACE_SETOPTIONS, pid, std::ptr::null_mut::<c_void>(), PTRACE_O_EXITKILL as *mut c_void);
    }
    let mem = std::fs::OpenOptions::new().read(true).write(true).open(format!("/proc/{}/mem", pid)).map_err(|e| format!("cannot open the memory of the program: {}", e))?;
    Ok(Box::new(Linux { child, pid, mem, signal: 0, stepping: false, exited: false }))
}

impl Linux {
    fn get(&self) -> R<[u64; FIELDS]> {
        let mut regs = [0u64; FIELDS];
        let r = unsafe { ptrace(PTRACE_GETREGS, self.pid, std::ptr::null_mut::<c_void>(), regs.as_mut_ptr() as *mut c_void) };
        if r == -1 {
            return Err("cannot read the registers: the program is not stopped".into());
        }
        Ok(regs)
    }
}

impl Backend for Linux {
    fn base(&self) -> u64 {
        LINUX_BASE
    }

    fn read(&mut self, addr: u64, len: usize) -> R<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = self.mem.read_at(&mut buf, addr).map_err(|_| format!("cannot read {} bytes at {:#x}", len, addr))?;
        buf.truncate(n);
        Ok(buf)
    }

    fn write(&mut self, addr: u64, data: &[u8]) -> R<()> {
        self.mem.write_all_at(data, addr).map_err(|_| format!("cannot write at {:#x}", addr))
    }

    fn regs(&mut self) -> R<Regs> {
        let g = self.get()?;
        Ok(Regs {
            rip: g[RIP],
            rsp: g[RSP],
            rbp: g[RBP],
            rax: g[RAX],
            rbx: g[RBX],
            rcx: g[RCX],
            rdx: g[RDX],
            rsi: g[RSI],
            rdi: g[RDI],
            r8: g[R8],
            r9: g[R9],
            r10: g[R10],
            r11: g[R11],
            r12: g[R12],
            r13: g[R13],
            r14: g[R14],
            r15: g[R15],
            eflags: g[EFLAGS],
        })
    }

    fn set_regs(&mut self, r: &Regs) -> R<()> {
        let mut g = self.get()?;
        g[RIP] = r.rip;
        g[RSP] = r.rsp;
        g[RBP] = r.rbp;
        g[RAX] = r.rax;
        g[RBX] = r.rbx;
        g[RCX] = r.rcx;
        g[RDX] = r.rdx;
        g[RSI] = r.rsi;
        g[RDI] = r.rdi;
        g[R8] = r.r8;
        g[R9] = r.r9;
        g[R10] = r.r10;
        g[R11] = r.r11;
        g[R12] = r.r12;
        g[R13] = r.r13;
        g[R14] = r.r14;
        g[R15] = r.r15;
        g[EFLAGS] = r.eflags;
        let rc = unsafe { ptrace(PTRACE_SETREGS, self.pid, std::ptr::null_mut::<c_void>(), g.as_mut_ptr() as *mut c_void) };
        if rc == -1 {
            return Err("cannot set the registers".into());
        }
        Ok(())
    }

    fn resume(&mut self, step: bool) -> R<()> {
        let request = if step { PTRACE_SINGLESTEP } else { PTRACE_CONT };
        let signal = std::mem::take(&mut self.signal);
        self.stepping = step;
        let rc = unsafe { ptrace(request, self.pid, std::ptr::null_mut::<c_void>(), signal as usize as *mut c_void) };
        if rc == -1 {
            return Err("cannot resume the program".into());
        }
        Ok(())
    }

    fn wait(&mut self, ms: u32) -> R<Option<Raw>> {
        if self.exited {
            return Ok(None);
        }
        let until = Instant::now() + Duration::from_millis(ms as u64);
        loop {
            let mut status = 0i32;
            let r = unsafe { waitpid(self.pid, &mut status, WALL | WNOHANG) };
            if r == 0 {
                if Instant::now() >= until {
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            if r != self.pid {
                return Err("waiting for the program failed".into());
            }
            // ended with a status
            if status & 0x7f == 0 {
                self.exited = true;
                return Ok(Some(Raw::Exited((status >> 8) & 0xff)));
            }
            // ended by a signal
            if (status & 0xff) != 0x7f {
                self.exited = true;
                return Ok(Some(Raw::Exited(128 + (status & 0x7f))));
            }
            let sig = (status >> 8) & 0xff;
            if sig == SIGTRAP || sig == SIGSTOP {
                return Ok(Some(Raw::Trap));
            }
            if FATAL.contains(&sig) {
                // the program will end with it unless it handles it: report it first
                self.signal = sig;
                let addr = self.get().map(|g| g[RIP]).unwrap_or(0);
                return Ok(Some(Raw::Fault { code: sig as u32, addr }));
            }
            // another signal is the program's own business: hand it over and go on
            let request = if self.stepping { PTRACE_SINGLESTEP } else { PTRACE_CONT };
            unsafe {
                ptrace(request, self.pid, std::ptr::null_mut::<c_void>(), sig as usize as *mut c_void);
            }
        }
    }

    fn interrupt(&mut self) -> R<()> {
        if unsafe { kill(self.pid, SIGSTOP) } == -1 {
            return Err("cannot stop the program".into());
        }
        Ok(())
    }

    fn kill(&mut self) {
        if !self.exited {
            unsafe {
                kill(self.pid, SIGKILL);
            }
            let _ = self.child.wait();
            self.exited = true;
        }
    }
}
