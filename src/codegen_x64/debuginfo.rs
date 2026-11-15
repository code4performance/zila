//! Debug information (`--debug`): what the debugger needs to map machine code to source and to
//! read values, written next to the executable as `<name>.zdbg` (JSON).
//!
//! While the code is generated (only with `--debug`) the compiler
//! - writes a note `;@L<module>:<line>` before the code of each statement and `;@E<symbol>` at the
//!   end of each procedure; the assembler records the address each note stands at and the peephole
//!   pass looks past notes, so the code is the same as without them;
//! - records, for each function, its parameters and locals with the type, where they live and
//!   the lines they are visible on.
//!
//! The file then holds: `functions` (name, module, symbol, address range, declaration line, locals),
//! `lines` (address, module, line), `types` (descriptors of records and enums, by key), `files`
//! (the files of each package of the project, to turn a line of a package into a file and line) and
//! the base address the addresses were assembled for. Descriptors name the layouts the code
//! generator uses; the debugger reads memory with them, and what it cannot read it shows as
//! unavailable.

use super::*;
use crate::lsp::Json;

/// A parameter or local of a function being compiled.
pub(super) struct DbgLocal {
    name: String,
    ty: Ty,
    off: i32,
    reg: Option<&'static str>,
    /// A `ref` list or map: the slot holds the address of the caller's slot.
    ind: bool,
    /// A `ref` parameter.
    by_ref: bool,
    param: bool,
    first_line: u32,
    last_line: u32,
}

/// A function whose code has been generated.
pub(super) struct DbgFunc {
    name: String,
    module: String,
    sym: String,
    line: u32,
    /// The return label of its frame: only that frame's locals are recorded (a closure has its own).
    label: String,
    locals: Vec<DbgLocal>,
    /// Indexes into `locals` where the open scopes began.
    scopes: Vec<usize>,
}

/// The compiler's notes for the debugger, ready to be joined with the addresses.
pub struct DebugInfo {
    functions: Vec<(String, String, String, u32, Json)>,
    types: Json,
    modules: Vec<String>,
    target: &'static str,
}

