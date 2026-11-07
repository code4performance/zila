//! `brasa-parse --run`: builds the program for this machine into a temporary executable,
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
    let n = BUILD.fetch_add(1, Ordering::SeqCst);
    let ext = if target == "windows" { ".exe" } else { "" };
    let exe = std::env::temp_dir().join(format!("brasa-run-{}-{}{}", std::process::id(), n, ext));
    let _guard = Cleanup(exe.clone());
    let options = CompileOptions { optimize, test_mode: false };
    let path = exe.to_string_lossy().to_string();
    let built = if target == "linux" { compile_to_linux(file, &path, &options) } else { compile_to_native_with(file, &path, &options) };
    if let Err(e) = built {
        eprintln!("compilation error: {}", e);
        return 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)) {
            eprintln!("error: cannot make {} executable: {}", path, e);
            return 1;
        }
    }
    match Command::new(&exe).status() {
        Ok(status) => {
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
        Err(e) => {
            eprintln!("error: cannot run {}: {}", path, e);
            1
        }
    }
}
