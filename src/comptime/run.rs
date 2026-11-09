//! Running generators: the driver program, the child process with its limits, and the cache.
//!
//! The compiler builds one executable per batch of generator calls for the host (the PE path on
//! Windows, the ELF path on Linux), runs it as a child with no input and piped output, and reads
//! `ok`/`fail` records from its standard output. Results are cached on disk by a hash of
//! everything they depend on.

use super::check::closure_text;
use super::reflect::{zila_lit, serialize_call, table_statements, WireCall};
use super::state::*;
use super::walk::*;
use crate::ast::*;
use crate::codegen_x64::{compile_to_linux, compile_to_native_with, CompileOptions, Target};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

type R<T> = Result<T, String>;

/// Output of one generator call is limited to this many bytes.
pub const OUTPUT_CAP: usize = 1 << 20;

/// The memory a generator process may use.
const MEMORY_LIMIT: u64 = 1 << 30;

// ---------------------------------------------------------------------------
// SHA-256
// ---------------------------------------------------------------------------

pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{:08x}", x)).collect()
}

// ---------------------------------------------------------------------------
// Calls and results
// ---------------------------------------------------------------------------

/// One generator call to run.
pub struct CallSpec {
    pub wire: WireCall,
    pub gen_module: String,
    pub gen_name: String,
    pub closure: Vec<(String, String)>,
    pub site_module: String,
    pub site_line: u32,
    pub site_desc: String,
    pub ty_name: String,
    /// Makes `reflect.fresh` names differ between calls (the start of the cache key).
    pub tag: String,
}

pub enum Outcome {
    Ok(String),
    /// `reflect.fail`: the line in the user's code and the message
    Fail { line: u32, msg: String },
}

fn host_target() -> R<Target> {
    if cfg!(windows) {
        Ok(Target::Windows)
    } else if cfg!(target_os = "linux") {
        Ok(Target::Linux)
    } else {
        Err("comptime code runs on the build machine, which must be Windows or Linux".into())
    }
}

fn cache_dir() -> PathBuf {
    let s = settings();
    if let Some(d) = s.cache_dir {
        return d;
    }
    if let Ok(d) = std::env::var("ZILA_COMPTIME_CACHE") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    std::env::temp_dir().join("zila-comptime")
}

/// The cache key of a call: the compiler version and host, the canonical text of what the
/// generator reaches, its name and kind, and the serialized input.
fn cache_key(uni: &super::reflect::Universe, spec: &CallSpec, target_name: &str) -> String {
    let s = settings();
    let mut w = spec.wire.clone();
    w.id = 0;
    let text = format!(
        "zila {} {}\nhost {} {} target {}\ngen {} {}\n{}\n--\n{}",
        env!("CARGO_PKG_VERSION"),
        s.version_tag,
        std::env::consts::OS,
        std::env::consts::ARCH,
        target_name,
        spec.wire.generator,
        spec.wire.arg_kind,
        closure_text(uni, &spec.closure),
        serialize_call(&w)
    );
    sha256_hex(text.as_bytes())
}

fn cache_read(key: &str) -> Option<String> {
    let data = std::fs::read(cache_dir().join(format!("{}.out", key))).ok()?;
    let nl = data.iter().position(|b| *b == b'\n')?;
    let head = std::str::from_utf8(&data[..nl]).ok()?;
    let len: usize = head.strip_prefix("zila-comptime 1 ")?.parse().ok()?;
    let body = &data[nl + 1..];
    if body.len() != len {
        return None;
    }
    String::from_utf8(body.to_vec()).ok()
}

