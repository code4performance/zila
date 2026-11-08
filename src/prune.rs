//! Reachability pruning (spec `static-linking`): what the program cannot reach is not emitted.
//!
//! The code generator produces one text unit per function, per global, per string and so on.
//! This module works on those units: it finds the symbols each one mentions, starts from the
//! roots (the entry, the runtime, exported functions, callbacks) and keeps what can be reached.
//! A module's initialiser is special: it runs when the program starts, so it is kept only when
//! it has effects (it calls user code or C) or when something kept reads a global it sets, which
//! is what makes `use x` free when nothing of `x` is used.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Turns pruning on or off (`--no-prune`); on by default.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// What a unit is, for the rules of the graph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Code or data kept only when something kept refers to it.
    Plain,
    /// A module initialiser, `B_init_<module>`.
    Init,
}

pub struct Unit {
    pub text: String,
    pub defs: Vec<String>,
    pub kind: Kind,
}

/// The identifiers of an assembly text, in order.
pub fn idents(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|t| !t.is_empty() && !t.as_bytes()[0].is_ascii_digit())
}

/// Runtime helpers that only allocate, count references or move memory: calling them is not an
/// effect that the program can observe.
const PURE_RUNTIME: [&str; 11] = ["rt_alloc", "rt_retain", "rt_release", "rt_drop", "rt_free_obj", "rt_list_", "rt_text_", "rt_concat", "rt_memcpy", "rt_map_", "rt_kind_"];

/// Whether a unit does something besides computing: it calls C, a closure, a runtime helper that
/// is not on the pure list (printing, `exit`, aborting), or makes a system call. Calls to other
/// units are not looked at here; `reachable` follows them.
pub fn unit_has_effects(text: &str) -> bool {
    for line in text.lines() {
        let t = line.trim_start();
        if t == "syscall" || t.starts_with("int ") || t.starts_with("in ") || t.starts_with("out ") {
            return true;
        }
        let Some(rest) = t.strip_prefix("call ") else { continue };
        let target = rest.trim();
        if target.starts_with("B_") || target.starts_with("CB_") {
            continue;
        }
        if PURE_RUNTIME.iter().any(|p| target.starts_with(p)) || target.starts_with("rt_error_") || target.starts_with("rt_drop") {
            continue;
        }
        return true;
    }
    false
}

