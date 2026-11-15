//! The Windows backend: `CreateProcess` with `DEBUG_ONLY_THIS_PROCESS`, `WaitForDebugEvent`,
//! `ContinueDebugEvent`, thread contexts and `ReadProcessMemory`/`WriteProcessMemory`.

use super::{Backend, Output, Raw, Regs, R};
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::mpsc::Sender;

type Handle = *mut c_void;

#[repr(C)]
struct Startup {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_chars: u32,
    y_chars: u32,
    fill: u32,
    flags: u32,
    show: u16,
    reserved2_size: u16,
    reserved2: *mut u8,
    std_in: Handle,
    std_out: Handle,
    std_err: Handle,
}

#[repr(C)]
struct ProcessInfo {
    process: Handle,
    thread: Handle,
    pid: u32,
    tid: u32,
}

#[repr(C)]
struct Security {
    length: u32,
    descriptor: *mut c_void,
    inherit: i32,
}

/// `CONTEXT` of x64: 1232 bytes, 16-aligned. Only the fields below are used.
#[repr(C, align(16))]
struct Context([u8; 1232]);

/// `DEBUG_EVENT`: a header and a union of up to 160 bytes.
#[repr(C, align(8))]
struct DebugEvent([u8; 192]);

#[link(name = "kernel32")]
extern "system" {
    fn CreateProcessW(app: *const u16, cmd: *mut u16, pa: *const Security, ta: *const Security, inherit: i32, flags: u32, env: *const c_void, cwd: *const u16, startup: *mut Startup, info: *mut ProcessInfo) -> i32;
    fn CreatePipe(read: *mut Handle, write: *mut Handle, attrs: *const Security, size: u32) -> i32;
    fn SetHandleInformation(h: Handle, mask: u32, flags: u32) -> i32;
    fn ReadFile(h: Handle, buf: *mut u8, n: u32, read: *mut u32, overlapped: *mut c_void) -> i32;
    fn CloseHandle(h: Handle) -> i32;
    fn WaitForDebugEvent(ev: *mut DebugEvent, ms: u32) -> i32;
    fn ContinueDebugEvent(pid: u32, tid: u32, status: u32) -> i32;
    fn TerminateProcess(h: Handle, code: u32) -> i32;
    fn DebugBreakProcess(h: Handle) -> i32;
    fn ReadProcessMemory(h: Handle, addr: *const c_void, buf: *mut u8, n: usize, read: *mut usize) -> i32;
    fn WriteProcessMemory(h: Handle, addr: *mut c_void, buf: *const u8, n: usize, written: *mut usize) -> i32;
    fn FlushInstructionCache(h: Handle, addr: *const c_void, n: usize) -> i32;
    fn GetThreadContext(h: Handle, ctx: *mut Context) -> i32;
    fn SetThreadContext(h: Handle, ctx: *const Context) -> i32;
    fn GetLastError() -> u32;
}

const DEBUG_ONLY_THIS_PROCESS: u32 = 0x2;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const STARTF_USESTDHANDLES: u32 = 0x100;
const HANDLE_FLAG_INHERIT: u32 = 1;
const DBG_CONTINUE: u32 = 0x0001_0002;
const DBG_EXCEPTION_NOT_HANDLED: u32 = 0x8001_0001;
const STATUS_BREAKPOINT: u32 = 0x8000_0003;
const STATUS_SINGLE_STEP: u32 = 0x8000_0004;
const CONTEXT_CONTROL_INTEGER: u32 = 0x0010_0003;
const TRAP_FLAG: u64 = 0x100;

const EXCEPTION_EVENT: u32 = 1;
const CREATE_THREAD_EVENT: u32 = 2;
const CREATE_PROCESS_EVENT: u32 = 3;
const EXIT_THREAD_EVENT: u32 = 4;
const EXIT_PROCESS_EVENT: u32 = 5;
const LOAD_DLL_EVENT: u32 = 6;