fn cache_write(key: &str, text: &str) {
    let dir = cache_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut data = format!("zila-comptime 1 {}\n", text.len()).into_bytes();
    data.extend_from_slice(text.as_bytes());
    let tmp = dir.join(format!("{}.tmp{}", key, std::process::id()));
    if std::fs::write(&tmp, &data).is_ok() && std::fs::rename(&tmp, dir.join(format!("{}.out", key))).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

// ---------------------------------------------------------------------------
// The driver program
// ---------------------------------------------------------------------------

/// Names an expression or type mentions: used to keep only the `use`s the driver needs.
struct Mentions(HashSet<String>);

impl Visit for Mentions {
    fn expr(&mut self, e: &mut Expr) -> R<bool> {
        match &e.kind {
            ExprKind::Ident(n) => {
                self.0.insert(n.clone());
            }
            ExprKind::Record { path, .. } => {
                self.0.insert(path[0].clone());
            }
            _ => {}
        }
        Ok(true)
    }
    fn ty(&mut self, t: &mut Type) {
        if let Type::Named { path, .. } = t {
            self.0.insert(path[0].clone());
        }
    }
}

fn has_call(e: &Expr) -> bool {
    let mut found = false;
    struct C<'a>(&'a mut bool);
    impl<'a> Visit for C<'a> {
        fn expr(&mut self, e: &mut Expr) -> R<bool> {
            if matches!(e.kind, ExprKind::Call { .. } | ExprKind::Lambda { .. }) {
                *self.0 = true;
            }
            Ok(true)
        }
    }
    let mut copy = e.clone();
    let _ = walk_expr(&mut copy, &mut C(&mut found));
    found
}

/// The source of the program that runs `specs`: the program's own types and the functions the
/// generators reach, then a `main` that rebuilds each type table, calls the generator and
/// writes the result.
fn driver_file(uni: &super::reflect::Universe, main_module: &str, specs: &[&CallSpec], target_name: &str) -> R<File> {
    let main_file = uni.file(main_module).ok_or("internal error: no main module")?;
    let mut reached: HashSet<String> = HashSet::new();
    let mut gen_modules: HashSet<String> = HashSet::new();
    for s in specs {
        for (m, n) in &s.closure {
            if m == main_module {
                reached.insert(n.clone());
            }
        }
        if s.gen_module != main_module {
            gen_modules.insert(s.gen_module.clone());
        }
    }
    let mut decls: Vec<Decl> = Vec::new();
    for d in &main_file.decls {
        let keep = match &d.kind {
            DeclKind::Func(fd) => reached.contains(&fd.name) && fd.name != "main",
            DeclKind::Type(_) => true,
            DeclKind::Var(v) => !v.mutable && v.value.as_ref().is_none_or(|e| !has_call(e)),
            _ => false,
        };
        if !keep {
            continue;
        }
        let mut d = d.clone();
        d.attrs.retain(|a| !matches!(a.name.as_str(), "derive" | "block" | "generated"));
        if let DeclKind::Func(fd) = &mut d.kind {
            fd.comptime = false;
        }
        decls.push(d);
    }
    // the packages the copied code uses
    let mut m = Mentions(HashSet::new());
    for d in decls.iter_mut() {
        let _ = walk_decl(d, &mut m);
    }
    let mut uses: Vec<Vec<String>> = vec![vec!["reflect".to_string()]];
    for u in &main_file.uses {
        if m.0.contains(&u[0]) && u[0] != "reflect" && !uses.contains(u) {
            uses.push(u.clone());
        }
    }
    let mut gm: Vec<String> = gen_modules.into_iter().collect();
    gm.sort();
    for g in gm {
        if g != "reflect" && !uses.iter().any(|u| u[0] == g) {
            uses.push(vec![g]);
        }
    }
    let mut body = String::from("package comptime_run\nfunc main()\n");
    body.push_str(&format!("    reflect.set_target({})\n", zila_lit(target_name)));
    for s in specs {
        let w = &s.wire;
        let tag = s.tag.clone();
        body.push_str(&format!("    reflect.begin({}, {})\n", w.id, zila_lit(&tag)));
        for st in table_statements(&w.table) {
            body.push_str("    ");
            body.push_str(&st);
            body.push('\n');
        }
        let call = if s.gen_module == main_module { s.gen_name.clone() } else { format!("{}.{}", s.gen_module, s.gen_name) };
        let arg = match &w.raw {
            Some(r) => zila_lit(r),
            None => format!("reflect.type_at({})", w.root),
        };
        body.push_str(&format!("    reflect.emit({}, {}({}))\n", w.id, call, arg));
    }
    body.push_str("end\n");
    let entry = crate::parser::parse_source_with(&body, HashSet::new()).map_err(|d| format!("internal error in the comptime driver: {}", d))?;
    decls.extend(entry.decls);
    Ok(File { package: "comptime_run".to_string(), uses, decls })
}

