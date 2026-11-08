//! Warnings of a build: things worth telling the person but that do not stop it. The compiler
//! library pushes them; the command line prints them once the build is done. They are kept per
//! thread, so builds running side by side in one process (the tests) do not see each other's.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

static TO_STDERR: AtomicBool = AtomicBool::new(false);

/// The command line asks for warnings to be printed as they are found, so that they come before
/// the output of a program that `--run` starts.
pub fn print_to_stderr(on: bool) {
    TO_STDERR.store(on, Ordering::Relaxed);
}

thread_local! {
    static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Records a warning (the same text is kept once).
pub fn push(msg: String) {
    WARNINGS.with(|w| {
        let mut w = w.borrow_mut();
        if !w.contains(&msg) {
            if TO_STDERR.load(Ordering::Relaxed) {
                eprintln!("warning: {}", msg);
            }
            w.push(msg);
        }
    });
}

/// The warnings so far, which are forgotten.
pub fn take() -> Vec<String> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

/// Forgets the warnings (the start of a build).
pub fn clear() {
    WARNINGS.with(|w| w.borrow_mut().clear());
}