fn last_error(what: &str) -> String {
    let code = unsafe { GetLastError() };
    if code == 225 {
        // the system refuses to run a file its virus scanner has flagged
        return format!("{} failed (Windows error 225: the system's virus scanner blocked the program; allow it or exclude the build folder)", what);
    }
    format!("{} failed (Windows error {})", what, code)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// The registers' places in `CONTEXT`.
const OFFSETS: [(usize, &str); 17] = [
    (0x78, "rax"),
    (0x80, "rcx"),
    (0x88, "rdx"),
    (0x90, "rbx"),
    (0x98, "rsp"),
    (0xA0, "rbp"),
    (0xA8, "rsi"),
    (0xB0, "rdi"),
    (0xB8, "r8"),
    (0xC0, "r9"),
    (0xC8, "r10"),
    (0xD0, "r11"),
    (0xD8, "r12"),
    (0xE0, "r13"),
    (0xE8, "r14"),
    (0xF0, "r15"),
    (0xF8, "rip"),
];
const EFLAGS_AT: usize = 0x44;
const FLAGS_AT: usize = 0x30;

struct Windows {
    process: Handle,
    base: u64,
    pid: u32,
    threads: HashMap<u32, Handle>,
    /// The thread of the last event, which is stopped: its registers are the ones asked for.
    current: u32,
    /// An event waits to be continued (`ContinueDebugEvent` is owed).
    pending: Option<(u32, u32)>,
    /// The exception that stopped the program was a crash: continuing passes it on.
    fault: bool,
    /// The process has ended.
    exited: bool,
}

// Handles are plain numbers to the system; the backend is only used by the worker thread.
unsafe impl Send for Windows {}

/// A pipe whose read end stays with us.
fn pipe() -> R<(Handle, Handle)> {
    let attrs = Security { length: std::mem::size_of::<Security>() as u32, descriptor: std::ptr::null_mut(), inherit: 1 };
    let (mut r, mut w): (Handle, Handle) = (std::ptr::null_mut(), std::ptr::null_mut());
    if unsafe { CreatePipe(&mut r, &mut w, &attrs, 0) } == 0 {
        return Err(last_error("CreatePipe"));
    }
    unsafe { SetHandleInformation(r, HANDLE_FLAG_INHERIT, 0) };
    Ok((r, w))
}

struct SendHandle(Handle);
unsafe impl Send for SendHandle {}

fn pump(read: Handle, stderr: bool, out: Sender<Output>) {
    let h = SendHandle(read);
    std::thread::spawn(move || {
        let h = h;
        let mut buf = [0u8; 4096];
        loop {
            let mut n = 0u32;
            let ok = unsafe { ReadFile(h.0, buf.as_mut_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut()) };
            if ok == 0 || n == 0 {
                break;
            }
            if out.send(Output { stderr, data: buf[..n as usize].to_vec() }).is_err() {
                break;
            }
        }
        unsafe { CloseHandle(h.0) };
    });
}

/// Quotes one argument of a command line.
fn quote(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) {
        return a.to_string();
    }
    format!("\"{}\"", a.replace('"', "\\\""))
}

pub(super) fn launch_backend(exe: &Path, args: &[String], cwd: Option<&Path>, out: Sender<Output>) -> R<Box<dyn Backend>> {
    let (out_r, out_w) = pipe()?;
    let (err_r, err_w) = pipe()?;
    let (in_r, in_w) = pipe()?;
    // the program reads end-of-file from its standard input: the debugger's own input is the protocol
    unsafe { CloseHandle(in_w) };
    let mut startup = Startup {
        cb: std::mem::size_of::<Startup>() as u32,
        reserved: std::ptr::null_mut(),
        desktop: std::ptr::null_mut(),
        title: std::ptr::null_mut(),
        x: 0,
        y: 0,
        x_size: 0,
        y_size: 0,
        x_chars: 0,
        y_chars: 0,
        fill: 0,
        flags: STARTF_USESTDHANDLES,
        show: 0,
        reserved2_size: 0,
        reserved2: std::ptr::null_mut(),
        std_in: in_r,
        std_out: out_w,
        std_err: err_w,
    };
    let mut info = ProcessInfo { process: std::ptr::null_mut(), thread: std::ptr::null_mut(), pid: 0, tid: 0 };
    let mut cmd: Vec<u16> = std::iter::once(quote(&exe.to_string_lossy())).chain(args.iter().map(|a| quote(a))).collect::<Vec<_>>().join(" ").encode_utf16().chain(std::iter::once(0)).collect();
    let cwd_w: Option<Vec<u16>> = cwd.map(|c| c.as_os_str().encode_wide().chain(std::iter::once(0)).collect());
    // a program the compiler has just written may be held for a moment by a virus scanner
    // (error 225): try again a few times
    let mut attempt = 0;
    let ok = loop {
        let ok = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            DEBUG_ONLY_THIS_PROCESS | CREATE_NO_WINDOW,
            std::ptr::null(),
            cwd_w.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            &mut startup,
            &mut info,
        )
        };
        attempt += 1;
        if ok != 0 || attempt >= 6 || unsafe { GetLastError() } != 225 {
            break ok;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    };
    // the child has its own copies of these now
    unsafe {
        CloseHandle(out_w);
        CloseHandle(err_w);
        CloseHandle(in_r);
    }
    if ok == 0 {
        let e = last_error("CreateProcess");
        unsafe {
            CloseHandle(out_r);
            CloseHandle(err_r);
        }
        return Err(format!("cannot start {}: {}", exe.display(), e));
    }
    pump(out_r, false, out.clone());
    pump(err_r, true, out);
    unsafe { CloseHandle(info.thread) };
    let mut w = Windows { process: info.process, base: 0, pid: info.pid, threads: HashMap::new(), current: 0, pending: None, fault: false, exited: false };
    // run to the system's first breakpoint, after the image is loaded and before its code runs
    match w.wait(10_000)? {
        Some(Raw::Trap) => {}
        Some(Raw::Exited(c)) => return Err(format!("the program ended at once (status {})", c)),
        Some(Raw::Fault { code, .. }) => return Err(format!("the program crashed at start (exception {:#x})", code)),
        None => return Err("the program did not start".into()),
    }
    Ok(Box::new(w))
}