fn compile_host(file: &File, exe: &Path, target: Target) -> R<()> {
    let prev = set_driver_mode(true);
    let opts = CompileOptions { optimize: true, test_mode: false };
    let path = exe.to_string_lossy().to_string();
    let r = match target {
        Target::Linux => compile_to_linux(file, &path, &opts),
        _ => compile_to_native_with(file, &path, &opts),
    };
    set_driver_mode(prev);
    r.map_err(|e| format!("while building the comptime program: {}", e))
}

// ---------------------------------------------------------------------------
// The child process
// ---------------------------------------------------------------------------

struct ChildOut {
    stdout: Vec<u8>,
    stderr: String,
    code: Option<i32>,
    timed_out: bool,
    over_cap: bool,
}

#[cfg(windows)]
mod job {
    //! A job object limits the memory of the child and kills it when the compiler goes away.
    use std::ffi::c_void;

    type Handle = *mut c_void;

    #[repr(C)]
    struct BasicLimit {
        per_process_user_time: i64,
        per_job_user_time: i64,
        limit_flags: u32,
        min_working_set: usize,
        max_working_set: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    struct IoCounters {
        v: [u64; 6],
    }

    #[repr(C)]
    struct ExtendedLimit {
        basic: BasicLimit,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process: usize,
        peak_job: usize,
    }

    extern "system" {
        fn CreateJobObjectW(attrs: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: i32, info: *mut c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn CloseHandle(h: Handle) -> i32;
    }

    pub struct Job(Handle);

    // SAFETY: the handle is only used from the thread that owns the `Job`.
    unsafe impl Send for Job {}

    impl Job {
        /// A job that allows `limit` bytes per process; `None` when it cannot be made.
        pub fn new(limit: u64) -> Option<Job> {
            unsafe {
                let h = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                let mut info: ExtendedLimit = std::mem::zeroed();
                // JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                info.basic.limit_flags = 0x100 | 0x2000;
                info.process_memory_limit = limit as usize;
                // JobObjectExtendedLimitInformation = 9
                if SetInformationJobObject(h, 9, &mut info as *mut ExtendedLimit as *mut c_void, std::mem::size_of::<ExtendedLimit>() as u32) == 0 {
                    CloseHandle(h);
                    return None;
                }
                Some(Job(h))
            }
        }

        pub fn assign(&self, child: &std::process::Child) {
            use std::os::windows::io::AsRawHandle;
            unsafe {
                AssignProcessToJobObject(self.0, child.as_raw_handle() as Handle);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(unix)]
fn limit_memory(cmd: &mut std::process::Command, limit: u64) {
    use std::os::unix::process::CommandExt;
    #[repr(C)]
    struct RLimit {
        cur: u64,
        max: u64,
    }
    extern "C" {
        fn setrlimit(resource: i32, rlim: *const RLimit) -> i32;
    }
    // RLIMIT_AS = 9 on Linux
    unsafe {
        cmd.pre_exec(move || {
            let r = RLimit { cur: limit, max: limit };
            setrlimit(9, &r);
            Ok(())
        });
    }
}

fn run_child(exe: &Path, timeout: Duration, cap: usize) -> R<ChildOut> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    limit_memory(&mut cmd, MEMORY_LIMIT);
    #[cfg(windows)]
    let job = job::Job::new(MEMORY_LIMIT);
    let mut child = cmd.spawn().map_err(|e| format!("cannot start the comptime program: {}", e))?;
    #[cfg(windows)]
    if let Some(j) = &job {
        j.assign(&child);
    }
    let over = Arc::new(AtomicBool::new(false));
    let total = Arc::new(AtomicUsize::new(0));
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let (over2, total2) = (over.clone(), total.clone());
    let t_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 65536];
        loop {
            match out.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    total2.store(buf.len(), Ordering::SeqCst);
                    if buf.len() > cap {
                        over2.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
        }
        buf
    });
    let t_err = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = err.by_ref().take(64 * 1024).read_to_end(&mut s);
        let mut sink = [0u8; 4096];
        while matches!(err.read(&mut sink), Ok(n) if n > 0) {}
        String::from_utf8_lossy(&s).to_string()
    });
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {}
            Err(e) => return Err(format!("cannot wait for the comptime program: {}", e)),
        }
        if over.load(Ordering::SeqCst) {
            let _ = child.kill();
            break child.wait().ok();
        }
        if start.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = t_out.join().unwrap_or_default();
    let stderr = t_err.join().unwrap_or_default();
    Ok(ChildOut { stdout, stderr, code: status.and_then(|s| s.code()), timed_out, over_cap: over.load(Ordering::SeqCst) })
}