fn obj(v: Vec<(&str, Json)>) -> Json {
    Json::Obj(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn num(n: impl Into<f64>) -> Json {
    Json::Num(n.into())
}

impl Compiler {
    /// Starts the notes of a function (`gen_func` calls this once its frame exists).
    pub(super) fn dbg_begin(&mut self, name: &str, module: &str, sym: &str) {
        if !self.debug {
            return;
        }
        self.dbg_cur = Some(DbgFunc { name: name.to_string(), module: module.to_string(), sym: sym.to_string(), line: self.line, label: self.fs.ret_label.clone(), locals: Vec::new(), scopes: Vec::new() });
    }

    /// The note that the code of the statement on `line` starts here.
    pub(super) fn dbg_note(&mut self, line: u32) {
        if self.debug && line > 0 {
            let note = format!(";@L{}:{}", self.cur_mod, line);
            self.fs.code.push(note);
        }
    }

    /// Ends the notes of the function: its locals are visible up to the line it ended on.
    pub(super) fn dbg_end(&mut self) {
        if let Some(mut f) = self.dbg_cur.take() {
            for l in f.locals.iter_mut().filter(|l| l.last_line == 0) {
                l.last_line = self.line;
            }
            self.dbg_funcs.push(f);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn dbg_local(&mut self, name: &str, ty: &Ty, off: i32, reg: Option<&'static str>, ind: bool, by_ref: bool, param: bool) {
        let line = self.line;
        let label = self.fs.ret_label.clone();
        if let Some(f) = self.dbg_cur.as_mut().filter(|f| f.label == label) {
            f.locals.push(DbgLocal { name: name.to_string(), ty: ty.clone(), off, reg, ind, by_ref, param, first_line: if param { 0 } else { line }, last_line: 0 });
        }
    }

    pub(super) fn dbg_scope_open(&mut self) {
        let label = self.fs.ret_label.clone();
        if let Some(f) = self.dbg_cur.as_mut().filter(|f| f.label == label) {
            f.scopes.push(f.locals.len());
        }
    }

    pub(super) fn dbg_scope_close(&mut self) {
        let (label, line) = (self.fs.ret_label.clone(), self.line);
        if let Some(f) = self.dbg_cur.as_mut().filter(|f| f.label == label) {
            if let Some(from) = f.scopes.pop() {
                for l in f.locals[from..].iter_mut().filter(|l| l.last_line == 0) {
                    l.last_line = line;
                }
            }
        }
    }

    /// The descriptor of a type: how a debugger reads a value of it. Records and enums go in `types`
    /// (by key) so that a type that contains itself is finite.
    fn dbg_type(&self, t: &Ty, types: &mut Vec<(String, Json)>) -> Json {
        let inner = |this: &Self, t: &Ty, types: &mut Vec<(String, Json)>| this.dbg_type(t, types);
        match t {
            Ty::Void => obj(vec![("kind", Json::Str("void".into()))]),
            Ty::Nil => obj(vec![("kind", Json::Str("nil".into()))]),
            Ty::Bool => obj(vec![("kind", Json::Str("bool".into()))]),
            Ty::Int { bits, signed } => obj(vec![("kind", Json::Str("int".into())), ("bits", num(*bits)), ("signed", Json::Bool(*signed))]),
            Ty::Float { bits } => obj(vec![("kind", Json::Str("float".into())), ("bits", num(*bits))]),
            Ty::Text => obj(vec![("kind", Json::Str("text".into()))]),
            Ty::Char => obj(vec![("kind", Json::Str("char".into()))]),
            Ty::Rec(key) => {
                if !types.iter().any(|(k, _)| k == key) {
                    // reserve the key first: a record can contain itself
                    types.push((key.clone(), Json::Null));
                    let info = self.tp.records.get(key).cloned();
                    let fields: Vec<Json> = info
                        .iter()
                        .flat_map(|i| i.fields.iter())
                        .map(|(n, ft, off)| obj(vec![("name", Json::Str(n.clone())), ("offset", num(*off)), ("type", inner(self, ft, types))]))
                        .collect();
                    let size = info.as_ref().map_or(0, |i| i.size);
                    let rc = self.tp.rc_recs.contains(key);
                    let d = obj(vec![("kind", Json::Str("record".into())), ("name", Json::Str(display_name(key))), ("size", num(size)), ("shared", Json::Bool(rc)), ("fields", Json::Arr(fields))]);
                    if let Some(slot) = types.iter_mut().find(|(k, _)| k == key) {
                        slot.1 = d;
                    }
                }
                obj(vec![("kind", Json::Str("record".into())), ("ref", Json::Str(key.clone()))])
            }
            Ty::Enum(key) => {
                if !types.iter().any(|(k, _)| k == key) {
                    types.push((key.clone(), Json::Null));
                    let info = self.tp.enums.get(key).cloned();
                    let variants: Vec<Json> = info
                        .iter()
                        .flat_map(|i| i.variants.iter().enumerate())
                        .map(|(tag, (n, tys))| {
                            let fields: Vec<Json> = tys.iter().map(|ft| inner(self, ft, types)).collect();
                            obj(vec![("name", Json::Str(n.clone())), ("tag", num(tag as f64)), ("fields", Json::Arr(fields))])
                        })
                        .collect();
                    let (size, payload) = info.as_ref().map_or((0, false), |i| (i.size, i.payload));
                    let d = obj(vec![("kind", Json::Str("enum".into())), ("name", Json::Str(display_name(key))), ("size", num(size)), ("payload", Json::Bool(payload)), ("variants", Json::Arr(variants))]);
                    if let Some(slot) = types.iter_mut().find(|(k, _)| k == key) {
                        slot.1 = d;
                    }
                }
                obj(vec![("kind", Json::Str("enum".into())), ("ref", Json::Str(key.clone()))])
            }
            Ty::List(el) => obj(vec![("kind", Json::Str("list".into())), ("element", inner(self, el, types))]),
            Ty::Map(k, v) => obj(vec![("kind", Json::Str("map".into())), ("key", inner(self, k, types)), ("value", inner(self, v, types))]),
            Ty::Ptr(el) => obj(vec![("kind", Json::Str("pointer".into())), ("target", inner(self, el, types))]),
            Ty::Rc(el) => obj(vec![("kind", Json::Str("rc".into())), ("target", inner(self, el, types))]),
            Ty::Weak(el) => obj(vec![("kind", Json::Str("weak".into())), ("target", inner(self, el, types))]),
            Ty::Result(el) => obj(vec![("kind", Json::Str("result".into())), ("value", inner(self, el, types))]),
            Ty::Chan(el) => obj(vec![("kind", Json::Str("channel".into())), ("element", inner(self, el, types))]),
            Ty::Future(el) => obj(vec![("kind", Json::Str("future".into())), ("value", inner(self, el, types))]),
            Ty::Func { .. } => obj(vec![("kind", Json::Str("function".into()))]),
            Ty::Err => obj(vec![("kind", Json::Str("error".into()))]),
            Ty::Kind => obj(vec![("kind", Json::Str("errorkind".into()))]),
            Ty::Param { .. } => obj(vec![("kind", Json::Str("unknown".into()))]),
        }
    }

    /// The notes of every function generated, as the file will hold them (without addresses).
    pub fn debug_info(&self) -> Option<DebugInfo> {
        if !self.debug {
            return None;
        }
        let mut types: Vec<(String, Json)> = Vec::new();
        let mut functions = Vec::new();
        for f in &self.dbg_funcs {
            let locals: Vec<Json> = f
                .locals
                .iter()
                .map(|l| {
                    let loc = match l.reg {
                        Some(r) => obj(vec![("kind", Json::Str("register".into())), ("register", Json::Str(r.to_string())), ("slot", num(l.off))]),
                        None => obj(vec![("kind", Json::Str("frame".into())), ("offset", num(l.off))]),
                    };
                    obj(vec![
                        ("name", Json::Str(l.name.clone())),
                        ("type", self.dbg_type(&l.ty, &mut types)),
                        ("location", loc),
                        ("indirect", Json::Bool(l.ind)),
                        ("ref", Json::Bool(l.by_ref)),
                        ("parameter", Json::Bool(l.param)),
                        ("firstLine", num(l.first_line)),
                        ("lastLine", num(l.last_line)),
                    ])
                })
                .collect();
            functions.push((f.name.clone(), f.module.clone(), f.sym.clone(), f.line, Json::Arr(locals)));
        }
        types.sort_by(|a, b| a.0.cmp(&b.0));
        let modules = self.modules.iter().map(|m| m.name.clone()).collect();
        let target = match self.target {
            Target::Linux => "linux",
            Target::Windows => "windows",
            Target::Bare => "bare",
        };
        Some(DebugInfo { functions, types: Json::Obj(types), modules, target })
    }
}

impl DebugInfo {
    /// The file: the notes joined with the addresses the assembler gave. `marks` and `symbols` are
    /// the assembler's; `base` is the address the image was assembled for.
    pub fn to_json(&self, marks: &[(String, u64)], symbols: &HashMap<String, u64>, base: u64) -> Json {
        let ends: HashMap<&str, u64> = marks.iter().filter_map(|(n, a)| n.strip_prefix('E').map(|s| (s, *a))).collect();
        let functions: Vec<Json> = self
            .functions
            .iter()
            .filter_map(|(name, module, sym, line, locals)| {
                let start = *symbols.get(sym)?;
                let end = *ends.get(sym.as_str())?;
                Some(obj(vec![
                    ("name", Json::Str(name.clone())),
                    ("module", Json::Str(module.clone())),
                    ("symbol", Json::Str(sym.clone())),
                    ("start", num(start as f64)),
                    ("end", num(end as f64)),
                    ("line", num(*line)),
                    ("locals", locals.clone()),
                ]))
            })
            .collect();
        // the table of lines, in address order. A statement that makes no code (a constant
        // declaration) stands at the address of the next one: both rows are kept, and a debugger
        // that looks up an address takes the last row there, the statement that has the code.
        let mut lines: Vec<(u64, &str, u32)> = marks
            .iter()
            .filter_map(|(n, a)| {
                let rest = n.strip_prefix('L')?;
                let (module, line) = rest.rsplit_once(':')?;
                Some((*a, module, line.parse().ok()?))
            })
            .collect();
        lines.sort_by_key(|l| l.0);
        let lines: Vec<Json> = lines.iter().map(|(a, m, l)| Json::Arr(vec![num(*a as f64), Json::Str(m.to_string()), num(*l)])).collect();
        let mut files = Vec::new();
        for m in &self.modules {
            if let Some(list) = crate::package::module_files(m) {
                let rows: Vec<Json> = list.iter().map(|(p, start, n)| obj(vec![("path", Json::Str(p.to_string_lossy().to_string())), ("start", num(*start)), ("lines", num(*n))])).collect();
                files.push((m.clone(), Json::Arr(rows)));
            }
        }
        let abort = symbols.get("rt_abort").map_or(Json::Null, |a| num(*a as f64));
        obj(vec![
            ("format", num(1)),
            ("abort", abort),
            ("target", Json::Str(self.target.into())),
            ("base", num(base as f64)),
            ("functions", Json::Arr(functions)),
            ("lines", Json::Arr(lines)),
            ("types", self.types.clone()),
            ("files", Json::Obj(files)),
        ])
    }
}

/// Where the debug-information file of the executable `exe` goes: the same name with `.zdbg`.
pub fn debug_info_path(exe: &str) -> std::path::PathBuf {
    std::path::Path::new(exe).with_extension("zdbg")
}