impl Windows {
    fn thread(&self) -> R<Handle> {
        self.threads.get(&self.current).copied().ok_or_else(|| "no thread is stopped".to_string())
    }

    fn context(&self) -> R<Context> {
        let mut ctx = Context([0; 1232]);
        ctx.0[FLAGS_AT..FLAGS_AT + 4].copy_from_slice(&CONTEXT_CONTROL_INTEGER.to_le_bytes());
        if unsafe { GetThreadContext(self.thread()?, &mut ctx) } == 0 {
            return Err(last_error("GetThreadContext"));
        }
        Ok(ctx)
    }

    fn set_context(&self, ctx: &Context) -> R<()> {
        if unsafe { SetThreadContext(self.thread()?, ctx) } == 0 {
            return Err(last_error("SetThreadContext"));
        }
        Ok(())
    }

    /// Hands the stopped program back to the system.
    fn release(&mut self, status: u32) -> R<()> {
        if let Some((pid, tid)) = self.pending.take() {
            if unsafe { ContinueDebugEvent(pid, tid, status) } == 0 {
                return Err(last_error("ContinueDebugEvent"));
            }
        }
        Ok(())
    }
}

impl Backend for Windows {
    fn base(&self) -> u64 {
        self.base
    }

    fn read(&mut self, addr: u64, len: usize) -> R<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let mut got = 0usize;
        if unsafe { ReadProcessMemory(self.process, addr as *const c_void, buf.as_mut_ptr(), len, &mut got) } == 0 {
            return Err(format!("cannot read {} bytes at {:#x}", len, addr));
        }
        buf.truncate(got);
        Ok(buf)
    }

    fn write(&mut self, addr: u64, data: &[u8]) -> R<()> {
        let mut put = 0usize;
        if unsafe { WriteProcessMemory(self.process, addr as *mut c_void, data.as_ptr(), data.len(), &mut put) } == 0 {
            return Err(format!("cannot write at {:#x}", addr));
        }
        unsafe { FlushInstructionCache(self.process, addr as *const c_void, data.len()) };
        Ok(())
    }

    fn regs(&mut self) -> R<Regs> {
        let ctx = self.context()?;
        let mut r = Regs { eflags: u32_at(&ctx.0, EFLAGS_AT) as u64, ..Regs::default() };
        for (at, name) in OFFSETS {
            let v = u64_at(&ctx.0, at);
            match name {
                "rax" => r.rax = v,
                "rcx" => r.rcx = v,
                "rdx" => r.rdx = v,
                "rbx" => r.rbx = v,
                "rsp" => r.rsp = v,
                "rbp" => r.rbp = v,
                "rsi" => r.rsi = v,
                "rdi" => r.rdi = v,
                "r8" => r.r8 = v,
                "r9" => r.r9 = v,
                "r10" => r.r10 = v,
                "r11" => r.r11 = v,
                "r12" => r.r12 = v,
                "r13" => r.r13 = v,
                "r14" => r.r14 = v,
                "r15" => r.r15 = v,
                _ => r.rip = v,
            }
        }
        Ok(r)
    }

    fn set_regs(&mut self, r: &Regs) -> R<()> {
        let mut ctx = self.context()?;
        for (at, name) in OFFSETS {
            let v = match name {
                "rax" => r.rax,
                "rcx" => r.rcx,
                "rdx" => r.rdx,
                "rbx" => r.rbx,
                "rsp" => r.rsp,
                "rbp" => r.rbp,
                "rsi" => r.rsi,
                "rdi" => r.rdi,
                "r8" => r.r8,
                "r9" => r.r9,
                "r10" => r.r10,
                "r11" => r.r11,
                "r12" => r.r12,
                "r13" => r.r13,
                "r14" => r.r14,
                "r15" => r.r15,
                _ => r.rip,
            };
            ctx.0[at..at + 8].copy_from_slice(&v.to_le_bytes());
        }
        ctx.0[EFLAGS_AT..EFLAGS_AT + 4].copy_from_slice(&(r.eflags as u32).to_le_bytes());
        self.set_context(&ctx)
    }

    fn resume(&mut self, step: bool) -> R<()> {
        let mut ctx = self.context()?;
        let flags = u32_at(&ctx.0, EFLAGS_AT) as u64;
        let new = if step { flags | TRAP_FLAG } else { flags & !TRAP_FLAG };
        ctx.0[EFLAGS_AT..EFLAGS_AT + 4].copy_from_slice(&(new as u32).to_le_bytes());
        self.set_context(&ctx)?;
        // a crash that stopped the program is passed on, so that it ends as it would have
        let status = if self.fault { DBG_EXCEPTION_NOT_HANDLED } else { DBG_CONTINUE };
        self.fault = false;
        self.release(status)
    }

    fn wait(&mut self, ms: u32) -> R<Option<Raw>> {
        loop {
            if self.exited {
                return Ok(None);
            }
            let mut ev = DebugEvent([0; 192]);
            if unsafe { WaitForDebugEvent(&mut ev, ms) } == 0 {
                return Ok(None);
            }
            let (code, pid, tid) = (u32_at(&ev.0, 0), u32_at(&ev.0, 4), u32_at(&ev.0, 8));
            let u = &ev.0[16..];
            match code {
                CREATE_PROCESS_EVENT => {
                    self.base = u64_at(u, 24);
                    self.threads.insert(tid, u64_at(u, 16) as Handle);
                    // the file handle the system gives is ours to close
                    let file = u64_at(u, 0) as Handle;
                    if !file.is_null() {
                        unsafe { CloseHandle(file) };
                    }
                    self.current = tid;
                    unsafe { ContinueDebugEvent(pid, tid, DBG_CONTINUE) };
                }
                CREATE_THREAD_EVENT => {
                    self.threads.insert(tid, u64_at(u, 0) as Handle);
                    unsafe { ContinueDebugEvent(pid, tid, DBG_CONTINUE) };
                }
                EXIT_THREAD_EVENT => {
                    self.threads.remove(&tid);
                    unsafe { ContinueDebugEvent(pid, tid, DBG_CONTINUE) };
                }
                LOAD_DLL_EVENT => {
                    let file = u64_at(u, 0) as Handle;
                    if !file.is_null() {
                        unsafe { CloseHandle(file) };
                    }
                    unsafe { ContinueDebugEvent(pid, tid, DBG_CONTINUE) };
                }
                EXIT_PROCESS_EVENT => {
                    let status = u32_at(u, 0) as i32;
                    unsafe { ContinueDebugEvent(pid, tid, DBG_CONTINUE) };
                    self.exited = true;
                    return Ok(Some(Raw::Exited(status)));
                }
                EXCEPTION_EVENT => {
                    let (ex, addr, first_chance) = (u32_at(u, 0), u64_at(u, 16), u32_at(u, 152));
                    self.current = tid;
                    self.pending = Some((pid, tid));
                    return Ok(Some(match ex {
                        STATUS_BREAKPOINT | STATUS_SINGLE_STEP => Raw::Trap,
                        _ => {
                            // the program will crash unless the exception is handled; report it first
                            let _ = first_chance;
                            self.fault = true;
                            Raw::Fault { code: ex, addr }
                        }
                    }));
                }
                _ => {
                    // output strings, unloaded libraries: nothing to do
                    unsafe { ContinueDebugEvent(pid, tid, DBG_CONTINUE) };
                }
            }
        }
    }

    fn interrupt(&mut self) -> R<()> {
        if unsafe { DebugBreakProcess(self.process) } == 0 {
            return Err(last_error("DebugBreakProcess"));
        }
        Ok(())
    }

    fn kill(&mut self) {
        if !self.exited {
            unsafe {
                TerminateProcess(self.process, 1);
            }
            // let the system report the end, so that nothing is left waiting
            if self.pending.is_some() {
                let _ = self.release(DBG_CONTINUE);
            }
            for _ in 0..50 {
                if matches!(self.wait(100), Ok(Some(Raw::Exited(_)))) {
                    break;
                }
            }
        }
        unsafe { CloseHandle(self.process) };
        let _ = self.pid;
    }
}