/// Reads the `ok` and `fail` records. Returns the records in order and whether the output ended
/// cleanly.
fn parse_output(data: &[u8]) -> Vec<(usize, Outcome)> {
    let mut v = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let Some(nl) = data[pos..].iter().position(|b| *b == b'\n') else { break };
        let line = String::from_utf8_lossy(&data[pos..pos + nl]).replace('\r', "");
        pos += nl + 1;
        let parts: Vec<&str> = line.split(' ').collect();
        let read_body = |pos: &mut usize, n: usize| -> Option<String> {
            if *pos + n > data.len() {
                return None;
            }
            let s = String::from_utf8_lossy(&data[*pos..*pos + n]).to_string();
            *pos += n;
            // the newline `print` adds (and a carriage return before it, on a console)
            if data.get(*pos) == Some(&b'\r') {
                *pos += 1;
            }
            if data.get(*pos) == Some(&b'\n') {
                *pos += 1;
            }
            Some(s)
        };
        match parts.as_slice() {
            ["ok", id, n] => {
                let (Ok(id), Ok(n)) = (id.parse::<usize>(), n.parse::<usize>()) else { break };
                match read_body(&mut pos, n) {
                    Some(text) => v.push((id, Outcome::Ok(text))),
                    None => break,
                }
            }
            ["fail", id, line, n] => {
                let (Ok(id), Ok(line), Ok(n)) = (id.parse::<usize>(), line.parse::<u32>(), n.parse::<usize>()) else { break };
                match read_body(&mut pos, n) {
                    Some(msg) => v.push((id, Outcome::Fail { line, msg })),
                    None => break,
                }
            }
            _ => {}
        }
    }
    v
}

