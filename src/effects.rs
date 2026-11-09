//! The effects of a package (spec `package-effects`): whether its code contains an `unsafe` block,
//! an `extern` declaration or an `asm` block, and which libraries the `extern`s name. It tells the
//! person who adds a dependency what that code can do to memory safety and to the machine, which
//! a matching hash alone does not.

use crate::ast::*;
use crate::comptime::walk::{walk_block, Visit};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Effects {
    pub unsafe_code: bool,
    pub asm: bool,
    /// The libraries named by `#[link("...")]` on `extern` declarations; `"?"` for one with none.
    pub externs: BTreeSet<String>,
}

impl Effects {
    pub fn is_none(&self) -> bool {
        !self.unsafe_code && !self.asm && self.externs.is_empty()
    }

    pub fn merge(&mut self, o: &Effects) {
        self.unsafe_code |= o.unsafe_code;
        self.asm |= o.asm;
        self.externs.extend(o.externs.iter().cloned());
    }

    /// `none`, or the effects in a fixed order: `unsafe, extern (user32, gdi32), asm`.
    pub fn show(&self) -> String {
        if self.is_none() {
            return "none".to_string();
        }
        let mut parts = Vec::new();
        if self.unsafe_code {
            parts.push("unsafe".to_string());
        }
        if !self.externs.is_empty() {
            let libs: Vec<&str> = self.externs.iter().map(|s| s.as_str()).filter(|s| *s != "?").collect();
            parts.push(if libs.is_empty() { "extern".to_string() } else { format!("extern ({})", libs.join(", ")) });
        }
        if self.asm {
            parts.push("asm".to_string());
        }
        parts.join(", ")
    }

    /// The effects `self` has that `old` did not, as words (`unsafe`, `extern (user32)`, `asm`).
    pub fn gained_over(&self, old: &Effects) -> Vec<String> {
        let mut v = Vec::new();
        if self.unsafe_code && !old.unsafe_code {
            v.push("unsafe".to_string());
        }
        let libs: Vec<&String> = self.externs.difference(&old.externs).collect();
        if !libs.is_empty() {
            v.push(format!("extern ({})", libs.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
        }
        if self.asm && !old.asm {
            v.push("asm".to_string());
        }
        v
    }
}

struct Finder<'a> {
    out: &'a mut Effects,
}

impl Visit for Finder<'_> {
    fn stmt(&mut self, s: &mut Stmt) -> Result<Option<Vec<Stmt>>, String> {
        match &s.kind {
            StmtKind::Unsafe(_) => self.out.unsafe_code = true,
            StmtKind::Asm(_) => self.out.asm = true,
            _ => {}
        }
        Ok(None)
    }
}

fn link_of(d: &Decl) -> String {
    for a in &d.attrs {
        if a.name == "link" {
            if let Some(Expr { kind: ExprKind::Text(parts), .. }) = a.args.first() {
                if let Some(TextPart::Lit(l)) = parts.first() {
                    return l.clone();
                }
            }
        }
    }
    "?".to_string()
}

/// The effects of the declarations for which `keep` is true. A function body is looked at for
/// `unsafe` and `asm` blocks; an `extern` declaration counts as such.
pub fn of_decls(decls: &[Decl], keep: &dyn Fn(&Decl) -> bool) -> Effects {
    let mut out = Effects::default();
    for d in decls {
        if !keep(d) {
            continue;
        }
        match &d.kind {
            DeclKind::Func(f) => {
                let mut body = f.body.clone();
                let mut finder = Finder { out: &mut out };
                let _ = walk_block(&mut body, &mut finder);
            }
            DeclKind::Extern(_) => {
                out.externs.insert(link_of(d));
            }
            _ => {}
        }
    }
    out
}

/// Every `.zila` file in a package directory and its sub-packages, tests left out.
fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')) {
                sources(&p, out);
            }
        } else if p.extension().and_then(|x| x.to_str()) == Some("zila") && !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("_test.zila")) {
            out.push(p);
        }
    }
}

/// The effects of a whole package directory (a dependency in the cache). Files that do not parse
/// are skipped: the build reports them.
pub fn of_dir(dir: &Path) -> Effects {
    let mut files = Vec::new();
    sources(dir, &mut files);
    let mut out = Effects::default();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let Ok(file) = crate::parse_source(&text) else { continue };
        out.merge(&of_decls(&file.decls, &|_| true));
    }
    out
}

/// The first file of a package directory (relative to it) that has the effect `kind`
/// (`"unsafe"`, `"extern"` or `"asm"`), for the error of a policy that forbids it.
pub fn first_with(dir: &Path, kind: &str) -> Option<String> {
    let mut files = Vec::new();
    sources(dir, &mut files);
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let Ok(file) = crate::parse_source(&text) else { continue };
        let e = of_decls(&file.decls, &|_| true);
        let has = match kind {
            "unsafe" => e.unsafe_code,
            "asm" => e.asm,
            _ => !e.externs.is_empty(),
        };
        if has {
            return Some(f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().replace('\\', "/"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eff(src: &str) -> Effects {
        let f = crate::parse_source(src).unwrap();
        of_decls(&f.decls, &|_| true)
    }

    #[test]
    fn a_pure_package_has_none() {
        assert_eq!(eff("package p\npub func f(x: int) -> int\n    return x + 1\nend\n").show(), "none");
    }

    #[test]
    fn unsafe_and_asm_blocks_are_found_inside_other_blocks() {
        let e = eff("package p\nfunc f()\n    if true\n        unsafe\n            let x = 1\n        end\n    end\nend\n");
        assert!(e.unsafe_code && !e.asm);
        let e = eff("package p\nfunc f()\n    asm\n        nop\n    end\nend\n");
        assert!(e.asm);
        assert_eq!(e.show(), "asm");
    }

    #[test]
    fn extern_names_its_library() {
        let e = eff("package p\n#[link(\"user32\")]\nextern func MessageBeep(t: u32) -> i32\n");
        assert_eq!(e.show(), "extern (user32)");
    }

    #[test]
    fn gained_effects_are_named() {
        let old = Effects::default();
        let mut new = Effects { unsafe_code: true, ..Default::default() };
        new.externs.insert("ws2_32".into());
        assert_eq!(new.gained_over(&old), vec!["unsafe".to_string(), "extern (ws2_32)".to_string()]);
        assert!(old.gained_over(&new).is_empty());
    }
}
