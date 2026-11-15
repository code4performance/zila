//! The debug adapter (`zila --dap`): the Debug Adapter Protocol over standard input and output.
//!
//! A `launch` request names a `.zila` program: the adapter builds it with debug information,
//! starts it under the debugger (`crate::debugger`) and answers the requests of the client with the
//! line table of the debug-information file: line breakpoints, continue, pause, the three steps,
//! and the call stack with source positions. The program's output comes back as `output` events.
//!
//! Stepping is a state machine driven by the stops of the debugger: a step executes instructions
//! until another statement starts; a call is run to its return address (or, to step in, to the
//! first statement of the callee); a step out runs to the return address of the frame.

mod info;
mod values;

use crate::debugger::{Debugger, Stop};
use crate::lsp::{read_message, send, Json};
use info::Info;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

fn obj(v: Vec<(&str, Json)>) -> Json {
    Json::Obj(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn num(n: impl Into<f64>) -> Json {
    Json::Num(n.into())
}

fn s(x: &str) -> Json {
    Json::Str(x.to_string())
}

/// The host's target name, as `--target` spells it.
fn host() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// A frame of the call stack, taken when the program stopped.
#[derive(Debug, Clone)]
pub(crate) struct Frame {
    name: String,
    module: String,
    line: u32,
    /// The frame pointer (`rbp`) of the function.
    fp: u64,
    /// The index of the function in the debug information.
    func: usize,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Over,
    Into,
    Out,
}

#[derive(Debug, Clone)]
enum Phase {
    /// Executing instructions one at a time.
    Stepping,
    /// Running to the return address of a call; `guard` is the stack address of that return address:
    /// a stop with the stack not above it is a deeper activation of the same function.
    RunToReturn { guard: u64 },
    /// Running to the first statement of a function that was stepped into.
    Enter,
    /// Running to the return address of the frame (step out).
    RunOut { guard: u64 },
}

#[derive(Debug, Clone)]
struct Step {
    kind: Kind,
    /// The function being stepped in (runtime addresses).
    start: u64,
    end: u64,
    prev_rsp: u64,
    phase: Phase,
    /// The temporary breakpoint the step set, if any.
    temp: Option<u64>,
}

struct Session {
    seq: i64,
    program: PathBuf,
    info: Option<Info>,
    dbg: Option<Debugger>,
    /// Where the image was loaded minus where it was built for.
    delta: u64,
    running: bool,
    step: Option<Step>,
    /// The user's breakpoints: source -> addresses set for it.
    by_source: HashMap<String, Vec<u64>>,
    user_bps: HashSet<u64>,
    /// Breakpoints asked for before the program was started.
    early: Vec<(PathBuf, Vec<u32>, i64)>,
    frames: Vec<Frame>,
    stop_on_entry: bool,
    configured: bool,
    exe: Option<PathBuf>,
    values: values::Handles,
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Whether the file starts like an executable of the other operating system.
fn foreign_executable(path: &Path) -> Option<&'static str> {
    let mut head = [0u8; 4];
    let mut f = std::fs::File::open(path).ok()?;
    std::io::Read::read_exact(&mut f, &mut head).ok()?;
    match (&head[..4], host()) {
        ([0x7F, b'E', b'L', b'F'], "windows") => Some("a Linux program"),
        ([b'M', b'Z', ..], "linux") => Some("a Windows program"),
        _ => None,
    }
}

impl Session {
    fn new() -> Session {
        Session {
            seq: 0,
            program: PathBuf::new(),
            info: None,
            dbg: None,
            delta: 0,
            running: false,
            step: None,
            by_source: HashMap::new(),
            user_bps: HashSet::new(),
            early: Vec::new(),
            frames: Vec::new(),
            stop_on_entry: false,
            configured: false,
            exe: None,
            values: values::Handles::default(),
        }
    }

    fn next_seq(&mut self) -> i64 {
        self.seq += 1;
        self.seq
    }

    fn respond(&mut self, out: &mut dyn Write, req: &Json, result: Result<Json, String>) {
        let seq = self.next_seq();
        let mut v = vec![("seq", num(seq as f64)), ("type", s("response")), ("request_seq", req.get("seq").clone()), ("command", req.get("command").clone())];
        match result {
            Ok(body) => {
                v.push(("success", Json::Bool(true)));
                v.push(("body", body));
            }
            Err(message) => {
                v.push(("success", Json::Bool(false)));
                v.push(("message", Json::Str(message)));
            }
        }
        send(out, &obj(v));
    }

    fn event(&mut self, out: &mut dyn Write, name: &str, body: Json) {
        let seq = self.next_seq();
        send(out, &obj(vec![("seq", num(seq as f64)), ("type", s("event")), ("event", s(name)), ("body", body)]));
    }

    /// The program's output so far, as `output` events.
    fn flush_output(&mut self, out: &mut dyn Write) {
        let Some(d) = &self.dbg else { return };
        let chunks = d.take_output();
        for c in chunks {
            let category = if c.stderr { "stderr" } else { "stdout" };
            let text = String::from_utf8_lossy(&c.data).to_string();
            self.event(out, "output", obj(vec![("category", s(category)), ("output", Json::Str(text))]));
        }
    }

    // ---------------- requests ----------------

    /// Handles one request. Returns false after `disconnect`.
    fn handle(&mut self, msg: &Json, out: &mut dyn Write) -> bool {
        if msg.get("type").str() != Some("request") {
            return true;
        }
        let args = msg.get("arguments");
        let command = msg.get("command").str().unwrap_or("");
        let result: Result<Json, String> = match command {
            "initialize" => Ok(obj(vec![
                ("supportsConfigurationDoneRequest", Json::Bool(true)),
                ("supportsEvaluateForHovers", Json::Bool(true)),
                ("supportsTerminateRequest", Json::Bool(true)),
                ("supportsSetVariable", Json::Bool(false)),
            ])),
            "launch" => self.launch(args, out),
            "setBreakpoints" => self.set_breakpoints(args),
            "setExceptionBreakpoints" => Ok(obj(vec![])),
            "configurationDone" => {
                self.configured = true;
                self.respond(out, msg, Ok(obj(vec![])));
                self.after_configuration(out);
                return true;
            }
            "threads" => Ok(obj(vec![("threads", Json::Arr(vec![obj(vec![("id", num(1)), ("name", s("main"))])]))])),
            "stackTrace" => Ok(self.stack_trace()),
            "scopes" => Ok(self.scopes(args)),
            "variables" => self.variables(args),
            "evaluate" => self.evaluate(args),
            "continue" => self.resume_command(out, Mode::Continue),
            "next" => self.resume_command(out, Mode::Step(Kind::Over)),
            "stepIn" => self.resume_command(out, Mode::Step(Kind::Into)),
            "stepOut" => self.resume_command(out, Mode::Step(Kind::Out)),
            "pause" => {
                if let Some(d) = &self.dbg {
                    d.pause();
                }
                Ok(obj(vec![]))
            }
            "terminate" | "disconnect" => {
                if let Some(d) = self.dbg.take() {
                    d.kill();
                }
                self.respond(out, msg, Ok(obj(vec![])));
                return command != "disconnect";
            }
            other => Err(format!("the request '{}' is not supported", other)),
        };
        let launch_ok = command == "launch" && result.is_ok();
        // the answer of a resuming request comes before the stop it leads to
        self.respond(out, msg, result);
        if launch_ok {
            self.event(out, "initialized", obj(vec![]));
        }
        true
    }

    fn launch(&mut self, args: &Json, out: &mut dyn Write) -> Result<Json, String> {
        let program = args.get("program").str().ok_or("launch needs `program`: the .zila file to debug")?;
        let program = PathBuf::from(program);
        let program = std::fs::canonicalize(&program).map(crate::package::strip_verbatim).map_err(|e| format!("cannot find {}: {}", program.display(), e))?;
        if let Some(t) = args.get("target").str() {
            if t != host() {
                return Err(format!("a {} program cannot be debugged on {}: build for the host or run it where it belongs", t, host()));
            }
        }
        if let Some(kind) = foreign_executable(&program) {
            return Err(format!("{} is {}, which cannot be debugged on {}", program.display(), kind, if cfg!(windows) { "Windows" } else { "Linux" }));
        }
        if program.extension().is_none_or(|e| e != "zila") {
            return Err(format!("{} is not a .zila program", program.display()));
        }
        let (exe, info) = self.build(&program, out)?;
        let argv: Vec<String> = match args.get("args") {
            Json::Arr(v) => v.iter().filter_map(|a| a.str().map(|x| x.to_string())).collect(),
            _ => Vec::new(),
        };
        let cwd = args.get("cwd").str().map(PathBuf::from).or_else(|| program.parent().map(|p| p.to_path_buf()));
        let dbg = Debugger::launch(&exe, &argv, cwd.as_deref())?;
        self.delta = dbg.base().wrapping_sub(info.base);
        self.program = program;
        self.stop_on_entry = args.get("stopOnEntry") == &Json::Bool(true);
        // a crash in the program's own `abort` is reported with its message
        if let Some(a) = info.abort {
            let _ = dbg.set_breakpoint(a.wrapping_add(self.delta));
        }
        self.info = Some(info);
        self.dbg = Some(dbg);
        self.exe = Some(exe);
        // breakpoints asked for before the program existed
        let early = std::mem::take(&mut self.early);
        for (path, lines, _) in early {
            let _ = self.apply_breakpoints(&path, &lines);
        }
        Ok(obj(vec![]))
    }

    /// Builds the program with debug information; the executable and what describes it.
    fn build(&mut self, program: &Path, out: &mut dyn Write) -> Result<(PathBuf, Info), String> {
        let src = std::fs::read_to_string(program).map_err(|e| format!("cannot read {}: {}", program.display(), e))?;
        crate::package::install_for(program)?;
        crate::comptime::register_siblings(program, &src);
        let file = crate::parser::parse_source(&src).map_err(|d| format!("{}:{}:{}: {}", program.display(), d.line, d.col, d.msg))?;
        let dir = std::env::temp_dir().join("zila-debug");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
        let stem = program.file_stem().and_then(|x| x.to_str()).unwrap_or("program");
        static BUILDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = BUILDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let name = format!("{}-{}-{}", stem, std::process::id(), n);
        let exe = dir.join(if cfg!(windows) { format!("{}.exe", name) } else { name });
        let options = crate::codegen_x64::CompileOptions { optimize: true, test_mode: false, debug: true };
        let built = if cfg!(windows) { crate::codegen_x64::compile_to_native_with(&file, &exe.to_string_lossy(), &options) } else { crate::codegen_x64::compile_to_linux(&file, &exe.to_string_lossy(), &options) };
        crate::warn::take().into_iter().for_each(|w| self.event(out, "output", obj(vec![("category", s("console")), ("output", Json::Str(format!("warning: {}\n", w)))])));
        built.map_err(|e| format!("compilation error: {}", e))?;
        let info = Info::load(&crate::codegen_x64::debug_info_path(&exe.to_string_lossy()))?;
        Ok((exe, info))
    }

    fn after_configuration(&mut self, out: &mut dyn Write) {
        if self.dbg.is_none() {
            return;
        }
        if self.stop_on_entry {
            // stop at the first statement of `main`
            let target = self.info.as_ref().and_then(|i| i.functions.iter().find(|f| f.name == "main" && f.module == "main").and_then(|f| i.first_statement(f)));
            if let (Some(a), Some(d)) = (target, &self.dbg) {
                let a = a.wrapping_add(self.delta);
                let _ = d.set_breakpoint(a);
                self.user_bps.insert(a);
            }
        }
        self.run(out, false);
    }

    /// Lets the program go (one instruction when `step`).
    fn run(&mut self, _out: &mut dyn Write, step: bool) {
        self.frames.clear();
        self.values.clear();
        self.running = true;
        if let Some(d) = &self.dbg {
            if step {
                d.step();
            } else {
                d.resume();
            }
        }
    }

    fn set_breakpoints(&mut self, args: &Json) -> Result<Json, String> {
        let path = PathBuf::from(args.get("source").get("path").str().ok_or("setBreakpoints needs a source path")?);
        let lines: Vec<u32> = match args.get("breakpoints") {
            Json::Arr(v) => v.iter().filter_map(|b| if let Json::Num(n) = b.get("line") { Some(*n as u32) } else { None }).collect(),
            _ => match args.get("lines") {
                Json::Arr(v) => v.iter().filter_map(|n| if let Json::Num(n) = n { Some(*n as u32) } else { None }).collect(),
                _ => Vec::new(),
            },
        };
        if self.info.is_none() {
            // not started yet: they are set when it is, and shown as pending until then
            self.early.retain(|e| norm(&e.0) != norm(&path));
            self.early.push((path, lines.clone(), 0));
            return Ok(obj(vec![("breakpoints", Json::Arr(lines.iter().map(|l| obj(vec![("verified", Json::Bool(false)), ("line", num(*l))])).collect()))]));
        }
        let shown = self.apply_breakpoints(&path, &lines)?;
        Ok(obj(vec![("breakpoints", Json::Arr(shown))]))
    }

    /// Replaces the breakpoints of one source: each line moves to the next one that has code.
    fn apply_breakpoints(&mut self, path: &Path, lines: &[u32]) -> Result<Vec<Json>, String> {
        let info = self.info.clone().ok_or("the program is not started")?;
        let key = norm(path);
        if let (Some(old), Some(d)) = (self.by_source.remove(&key), &self.dbg) {
            for a in old {
                if self.by_source.values().all(|v| !v.contains(&a)) {
                    self.user_bps.remove(&a);
                    let _ = d.clear_breakpoint(a);
                }
            }
        }
        let mut shown = Vec::new();
        let mut set = Vec::new();
        for &line in lines {
            let resolved = info.module_line_of(path, line, &self.program).and_then(|(module, l)| {
                let (actual, addr) = info.resolve_line(&module, l)?;
                // a line the package's merged text moved is shown in the file again
                let file_line = info.source_of(&module, actual, &self.program).map_or(actual, |x| x.1);
                Some((file_line, addr))
            });
            match resolved {
                Some((actual, addr)) => {
                    let rt = addr.wrapping_add(self.delta);
                    if let Some(d) = &self.dbg {
                        d.set_breakpoint(rt)?;
                    }
                    self.user_bps.insert(rt);
                    set.push(rt);
                    let mut b = vec![("verified", Json::Bool(true)), ("line", num(actual))];
                    if actual != line {
                        b.push(("message", Json::Str(format!("moved to line {}: there is no code on line {}", actual, line))));
                    }
                    shown.push(obj(b));
                }
                None => shown.push(obj(vec![("verified", Json::Bool(false)), ("line", num(line)), ("message", s("there is no code on or after this line"))])),
            }
        }
        self.by_source.insert(key, set);
        Ok(shown)
    }

    // ---------------- the stack ----------------

    fn read_u64(&self, addr: u64) -> Option<u64> {
        let d = self.dbg.as_ref()?;
        let b = d.read_memory(addr, 8).ok()?;
        Some(u64::from_le_bytes(b.try_into().ok()?))
    }

    /// The Zila frames of the stopped program, innermost first.
    fn compute_frames(&self) -> Vec<Frame> {
        let (Some(info), Some(d)) = (&self.info, &self.dbg) else { return Vec::new() };
        let Ok(regs) = d.regs() else { return Vec::new() };
        let delta = self.delta;
        let build = |a: u64| a.wrapping_sub(delta);
        let mut frames = Vec::new();
        let (mut pc, mut fp) = (regs.rip, regs.rbp);
        let mut at_call = false;
        if info.function_at(build(pc)).is_none() {
            // inside the runtime: the nearest return address into a function of the program on the stack
            let mut found = None;
            for i in 0..512u64 {
                let a = regs.rsp + 8 * i;
                match self.read_u64(a) {
                    Some(v) if info.function_at(build(v).wrapping_sub(1)).is_some() => {
                        found = Some((a, v));
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            let Some((at, ret)) = found else { return frames };
            pc = ret;
            at_call = true;
            // a runtime routine with a frame of its own saved the caller's frame pointer
            if at == regs.rbp.wrapping_add(8) {
                fp = self.read_u64(regs.rbp).unwrap_or(0);
            }
        }
        let mut last_fp = 0u64;
        for _ in 0..200 {
            let lookup = if at_call { build(pc).wrapping_sub(1) } else { build(pc) };
            let Some(f) = info.function_at(lookup) else { break };
            let func = info.functions.iter().position(|g| g.start == f.start).unwrap_or(0);
            let (module, line) = info.line_at(lookup).map(|(m, l)| (m.to_string(), l)).unwrap_or((f.module.clone(), f.line));
            frames.push(Frame { name: f.name.clone(), module, line, fp, func });
            if fp == 0 || fp <= last_fp {
                break;
            }
            last_fp = fp;
            let (Some(ret), Some(next)) = (self.read_u64(fp + 8), self.read_u64(fp)) else { break };
            pc = ret;
            fp = next;
            at_call = true;
        }
        frames
    }

    fn stack_trace(&mut self) -> Json {
        if self.frames.is_empty() && !self.running {
            self.frames = self.compute_frames();
        }
        let Some(info) = &self.info else { return obj(vec![("stackFrames", Json::Arr(vec![])), ("totalFrames", num(0))]) };
        let rows: Vec<Json> = self
            .frames
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let mut v = vec![("id", num(i as f64)), ("name", Json::Str(f.name.clone())), ("line", num(f.line)), ("column", num(1))];
                if let Some((path, line)) = info.source_of(&f.module, f.line, &self.program) {
                    v[2] = ("line", num(line));
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    v.push(("source", obj(vec![("name", Json::Str(name)), ("path", Json::Str(path.to_string_lossy().to_string()))])));
                }
                obj(v)
            })
            .collect();
        let total = rows.len();
        obj(vec![("stackFrames", Json::Arr(rows)), ("totalFrames", num(total as f64))])
    }

    fn scopes(&mut self, args: &Json) -> Json {
        let frame = if let Json::Num(n) = args.get("frameId") { *n as usize } else { 0 };
        if self.frames.is_empty() && !self.running {
            self.frames = self.compute_frames();
        }
        let r = self.values.locals_scope(frame);
        obj(vec![("scopes", Json::Arr(vec![obj(vec![("name", s("Locals")), ("variablesReference", num(r as f64)), ("expensive", Json::Bool(false))])]))])
    }

    fn variables(&mut self, args: &Json) -> Result<Json, String> {
        let r = if let Json::Num(n) = args.get("variablesReference") { *n as i64 } else { 0 };
        let rows = values::variables(self, r)?;
        Ok(obj(vec![("variables", Json::Arr(rows))]))
    }

    fn evaluate(&mut self, args: &Json) -> Result<Json, String> {
        let expr = args.get("expression").str().unwrap_or("").to_string();
        let frame = if let Json::Num(n) = args.get("frameId") { *n as usize } else { 0 };
        values::evaluate(self, &expr, frame)
    }

    // ---------------- running and stepping ----------------

    fn resume_command(&mut self, out: &mut dyn Write, mode: Mode) -> Result<Json, String> {
        if self.dbg.is_none() {
            return Err("the program is not running".into());
        }
        if self.running {
            return Err("the program is already running".into());
        }
        if self.frames.is_empty() {
            self.frames = self.compute_frames();
        }
        match mode {
            Mode::Continue => {
                self.step = None;
                self.run(out, false);
                Ok(obj(vec![("allThreadsContinued", Json::Bool(true))]))
            }
            Mode::Step(kind) => {
                self.begin_step(kind)?;
                Ok(obj(vec![]))
            }
        }
    }

    fn begin_step(&mut self, kind: Kind) -> Result<(), String> {
        let top = self.frames.first().cloned().ok_or("the program is not stopped in code of its own")?;
        let info = self.info.clone().ok_or("no debug information")?;
        let f = &info.functions[top.func];
        let (start, end) = (f.start.wrapping_add(self.delta), f.end.wrapping_add(self.delta));
        let d = self.dbg.as_ref().ok_or("not running")?;
        let regs = d.regs()?;
        let mut st = Step { kind, start, end, prev_rsp: regs.rsp, phase: Phase::Stepping, temp: None };
        self.frames.clear();
        self.values.clear();
        self.running = true;
        if kind == Kind::Out {
            // to the return address of this frame: just above the saved frame pointer
            let ret = self.read_u64(regs.rbp + 8).ok_or("cannot read the return address")?;
            st.phase = Phase::RunOut { guard: regs.rbp };
            st.temp = Some(ret);
            self.set_temp(ret);
            self.step = Some(st);
            self.dbg.as_ref().unwrap().resume();
        } else {
            self.step = Some(st);
            self.dbg.as_ref().unwrap().step();
        }
        Ok(())
    }

    fn set_temp(&mut self, addr: u64) {
        if let Some(d) = &self.dbg {
            let _ = d.set_breakpoint(addr);
        }
    }

    fn clear_temp(&mut self, addr: u64) {
        if !self.user_bps.contains(&addr) {
            if let Some(d) = &self.dbg {
                let _ = d.clear_breakpoint(addr);
            }
        }
    }

    /// What the debugger reported: the next move of a step, or a stop shown to the client.
    fn on_stop(&mut self, stop: Stop, out: &mut dyn Write) {
        self.flush_output(out);
        self.running = false;
        let Some(mut st) = self.step.take() else { return self.report(stop, "breakpoint", out) };
        match stop {
            Stop::Exited(_) | Stop::Fault { .. } | Stop::Paused => {
                if let Some(t) = st.temp.take() {
                    self.clear_temp(t);
                }
                let reason = if stop == Stop::Paused { "pause" } else { "exception" };
                self.report(stop, reason, out)
            }
            Stop::Breakpoint(a) if Some(a) != st.temp || self.user_bps.contains(&a) && Some(a) != st.temp => {
                if let Some(t) = st.temp.take() {
                    self.clear_temp(t);
                }
                self.report(Stop::Breakpoint(a), "breakpoint", out)
            }
            Stop::Breakpoint(a) => self.at_temp(st, a, out),
            Stop::Step(pc) => self.after_instruction(st, pc, out),
        }
    }

    /// A temporary breakpoint of a step was reached.
    fn at_temp(&mut self, mut st: Step, addr: u64, out: &mut dyn Write) {
        let Some(d) = &self.dbg else { return };
        let Ok(regs) = d.regs() else { return };
        match st.phase.clone() {
            Phase::RunToReturn { guard } => {
                if regs.rsp <= guard {
                    // a deeper activation of the same function came back first: go on
                    self.step = Some(st);
                    self.running = true;
                    self.dbg.as_ref().unwrap().resume();
                    return;
                }
                st.temp = None;
                self.clear_temp(addr);
                st.phase = Phase::Stepping;
                st.prev_rsp = regs.rsp;
                self.step = Some(st);
                self.running = true;
                self.dbg.as_ref().unwrap().step();
            }
            Phase::RunOut { guard } => {
                if regs.rsp <= guard {
                    self.step = Some(st);
                    self.running = true;
                    self.dbg.as_ref().unwrap().resume();
                    return;
                }
                self.clear_temp(addr);
                self.finish_step(regs.rip, out);
            }
            Phase::Enter => {
                self.clear_temp(addr);
                self.finish_step(regs.rip, out);
            }
            Phase::Stepping => {
                self.clear_temp(addr);
                self.finish_step(regs.rip, out);
            }
        }
    }

    /// One instruction was executed during a step.
    fn after_instruction(&mut self, mut st: Step, pc: u64, out: &mut dyn Write) {
        let Some(info) = self.info.clone() else { return };
        let Some(d) = &self.dbg else { return };
        let Ok(regs) = d.regs() else { return };
        let build = pc.wrapping_sub(self.delta);
        if pc >= st.start && pc < st.end && pc != st.start {
            // still in the function: another statement starts at a row. A row of the same line counts
            // too: it is the next time round a loop, since the step left this statement's first
            // instruction before.
            if info.row_at(build).is_some() {
                return self.finish_step(pc, out);
            }
            st.prev_rsp = regs.rsp;
            self.step = Some(st);
            self.running = true;
            self.dbg.as_ref().unwrap().step();
            return;
        }
        if regs.rsp < st.prev_rsp {
            // a call: its return address is on the top of the stack
            let ret = self.read_u64(regs.rsp).unwrap_or(0);
            let entered = info.function_at(build).filter(|f| f.start == build).cloned();
            if let (Kind::Into, Some(f)) = (st.kind, entered) {
                if let Some(first) = info.first_statement(&f) {
                    let target = first.wrapping_add(self.delta);
                    st.phase = Phase::Enter;
                    st.temp = Some(target);
                    self.set_temp(target);
                    self.step = Some(st);
                    self.running = true;
                    self.dbg.as_ref().unwrap().resume();
                    return;
                }
            }
            st.phase = Phase::RunToReturn { guard: regs.rsp };
            st.temp = Some(ret);
            self.set_temp(ret);
            self.step = Some(st);
            self.running = true;
            self.dbg.as_ref().unwrap().resume();
            return;
        }
        // a return: stop in the caller when it is the program's own code, else let the program run on
        if info.function_at(build).is_some() {
            return self.finish_step(pc, out);
        }
        self.run(out, false);
    }

    fn finish_step(&mut self, _pc: u64, out: &mut dyn Write) {
        self.step = None;
        self.running = false;
        self.frames = self.compute_frames();
        self.event(out, "stopped", obj(vec![("reason", s("step")), ("threadId", num(1)), ("allThreadsStopped", Json::Bool(true))]));
    }

    /// A stop that is shown to the client.
    fn report(&mut self, stop: Stop, reason: &str, out: &mut dyn Write) {
        match stop {
            Stop::Exited(code) => {
                self.dbg = None;
                self.event(out, "exited", obj(vec![("exitCode", num(code as f64))]));
                self.event(out, "terminated", obj(vec![]));
            }
            Stop::Fault { code, addr } => {
                self.frames = self.compute_frames();
                let text = format!("the program crashed: {} at {:#x}", fault_name(code), addr);
                self.event(out, "stopped", obj(vec![("reason", s("exception")), ("description", Json::Str(text.clone())), ("text", Json::Str(text)), ("threadId", num(1)), ("allThreadsStopped", Json::Bool(true))]));
            }
            Stop::Breakpoint(a) if self.info.as_ref().and_then(|i| i.abort).is_some_and(|x| x.wrapping_add(self.delta) == a) => {
                self.frames = self.compute_frames();
                let text = values::abort_message(self);
                self.event(out, "stopped", obj(vec![("reason", s("exception")), ("description", Json::Str(text.clone())), ("text", Json::Str(text)), ("threadId", num(1)), ("allThreadsStopped", Json::Bool(true))]));
            }
            _ => {
                self.frames = self.compute_frames();
                self.event(out, "stopped", obj(vec![("reason", s(reason)), ("threadId", num(1)), ("allThreadsStopped", Json::Bool(true))]));
            }
        }
    }

    /// Looks at the debugger for a stop and at the program for output.
    fn poll(&mut self, out: &mut dyn Write) {
        self.flush_output(out);
        let stop = match &self.dbg {
            Some(d) if self.running => d.next_stop(Duration::from_millis(0)),
            _ => None,
        };
        if let Some(stop) = stop {
            self.on_stop(stop, out);
        }
    }
}

enum Mode {
    Continue,
    Step(Kind),
}

fn fault_name(code: u32) -> &'static str {
    if cfg!(unix) {
        // a signal number
        return match code {
            11 => "segmentation fault",
            8 => "arithmetic exception",
            4 => "illegal instruction",
            7 => "bus error",
            6 => "abort signal",
            _ => "signal",
        };
    }
    match code {
        0xC000_0005 => "access violation",
        0xC000_00FD => "stack overflow",
        0xC000_0094 => "division by zero",
        0xC000_001D => "illegal instruction",
        _ => "exception",
    }
}

/// Serves the requests that arrive on `rx` until `disconnect` or the end of the input.
pub fn serve(rx: Receiver<Json>, out: &mut dyn Write) -> i32 {
    let mut session = Session::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(msg) => {
                if !session.handle(&msg, out) {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        session.poll(out);
    }
    if let Some(d) = session.dbg.take() {
        d.kill();
    }
    0
}

/// `zila --dap`: the adapter on standard input and output.
pub fn run() -> i32 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        while let Some(msg) = read_message(&mut input) {
            if tx.send(msg).is_err() {
                break;
            }
        }
    });
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    serve(rx, &mut out)
}