/// Runs the calls (from the cache when it can) and returns one outcome for each.
pub fn run_calls(uni: &super::reflect::Universe, main_module: &str, specs: &[CallSpec], target_name: &str) -> R<Vec<Outcome>> {
    let st = settings();
    let mut results: Vec<Option<Outcome>> = (0..specs.len()).map(|_| None).collect();
    let keys: Vec<String> = specs.iter().map(|s| cache_key(uni, s, target_name)).collect();
    if st.cache {
        for (i, k) in keys.iter().enumerate() {
            if let Some(text) = cache_read(k) {
                results[i] = Some(Outcome::Ok(text));
            }
        }
    }
    let misses: Vec<usize> = (0..specs.len()).filter(|i| results[*i].is_none()).collect();
    if !misses.is_empty() {
        let host = host_target()?;
        let miss_specs: Vec<&CallSpec> = misses.iter().map(|i| &specs[*i]).collect();
        // the driver numbers its calls by position in this run
        let renumbered: Vec<CallSpec> = miss_specs
            .iter()
            .enumerate()
            .map(|(n, s)| {
                let mut w = s.wire.clone();
                w.id = n;
                CallSpec {
                    wire: w,
                    gen_module: s.gen_module.clone(),
                    gen_name: s.gen_name.clone(),
                    closure: s.closure.clone(),
                    site_module: s.site_module.clone(),
                    site_line: s.site_line,
                    site_desc: s.site_desc.clone(),
                    ty_name: s.ty_name.clone(),
                    tag: keys[misses[n]][..8].to_string(),
                }
            })
            .collect();
        let refs: Vec<&CallSpec> = renumbered.iter().collect();
        let file = driver_file(uni, main_module, &refs, target_name)?;
        static BUILD: AtomicUsize = AtomicUsize::new(0);
        let n = BUILD.fetch_add(1, Ordering::SeqCst);
        let ext = if host == Target::Windows { ".exe" } else { "" };
        let exe = std::env::temp_dir().join(format!("zila-comptime-{}-{}{}", std::process::id(), n, ext));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _guard = Cleanup(exe.clone());
        compile_host(&file, &exe, host)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
        }
        for s in &renumbered {
            log_launch(format!("{}({})", s.wire.generator, s.ty_name));
        }
        let cap = renumbered.len() * (OUTPUT_CAP + 256) + 4096;
        let out = run_child(&exe, Duration::from_secs(st.timeout_secs.max(1)), cap)?;
        let records = parse_output(&out.stdout);
        let mut got: Vec<Option<Outcome>> = (0..renumbered.len()).map(|_| None).collect();
        for (id, o) in records {
            if id < got.len() && got[id].is_none() {
                got[id] = Some(o);
            }
        }
        let any_fail = got.iter().any(|g| matches!(g, Some(Outcome::Fail { .. })));
        for (n, slot) in got.into_iter().enumerate() {
            let s = &renumbered[n];
            let at = format!("{}.zila:{}:", s.site_module, s.site_line);
            match slot {
                Some(Outcome::Ok(text)) => {
                    if text.len() > OUTPUT_CAP {
                        return Err(format!(
                            "{} comptime function `{}` produced more than 1 MiB of output (for `{}`)",
                            at, s.wire.generator, s.ty_name
                        ));
                    }
                    results[misses[n]] = Some(Outcome::Ok(text));
                }
                Some(f @ Outcome::Fail { .. }) => results[misses[n]] = Some(f),
                None if any_fail => {}
                None => {
                    if out.over_cap {
                        return Err(format!(
                            "{} comptime function `{}` produced more than 1 MiB of output (for `{}`)",
                            at, s.wire.generator, s.ty_name
                        ));
                    }
                    if out.timed_out {
                        return Err(format!(
                            "{} comptime function `{}` did not finish within {} seconds (for `{}`; raise it with --comptime-timeout)",
                            at,
                            s.wire.generator,
                            st.timeout_secs.max(1),
                            s.ty_name
                        ));
                    }
                    let first = out.stderr.lines().next().unwrap_or("").trim().to_string();
                    return Err(format!(
                        "{} comptime function `{}` crashed (exit code {}) while generating for `{}`{}",
                        at,
                        s.wire.generator,
                        out.code.map_or("none".to_string(), |c| c.to_string()),
                        s.ty_name,
                        if first.is_empty() { String::new() } else { format!(": {}", first) }
                    ));
                }
            }
        }
        if st.cache {
            for (n, i) in misses.iter().enumerate() {
                let _ = n;
                if let Some(Outcome::Ok(text)) = &results[*i] {
                    cache_write(&keys[*i], text);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (i, r) in results.into_iter().enumerate() {
        let r = r.unwrap_or(Outcome::Fail { line: 0, msg: "not run because an earlier generator failed".to_string() });
        if st.dump {
            if let Outcome::Ok(text) = &r {
                eprintln!("--- comptime {} by `{}` for `{}` ---\n{}\n--- end ---", &keys[i][..12], specs[i].wire.generator, specs[i].ty_name, text);
            }
        }
        out.push(r);
    }
    Ok(out)
}
