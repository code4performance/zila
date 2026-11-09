//! `zila --run`: builds the program for this machine into a temporary executable,
//! runs it with the terminal's input and output, deletes it and returns its exit status.

use crate::ast::File;
use crate::codegen_x64::{compile_to_linux, compile_to_native_with, CompileOptions};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static BUILD: AtomicUsize = AtomicUsize::new(0);

/// The target of the machine the tool runs on.
pub fn host_target() -> Option<&'static str> {
    if cfg!(windows) {
        Some("windows")
    } else if cfg!(target_os = "linux") {
        Some("linux")
    } else {
        None
    }
}

/// Deletes the temporary executable however `run_file` ends.
struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Builds `file` for `target` (which must be the host), runs it and returns the exit status
/// of the tool: the program's own, `128 + signal` after a signal, 1 for a build error.
pub fn run_file(file: &File, optimize: bool, target: &str) -> i32 {
    run_file_with(file, optimize, target, &|e| e)
}

/// Like `run_file`, but `rewrite` may change the text of a build error before it is printed
/// (`zila eval` uses it to name the snippet's lines).
pub fn run_file_with(file: &File, optimize: bool, target: &str, rewrite: &dyn Fn(String) -> String) -> i32 {
    let (exe, _guard) = match build_exe(file, optimize, target) {
        Ok(b) => b,
        Err(Failure::Build(e)) => {
            eprintln!("compilation error: {}", rewrite(e));
            return 1;
        }
        Err(Failure::Other(e)) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    let path = exe.to_string_lossy().to_string();
    match Command::new(&exe).status() {
        Ok(status) => status_code(status),
        Err(e) => {
            eprintln!("error: cannot run {}: {}", path, e);
            1
        }
    }
}

/// What a program that was built and run with its output captured did.
#[derive(Debug)]
pub struct Captured {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Why a program could not be run.
enum Failure {
    /// The compiler's message.
    Build(String),
    /// Anything else (a file that cannot be made executable; only built on Unix).
    #[cfg_attr(not(unix), allow(dead_code))]
    Other(String),
}

/// Builds `file` into a temporary executable; the guard deletes it when dropped.
fn build_exe(file: &File, optimize: bool, target: &str) -> Result<(PathBuf, Cleanup), Failure> {
    let n = BUILD.fetch_add(1, Ordering::SeqCst);
    let ext = if target == "windows" { ".exe" } else { "" };
    let exe = std::env::temp_dir().join(format!("zila-run-{}-{}{}", std::process::id(), n, ext));
    let guard = Cleanup(exe.clone());
    let options = CompileOptions { optimize, test_mode: false };
    let path = exe.to_string_lossy().to_string();
    let built = if target == "linux" { compile_to_linux(file, &path, &options) } else { compile_to_native_with(file, &path, &options) };
    if let Err(e) = built {
        return Err(Failure::Build(e.to_string()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)) {
            return Err(Failure::Other(format!("cannot make {} executable: {}", path, e)));
        }
    }
    Ok((exe, guard))
}

/// The tool's exit status for a finished program: its own, `128 + signal` after a signal.
fn status_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    1
}

/// Builds and runs `file` like `run_file`, but collects standard output and error instead of
/// passing them through (standard input is the terminal's). `Err` is the compiler's message,
/// or the reason the program could not be started.
pub fn run_captured(file: &File, optimize: bool, target: &str) -> Result<Captured, String> {
    let (exe, _guard) = match build_exe(file, optimize, target) {
        Ok(b) => b,
        Err(Failure::Build(e)) | Err(Failure::Other(e)) => return Err(e),
    };
    let out = Command::new(&exe).stdin(std::process::Stdio::inherit()).output().map_err(|e| format!("cannot run {}: {}", exe.display(), e))?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).replace("\r\n", "\n");
    Ok(Captured { code: status_code(out.status), stdout: text(&out.stdout), stderr: text(&out.stderr) })
}