/// Which units are kept. `roots` are texts whose mentions are always kept.
pub fn reachable(units: &[Unit], roots: &[&str]) -> Vec<bool> {
    let mut at: HashMap<&str, usize> = HashMap::new();
    for (i, u) in units.iter().enumerate() {
        for d in &u.defs {
            at.insert(d.as_str(), i);
        }
    }
    let refs: Vec<Vec<usize>> = units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let mut v: Vec<usize> = idents(&u.text).filter_map(|t| at.get(t).copied()).filter(|j| *j != i).collect();
            v.sort_unstable();
            v.dedup();
            v
        })
        .collect();
    let mut keep = vec![false; units.len()];
    let mut work: Vec<usize> = Vec::new();
    let mark = |i: usize, keep: &mut Vec<bool>, work: &mut Vec<usize>| {
        if !keep[i] {
            keep[i] = true;
            work.push(i);
        }
    };
    for r in roots {
        for t in idents(r) {
            if let Some(&i) = at.get(t) {
                if units[i].kind != Kind::Init {
                    mark(i, &mut keep, &mut work);
                }
            }
        }
    }
    // What each initialiser can reach, without going through other initialisers: its effects are
    // those of everything in that set, and the globals in it are what it may set.
    let closure = |start: usize| -> Vec<usize> {
        let mut seen = vec![false; units.len()];
        let mut stack = vec![start];
        seen[start] = true;
        let mut out = Vec::new();
        while let Some(i) = stack.pop() {
            out.push(i);
            for &j in &refs[i] {
                if !seen[j] && units[j].kind != Kind::Init {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        out
    };
    let mut reach_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, u) in units.iter().enumerate() {
        if u.kind == Kind::Init {
            let c = closure(i);
            if c.iter().any(|&j| unit_has_effects(&units[j].text)) {
                mark(i, &mut keep, &mut work);
            }
            reach_of.insert(i, c);
        }
    }
    loop {
        while let Some(i) = work.pop() {
            for &j in &refs[i] {
                if units[j].kind != Kind::Init {
                    mark(j, &mut keep, &mut work);
                }
            }
        }
        // an initialiser is needed when something kept reads a global it sets
        let mut grew = false;
        for (i, u) in units.iter().enumerate() {
            if u.kind == Kind::Init && !keep[i] && reach_of[&i].iter().any(|&j| j != i && keep[j] && units[j].defs.iter().any(|d| d.starts_with("G_"))) {
                mark(i, &mut keep, &mut work);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    keep
}

/// Splits the text of the data units (`dq <immortal>, 0` header, label line, `db` lines) into
/// one unit per header; a line before the first header forms its own unit.
pub fn data_units(lines: &[String]) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    for l in lines {
        let new = l.starts_with("dq ") || units.is_empty();
        if new {
            units.push(Unit { text: String::new(), defs: Vec::new(), kind: Kind::Plain });
        }
        let u = units.last_mut().unwrap();
        u.text.push_str(l);
        u.text.push('\n');
        if !l.starts_with(' ') && !l.starts_with("dq ") {
            if let Some(name) = l.split_whitespace().next() {
                u.defs.push(name.to_string());
            }
        }
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str, defs: &[&str]) -> Unit {
        Unit { text: text.to_string(), defs: defs.iter().map(|d| d.to_string()).collect(), kind: Kind::Plain }
    }

    #[test]
    fn keeps_what_the_roots_reach() {
        let units = vec![plain("B_a proc\n call B_b\nB_a endp", &["B_a"]), plain("B_b proc\nB_b endp", &["B_b"]), plain("B_c proc\nB_c endp", &["B_c"])];
        let keep = reachable(&units, &["call B_a"]);
        assert_eq!(keep, vec![true, true, false]);
    }

    #[test]
    fn an_initialiser_without_effects_is_dropped_unless_its_global_is_read() {
        let init = Unit { text: "B_init_m proc\n mov qword ptr G_m_x, rax\nB_init_m endp".into(), defs: vec!["B_init_m".into()], kind: Kind::Init };
        let units = vec![init, plain("G_m_x dq 0", &["G_m_x"]), plain("B_m_f proc\n mov rax, qword ptr G_m_x\nB_m_f endp", &["B_m_f"])];
        assert_eq!(reachable(&units, &["call B_init_m"]), vec![false, false, false]);
        assert_eq!(reachable(&units, &["call B_init_m\n call B_m_f"]), vec![true, true, true]);
    }

    #[test]
    fn an_initialiser_that_reaches_an_effect_is_kept() {
        let init = Unit { text: "B_init_m proc\n call B_m_f\nB_init_m endp".into(), defs: vec!["B_init_m".into()], kind: Kind::Init };
        let units = vec![init, plain("B_m_f proc\n call rt_write_out\nB_m_f endp", &["B_m_f"])];
        assert_eq!(reachable(&units, &[]), vec![true, true]);
    }

    #[test]
    fn an_initialiser_that_only_computes_is_dropped() {
        let init = Unit { text: "B_init_m proc\n call B_m_f\nB_init_m endp".into(), defs: vec!["B_init_m".into()], kind: Kind::Init };
        let units = vec![init, plain("B_m_f proc\n call rt_list_new\nB_m_f endp", &["B_m_f"])];
        assert_eq!(reachable(&units, &[]), vec![false, false]);
    }

    #[test]
    fn runtime_calls_are_not_effects() {
        assert!(!unit_has_effects("    call rt_list_new\n    call rt_kind_intern\n    call B_x_f"));
        assert!(unit_has_effects("    call rt_write_out"));
        assert!(unit_has_effects("    syscall"));
        assert!(unit_has_effects("    call qword ptr GOT_puts"));
        assert!(unit_has_effects("    call rax"));
    }

    #[test]
    fn data_lines_group_by_header() {
        let lines: Vec<String> = ["dq 4611686018427387904, 0", "S_0 dq 2", "    db 1,2,0", "dq 4611686018427387904, 0", "S_1 dq 1", "    db 3,0"].iter().map(|s| s.to_string()).collect();
        let u = data_units(&lines);
        assert_eq!(u.len(), 2);
        assert_eq!(u[1].defs, vec!["S_1".to_string()]);
    }
}
