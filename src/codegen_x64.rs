//! Native x86-64 back end for Brasa (Windows, Win64 ABI).
//!
//! Brasa source -> x86-64 assembly (MASM syntax) -> `ml64` + `link` -> PE .exe.
//! No C, no LLVM, no C runtime: the executable only imports from the
//! Windows DLLs the program uses (kernel32 always; user32/gdi32 via `#[link]`).
//!
//! Value model
//! - Every value is one 64-bit word. Integers are kept sign/zero-extended
//!   to 64 bits according to their declared type.
//! - Heap objects (text, records, lists, closures, enums with data, results,
//!   errors, channels) have a 16-byte header `[refcount][drop fn]` in front
//!   of the pointer and are freed by reference counting. Literals and
//!   function values are static and immortal. Cycles leak.
//! - Ownership: every expression that yields a heap value yields an *owned*
//!   reference (+1). Variables, fields and list slots own what they hold,
//!   arguments are handed over to the callee, and scopes release their
//!   locals when they end (including on `return`, `break`, `continue`, `try`).
//!   Reading a local that is only inspected (field access, comparisons, ...)
//!   borrows it without touching the count.
//! - `text`: `[len: u64][bytes...][0]` (always NUL-terminated, so `c.cstr(t)`
//!   is just `t + 8`).
//! - records: C layout (declaration order, natural alignment). Value
//!   semantics: a record read from a variable/field/list is deep-copied when
//!   it is stored somewhere else. `ref` parameters and receivers share it.
//! - `[]T`: `[len][cap][data*][elements_are_heap]`, shared by reference.
//! - enums: a number when no variant has data, otherwise `[tag][fields...]`.
//! - `T or error`: a box `[error][value]`; `error` is `[message]`.
//! - `func(...)` values: `[code][env]` (a closure; captures by value).
//!   `extern "C" func(...)` values are raw code addresses (C callbacks).
//!
//! Every Brasa function uses the Win64 calling convention and only volatile
//! registers, so any top-level function can be handed to C as a callback.

use crate::ast::*;
use crate::parser::parse_source;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

type R<T> = Result<T, String>;

/// Standard library modules written in Brasa and embedded in the compiler.
pub fn std_source(name: &str) -> Option<&'static str> {
    match name {
        "ui" => Some(include_str!("../std/ui.br")),
        "task" => Some(include_str!("../std/task.br")),
        "future" => Some(include_str!("../std/future.br")),
        "fs" => Some(include_str!("../std/fs.br")),
        "net" => Some(include_str!("../std/net.br")),
        "sys" => Some(include_str!("../std/sys.br")),
        "ws" => Some(include_str!("../std/ws.br")),
        "float" => Some(include_str!("../std/float.br")),
        "math" => Some(include_str!("../std/math.br")),
        "text" => Some(include_str!("../std/text.br")),
        "list" => Some(include_str!("../std/list.br")),
        "format" => Some(include_str!("../std/format.br")),
        "set" => Some(include_str!("../std/set.br")),
        "sorted" => Some(include_str!("../std/sorted.br")),
        "bufio" => Some(include_str!("../std/bufio.br")),
        _ => None,
    }
}

const IMMORTAL: &str = "4611686018427387904";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Ty {
    Void,
    Nil,
    Bool,
    Int { bits: u8, signed: bool },
    /// `f32` (bits 32) and `f64`/`float` (bits 64). Held in rax as the raw IEEE bits.
    Float { bits: u8 },
    Text,
    Rec(String),
    Enum(String),
    List(Box<Ty>),
    /// `Map<K, V>`: `K` is `text` or an integer type.
    Map(Box<Ty>, Box<Ty>),
    Ptr(Box<Ty>),
    Func { params: Vec<Ty>, ret: Box<Ty>, ext: bool },
    Err,
    /// `T or error` (`T` is `Void` for `-> error`).
    Result(Box<Ty>),
    Chan(Box<Ty>),
    /// `Future<T>`: a handle to the result of an `async func` (`T` may be `Void`).
    Future(Box<Ty>),
    /// A type parameter while a generic body is checked once: nothing is known about it but its bound
    /// (the qualified name of an interface).
    Param { name: String, bound: Option<String> },
}

impl Ty {
    fn i64() -> Ty {
        Ty::Int { bits: 64, signed: true }
    }
    fn is_int(&self) -> bool {
        matches!(self, Ty::Int { .. })
    }
    fn is_float(&self) -> bool {
        matches!(self, Ty::Float { .. })
    }
    fn is_unsigned(&self) -> bool {
        matches!(self, Ty::Int { signed: false, .. } | Ty::Ptr(_))
    }
    /// Size in memory (record fields, pointer targets).
    fn size(&self) -> u32 {
        match self {
            Ty::Bool => 1,
            Ty::Int { bits, .. } => *bits as u32 / 8,
            Ty::Float { bits } => *bits as u32 / 8,
            _ => 8,
        }
    }
    fn nullable(&self) -> bool {
        matches!(self, Ty::Nil | Ty::Text | Ty::Rec(_) | Ty::List(_) | Ty::Map(..) | Ty::Ptr(_) | Ty::Func { .. } | Ty::Chan(_) | Ty::Param { .. })
    }
    fn show(&self) -> String {
        match self {
            Ty::Void => "nothing".into(),
            Ty::Nil => "nil".into(),
            Ty::Bool => "bool".into(),
            Ty::Int { bits, signed } => format!("{}{}", if *signed { "i" } else { "u" }, bits),
            Ty::Float { bits: 32 } => "f32".into(),
            Ty::Float { .. } => "float".into(),
            Ty::Text => "text".into(),
            Ty::Rec(n) | Ty::Enum(n) => display_name(n),
            Ty::Param { name, .. } => name.clone(),
            Ty::List(t) => format!("[]{}", t.show()),
            Ty::Map(k, v) => format!("Map<{}, {}>", k.show(), v.show()),
            Ty::Ptr(t) => format!("*{}", t.show()),
            Ty::Err => "error".into(),
            Ty::Result(t) => {
                if **t == Ty::Void {
                    "error".into()
                } else {
                    format!("{} or error", t.show())
                }
            }
            Ty::Chan(t) => format!("Channel<{}>", t.show()),
            Ty::Future(t) => {
                if **t == Ty::Void {
                    "Future<nothing>".into()
                } else {
                    format!("Future<{}>", t.show())
                }
            }
            Ty::Func { params, ret, ext } => {
                let ps: Vec<String> = params.iter().map(|p| p.show()).collect();
                let r = if **ret == Ty::Void { String::new() } else { format!(" -> {}", ret.show()) };
                format!("{}func({}){}", if *ext { "extern " } else { "" }, ps.join(", "), r)
            }
        }
    }
}

/// Can a value of type `got` be stored where `want` is expected?
fn compatible(want: &Ty, got: &Ty) -> bool {
    match (want, got) {
        (a, b) if a == b => true,
        (Ty::Int { .. }, Ty::Int { .. }) => true,
        (Ty::Int { .. }, Ty::Bool) | (Ty::Bool, Ty::Int { .. }) => true,
        (w, Ty::Nil) => w.nullable(),
        (Ty::Ptr(_), Ty::Ptr(_)) => true,
        (Ty::Ptr(_), Ty::Int { .. }) | (Ty::Int { .. }, Ty::Ptr(_)) => true,
        // an `extern` function value is a raw code address (GetProcAddress, WSAIoctl, ...)
        (Ty::Func { ext: true, .. }, Ty::Int { .. }) | (Ty::Int { .. }, Ty::Func { ext: true, .. }) => true,
        (Ty::Ptr(a), Ty::Rec(b)) => matches!(&**a, Ty::Rec(x) if x == b),
        (Ty::List(a), Ty::List(b)) => compatible(a, b),
        (Ty::Map(ak, av), Ty::Map(bk, bv)) => ak == bk && compatible(av, bv),
        (Ty::Result(a), Ty::Result(b)) => compatible(a, b),
        (Ty::Future(a), Ty::Future(b)) => compatible(a, b),
        (Ty::Rec(a), Ty::Future(_)) if a == "future.Future" => true,
        (Ty::Func { params: p1, ret: r1, ext: e1 }, Ty::Func { params: p2, ret: r2, ext: e2 }) => {
            e1 == e2 && p1.len() == p2.len() && compatible(r1, r2)
        }
        _ => false,
    }
}

#[derive(Clone)]
struct RecInfo {
    size: u32,
    fields: Vec<(String, Ty, u32)>,
}

#[derive(Clone)]
struct EnumInfo {
    variants: Vec<(String, Vec<Ty>)>,
    payload: bool,
    size: u32,
}

#[derive(Clone)]
struct ParamInfo {
    name: String,
    ty: Ty,
    by_ref: bool,
    default: Option<Expr>,
}

#[derive(Clone)]
struct Sig {
    sym: String,
    module: String,
    params: Vec<ParamInfo>,
    ret: Ty,
    /// Brasa variadic: last parameter `name: ...T` collects a list.
    variadic: bool,
    c_variadic: bool,
    is_extern: bool,
    /// Receiver type for methods.
    receiver: Option<Ty>,
}

impl Sig {
    fn func_ty(&self, ext: bool) -> Ty {
        Ty::Func {
            params: self.params.iter().map(|p| p.ty.clone()).collect(),
            ret: Box::new(self.ret.clone()),
            ext,
        }
    }
}

struct Global {
    sym: String,
    ty: Ty,
    konst: Option<i64>,
    mutable: bool,
}

#[derive(Clone)]
struct Local {
    off: i32,
    ty: Ty,
    mutable: bool,
    captured: bool,
    /// Callee-saved register holding the variable instead of its slot.
    reg: Option<&'static str>,
    /// A parameter of an inlined call whose argument is a literal: the value read from the
    /// variable (the raw bits, as `rax` would hold them), with no slot behind it.
    imm: Option<i64>,
}

#[derive(Default, Clone)]
struct Scope {
    vars: HashMap<String, Local>,
    /// Slots holding heap values this scope must release when it ends.
    owned: Vec<(i32, Ty)>,
}

#[derive(Clone)]
struct LoopCtx {
    cont: String,
    brk: String,
    /// Scopes with an index >= `depth` are inside the loop body.
    depth: usize,
}

#[derive(Clone)]
struct FnState {
    code: Vec<String>,
    scopes: Vec<Scope>,
    next: i32,
    free: Vec<i32>,
    max_args: usize,
    ret: Ty,
    ret_label: String,
    loops: Vec<LoopCtx>,
    unsafe_depth: u32,
    /// Slots holding temporaries (`c.cstr`) released at the end of the statement.
    stmt_temps: Vec<i32>,
    /// `await` is allowed here: inside an `async func` and inside `main`.
    allow_await: bool,
    /// Slot holding the list of deferred closures, and a slot to keep the return value.
    defer_list: Option<i32>,
    defer_save: Option<i32>,
    /// A statement may ignore the result of a call that can fail (the body of a deferred call).
    allow_ignored_result: bool,
    /// The function contains an inline `asm` block: its code is left exactly as written.
    has_asm: bool,
    /// Some slot of the function can own a heap value (or `defer` runs code at the end): the
    /// slots are zeroed on entry so that a release never sees garbage.
    may_own: bool,
    /// Variables kept in registers (by name) and the registers that ended up used.
    regs: HashMap<String, &'static str>,
    used_regs: Vec<&'static str>,
}

impl FnState {
    fn new(ret: Ty, ret_label: String) -> Self {
        FnState {
            code: Vec::new(),
            scopes: vec![Scope::default()],
            next: 0,
            free: Vec::new(),
            max_args: 4,
            ret,
            ret_label,
            loops: Vec::new(),
            unsafe_depth: 0,
            stmt_temps: Vec::new(),
            allow_await: false,
            defer_list: None,
            defer_save: None,
            allow_ignored_result: false,
            has_asm: false,
            may_own: false,
            regs: HashMap::new(),
            used_regs: Vec::new(),
        }
    }
}

const ARG_REGS: [&str; 4] = ["rcx", "rdx", "r8", "r9"];

/// Largest body (in expression nodes) and deepest nesting of calls compiled in place.
const INLINE_NODES: usize = 12;
const INLINE_DEPTH: usize = 4;
const INLINE_PARAMS: usize = 4;

fn plain_scalar(t: &Ty) -> bool {
    matches!(t, Ty::Int { .. } | Ty::Bool | Ty::Float { .. } | Ty::Ptr(_))
}

/// The expression of a function that calls may be compiled in place of: an ordinary function
/// of at most a few plain parameters, with a single `return <expression>` of a few operations.
fn inlinable(fd: &FuncDecl, sig: &Sig) -> Option<Expr> {
    if fd.is_async
        || !fd.generics.is_empty()
        || fd.receiver.is_some()
        || sig.variadic
        || sig.is_extern
        || sig.params.len() > INLINE_PARAMS
        || sig.params.iter().any(|p| p.by_ref || p.default.is_some() || !plain_scalar(&p.ty))
        || !plain_scalar(&sig.ret)
    {
        return None;
    }
    let [Stmt { kind: StmtKind::Return(Some(e)), .. }] = &fd.body[..] else { return None };
    (inline_size(e, &fd.name)? <= INLINE_NODES).then(|| e.clone())
}

/// Number of nodes of `e`, or `None` when it contains something that is never compiled in place.
fn inline_size(e: &Expr, me: &str) -> Option<usize> {
    Some(
        1 + match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Char(_) | ExprKind::Bool(_) | ExprKind::Ident(_) => 0,
            ExprKind::Unary { op, expr } => {
                if *op == "&" {
                    return None;
                }
                inline_size(expr, me)?
            }
            ExprKind::Binary { lhs, rhs, .. } => inline_size(lhs, me)? + inline_size(rhs, me)?,
            ExprKind::Call { callee, args } => {
                let mut n = match &callee.kind {
                    ExprKind::Ident(f) if f != me => 1,
                    ExprKind::Field { expr, safe: false, .. } if matches!(expr.kind, ExprKind::Ident(_)) => 2,
                    _ => return None,
                };
                for a in args {
                    if a.by_ref || a.spread {
                        return None;
                    }
                    n += inline_size(&a.value, me)?;
                }
                n
            }
            _ => return None,
        },
    )
}

/// Only reads variables: evaluating it cannot change a local of the caller.
fn reads_only(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Char(_) | ExprKind::Bool(_) | ExprKind::Nil | ExprKind::Ident(_) => true,
        ExprKind::Unary { op, expr } => *op != "&" && reads_only(expr),
        ExprKind::Binary { lhs, rhs, .. } => reads_only(lhs) && reads_only(rhs),
        _ => false,
    }
}

/// The raw bits a literal argument of type `ty` puts in `rax`, when it fits.
fn literal_bits(e: &Expr, ty: &Ty) -> Option<i64> {
    let int = |v: i128, bits: u8, signed: bool| -> Option<i64> {
        let (lo, hi) = if signed { (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1) } else { (0, (1i128 << bits) - 1) };
        (lo <= v && v <= hi).then(|| v as i64)
    };
    match (&e.kind, ty) {
        (ExprKind::Int(n), Ty::Int { bits, signed }) => int(*n as i128, *bits, *signed),
        (ExprKind::Unary { op: "-", expr }, Ty::Int { bits, signed }) => match &expr.kind {
            ExprKind::Int(n) => int(-(*n as i128), *bits, *signed),
            _ => None,
        },
        (ExprKind::Bool(b), Ty::Bool) => Some(*b as i64),
        (ExprKind::Float(f), Ty::Float { bits: 64 }) => Some(f.to_bits() as i64),
        (ExprKind::Float(f), Ty::Float { bits: 32 }) => Some((*f as f32).to_bits() as i64),
        _ => None,
    }
}

fn is_place(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Ident(_) | ExprKind::Field { .. } | ExprKind::Index { .. } | ExprKind::Deref(_))
}

fn int_type(name: &str) -> Option<Ty> {
    let (bits, signed) = match name {
        "i8" => (8, true),
        "i16" => (16, true),
        "i32" => (32, true),
        "i64" | "isize" | "int" => (64, true),
        "u8" | "byte" => (8, false),
        "u16" => (16, false),
        "u32" => (32, false),
        "u64" | "usize" => (64, false),
        _ => return None,
    };
    Some(Ty::Int { bits, signed })
}

fn c_type(name: &str) -> Option<Ty> {
    let (bits, signed) = match name {
        "char" | "schar" => (8, true),
        "uchar" | "void" => (8, false),
        "short" => (16, true),
        "ushort" => (16, false),
        "int" | "long" => (32, true), // Windows x64: long is 32-bit
        "uint" | "ulong" => (32, false),
        "longlong" | "ssize_t" => (64, true),
        "ulonglong" | "size_t" => (64, false),
        _ => return None,
    };
    Some(Ty::Int { bits, signed })
}

/// Key under which the methods of a receiver type are stored.
fn method_key(t: &Ty) -> String {
    match t {
        Ty::Rec(r) => r.clone(),
        other => other.show(),
    }
}

/// An identifier-safe spelling of a type name for generated symbols.
fn mangle(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            c if c.is_ascii_alphanumeric() => out.push(c),
            '[' => out.push_str("L"),
            ']' => out.push_str("R"),
            '<' => out.push_str("_o_"),
            '>' => out.push_str("_c_"),
            ',' => out.push_str("_k_"),
            '*' => out.push_str("P"),
            '.' => out.push_str("_d_"),
            _ => out.push('_'),
        }
    }
    out
}

fn sym_of(q: &str) -> String {
    let mut out = String::new();
    for c in q.chars() {
        match c {
            c if c.is_ascii_alphanumeric() || c == '_' => out.push(c),
            '.' => out.push('_'),
            '<' => out.push_str("_o_"),
            '>' => out.push_str("_c_"),
            ',' => out.push_str("_k_"),
            ' ' => {}
            '[' => out.push('L'),
            ']' => out.push('R'),
            '*' => out.push('P'),
            _ => out.push('_'),
        }
    }
    out
}

/// A qualified type name as a person writes it: the module prefixes are dropped, also inside
/// the arguments of a generic instance (`main.Stack<main.Point>` is `Stack<Point>`).
fn display_name(n: &str) -> String {
    let mut out = String::new();
    let mut token = String::new();
    for c in n.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            token.push(c);
        } else {
            out.push_str(token.rsplit('.').next().unwrap_or(""));
            token.clear();
            out.push(c);
        }
    }
    out.push_str(token.rsplit('.').next().unwrap_or(""));
    out
}

// ---------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------

struct Module {
    name: String,
    file: File,
}

/// What the generated code runs on.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    /// A Windows x64 console or GUI program.
    Windows,
    /// A freestanding kernel in 64-bit long mode (no operating system).
    Bare,
}

/// Options of a compilation.
#[derive(Clone, Copy, Debug)]
pub struct CompileOptions {
    /// Constant folding, direct operands, fused compare-and-branch, peephole cleanups and the
    /// cheaper function entry. `--no-opt` turns it off.
    pub optimize: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions { optimize: true }
    }
}

pub struct Compiler {
    target: Target,
    /// All optimizations are on (see `CompileOptions`).
    opt: bool,
    modules: Vec<Module>,
    imports: HashMap<String, HashSet<String>>,
    rec_decls: HashMap<String, (String, Vec<Field>)>,
    /// `#[shared]` records have reference semantics: never copied on assignment, capture or iteration.
    shared_recs: HashSet<String>,
    enum_decls: HashMap<String, (String, Vec<Variant>)>,
    enum_payload: HashSet<String>,
    aliases: HashMap<String, (String, Type)>,
    records: HashMap<String, RecInfo>,
    enums: HashMap<String, EnumInfo>,
    /// (module, variant name) -> (enum, index)
    variants: HashMap<(String, String), (String, usize)>,
    enum_statics: BTreeSet<(String, usize)>,
    funcs: HashMap<(String, String), Sig>,
    methods: HashMap<(String, String), Sig>,
    /// Functions whose calls are compiled in place (with `opt`): symbol -> (body, module).
    inline_bodies: HashMap<String, (Expr, String)>,
    inline_depth: usize,
    globals: HashMap<(String, String), Global>,
    libs: BTreeSet<String>,
    extern_syms: BTreeSet<String>,
    data: Vec<String>,
    procs: Vec<String>,
    strings: HashMap<String, String>,
    fn_closures: BTreeSet<String>,
    labels: usize,
    /// the next `lambda()` is the body of an async function: `await` is allowed in it
    next_lambda_await: bool,
    /// the next `lambda()` shares the records it captures instead of copying them
    next_lambda_share: bool,
    /// the next `lambda()` may ignore the result of a call that can fail
    next_lambda_ignore_result: bool,
    cur_mod: String,
    line: u32,
    fs: FnState,
    uses_ui: bool,
    // ---- generics and interfaces ----
    /// Generic functions: (module, name) -> declaration and line.
    tmpl_funcs: HashMap<(String, String), (FuncDecl, u32)>,
    /// Generic records: qualified name -> (module, declaration, `#[shared]`).
    tmpl_recs: HashMap<String, (String, TypeDecl, bool)>,
    /// Methods of generic records: qualified record name -> (module, declaration, line).
    tmpl_methods: HashMap<String, Vec<(String, FuncDecl, u32)>>,
    /// Interfaces: qualified name -> (module, method signatures).
    interfaces: HashMap<String, (String, Vec<MethodSig>)>,
    /// Type parameters bound to types while an instance is compiled.
    bindings: HashMap<String, Ty>,
    /// Instance of a generic record -> (template, type arguments).
    inst_args: HashMap<String, (String, Vec<Ty>)>,
    /// (module, function, type arguments) -> instance name.
    inst_funcs: HashMap<(String, String, String), String>,
    /// Instances waiting to be compiled.
    pending: Vec<Pending>,
    /// All signatures are known (bounds can be checked).
    declared: bool,
    /// Bound checks met before all methods were known: (type, interface, module, line).
    deferred_bounds: Vec<(Ty, String, u32)>,
    inst_count: usize,
}

struct Pending {
    module: String,
    decl: FuncDecl,
    bindings: HashMap<String, Ty>,
    line: u32,
}

/// Everything a throw-away compilation (inference, checking a generic body) may change.
struct Snapshot {
    fs: FnState,
    procs: usize,
    data: usize,
    strings: HashMap<String, String>,
    records: HashMap<String, RecInfo>,
    rec_decls: HashMap<String, (String, Vec<Field>)>,
    shared_recs: HashSet<String>,
    methods: HashMap<(String, String), Sig>,
    funcs: HashMap<(String, String), Sig>,
    inst_args: HashMap<String, (String, Vec<Ty>)>,
    inst_funcs: HashMap<(String, String, String), String>,
    pending: Vec<Pending>,
    fn_closures: BTreeSet<String>,
    bindings: HashMap<String, Ty>,
    cur_mod: String,
    line: u32,
}

impl Compiler {
    fn new(target: Target) -> Self {
        Compiler {
            target,
            opt: true,
            modules: Vec::new(),
            imports: HashMap::new(),
            rec_decls: HashMap::new(),
            shared_recs: HashSet::new(),
            enum_decls: HashMap::new(),
            enum_payload: HashSet::new(),
            aliases: HashMap::new(),
            records: HashMap::new(),
            enums: HashMap::new(),
            variants: HashMap::new(),
            enum_statics: BTreeSet::new(),
            funcs: HashMap::new(),
            methods: HashMap::new(),
            inline_bodies: HashMap::new(),
            inline_depth: 0,
            globals: HashMap::new(),
            libs: BTreeSet::new(),
            extern_syms: BTreeSet::new(),
            data: Vec::new(),
            procs: Vec::new(),
            strings: HashMap::new(),
            fn_closures: BTreeSet::new(),
            labels: 0,
            next_lambda_await: false,
            next_lambda_share: false,
            next_lambda_ignore_result: false,
            cur_mod: String::new(),
            line: 0,
            fs: FnState::new(Ty::Void, String::new()),
            uses_ui: false,
            tmpl_funcs: HashMap::new(),
            tmpl_recs: HashMap::new(),
            tmpl_methods: HashMap::new(),
            interfaces: HashMap::new(),
            bindings: HashMap::new(),
            inst_args: HashMap::new(),
            inst_funcs: HashMap::new(),
            pending: Vec::new(),
            declared: false,
            deferred_bounds: Vec::new(),
            inst_count: 0,
        }
    }

    fn err<T>(&self, msg: impl Into<String>) -> R<T> {
        Err(format!("{}.br:{}: {}", self.cur_mod, self.line, msg.into()))
    }

    fn lbl(&mut self, p: &str) -> String {
        self.labels += 1;
        format!("{}_{}", p, self.labels)
    }

    fn e(&mut self, s: impl Into<String>) {
        self.fs.code.push(format!("    {}", s.into()));
    }

    fn label(&mut self, l: &str) {
        self.fs.code.push(format!("{}:", l));
    }

    fn slot(&mut self) -> i32 {
        self.fs.next += 8;
        self.fs.next
    }

    fn temp(&mut self) -> i32 {
        match self.fs.free.pop() {
            Some(o) => o,
            None => self.slot(),
        }
    }

    fn free(&mut self, o: i32) {
        self.fs.free.push(o);
    }

    fn string_lit(&mut self, s: &str) -> String {
        if let Some(l) = self.strings.get(s) {
            return l.clone();
        }
        let l = format!("S_{}", self.strings.len());
        let mut bytes: Vec<String> = s.bytes().map(|b| b.to_string()).collect();
        bytes.push("0".into());
        self.data.push(format!("dq {}, 0", IMMORTAL));
        self.data.push(format!("{} dq {}", l, s.len()));
        for chunk in bytes.chunks(40) {
            self.data.push(format!("    db {}", chunk.join(",")));
        }
        self.strings.insert(s.to_string(), l.clone());
        l
    }

    // ---------------- ownership helpers ----------------

    fn is_rc(&self, t: &Ty) -> bool {
        match t {
            Ty::Text | Ty::Rec(_) | Ty::List(_) | Ty::Map(..) | Ty::Err | Ty::Result(_) | Ty::Chan(_) | Ty::Future(_) => true,
            Ty::Func { ext, .. } => !*ext,
            Ty::Enum(q) => self.enum_payload.contains(q),
            _ => false,
        }
    }

    /// rax keeps its value; the reference count of the object in rax grows.
    fn retain(&mut self) {
        self.e("mov rcx, rax");
        self.e("call rt_retain");
    }

    fn retain_if(&mut self, t: &Ty) {
        if self.is_rc(t) {
            self.retain();
        }
    }

    /// Releases the value in rax (rax itself is preserved).
    fn release_rax(&mut self) {
        self.e("mov rcx, rax");
        self.e("call rt_release");
    }

    /// Releases the object stored in a slot (rax is preserved).
    fn release_slot(&mut self, off: i32) {
        self.e(format!("mov rcx, qword ptr [rbp - {}]", off));
        self.e("call rt_release");
    }

    /// rax = new zeroed heap object. `drop` is a procedure name or "0".
    fn alloc(&mut self, size: u32, drop: &str) {
        self.e(format!("mov ecx, {}", size));
        if drop == "0" {
            self.e("xor edx, edx");
        } else {
            self.e(format!("lea rdx, {}", drop));
        }
        self.e("call rt_alloc");
    }

    /// Checks the key type of a `Map<K, V>` and builds the type.
    fn map_type(&self, k: Ty, v: Ty) -> R<Ty> {
        if self.target == Target::Bare {
            return self.err("maps need the Windows runtime and cannot be used in a bare-metal kernel");
        }
        if !(k == Ty::Text || k.is_int() || matches!(k, Ty::Param { .. })) {
            return self.err(format!("{} cannot be a map key; keys are text or integers", k.show()));
        }
        Ok(Ty::Map(Box::new(k), Box::new(v)))
    }

    /// rax = new empty map of type `t`.
    fn new_map(&mut self, t: &Ty) {
        let Ty::Map(k, v) = t else { unreachable!() };
        let flags = (**k == Ty::Text) as i32 | ((self.is_rc(v) as i32) << 1);
        self.e(format!("mov ecx, {}", flags));
        self.e("call rt_map_new");
    }

    fn new_list(&mut self, el: &Ty) {
        let flag = self.is_rc(el) as i32;
        self.e(format!("mov ecx, {}", flag));
        self.e("call rt_list_new");
    }

    fn rec_has_rc(&self, q: &str) -> bool {
        self.records[q].fields.iter().any(|f| self.is_rc(&f.1))
    }

    /// `func (ref x: T) drop()` runs when the last reference to a `T` goes away.
    fn drop_hook(&self, q: &str) -> Option<String> {
        self.methods.get(&(q.to_string(), "drop".to_string())).map(|s| s.sym.clone())
    }

    fn rec_drop(&self, q: &str) -> String {
        if self.rec_has_rc(q) || self.drop_hook(q).is_some() {
            format!("B_drop_{}", sym_of(q))
        } else {
            "0".into()
        }
    }

    fn enum_drop(&self, q: &str) -> String {
        let info = &self.enums[q];
        if info.variants.iter().any(|v| v.1.iter().any(|t| self.is_rc(t))) {
            format!("B_edrop_{}", sym_of(q))
        } else {
            "0".into()
        }
    }

    fn box_drop(&self, inner: &Ty) -> &'static str {
        if self.is_rc(inner) {
            "rt_drop_result_rc"
        } else {
            "rt_drop_result_plain"
        }
    }

    /// The register the plan gives to variable `name` when its type fits in one.
    fn reg_for(&mut self, name: &str, ty: &Ty) -> Option<&'static str> {
        if !self.opt || !(ty.is_int() || matches!(ty, Ty::Bool | Ty::Ptr(_))) {
            return None;
        }
        let r = *self.fs.regs.get(name)?;
        if !self.fs.used_regs.contains(&r) {
            self.fs.used_regs.push(r);
        }
        Some(r)
    }

    /// Loads variable `l` into `dst` (`rax` or `rcx`).
    fn load_local(&mut self, dst: &str, l: &Local) {
        match (l.reg, l.imm) {
            (_, Some(v)) => self.e(format!("mov {}, {}", dst, imm64(v))),
            (Some(r), _) => self.e(format!("mov {}, {}", dst, r)),
            (None, _) => self.e(format!("mov {}, qword ptr [rbp - {}]", dst, l.off)),
        }
    }

    /// Stores `rax` into variable `l`.
    fn store_local(&mut self, l: &Local) {
        match l.reg {
            Some(r) => self.e(format!("mov {}, rax", r)),
            None => self.e(format!("mov qword ptr [rbp - {}], rax", l.off)),
        }
    }

    fn declare_local(&mut self, name: &str, ty: Ty, mutable: bool) -> i32 {
        let off = self.slot();
        let rc = self.is_rc(&ty);
        let reg = self.reg_for(name, &ty);
        let sc = self.fs.scopes.last_mut().unwrap();
        sc.vars.insert(name.to_string(), Local { off, ty: ty.clone(), mutable, captured: false, reg, imm: None });
        if rc {
            sc.owned.push((off, ty));
            self.fs.may_own = true;
        }
        off
    }

    /// An unnamed slot that owns a heap value until the scope ends.
    fn hidden_local(&mut self, ty: Ty) -> i32 {
        let off = self.slot();
        if self.is_rc(&ty) {
            self.fs.scopes.last_mut().unwrap().owned.push((off, ty));
            self.fs.may_own = true;
        }
        off
    }

    /// Emits releases for the locals of scopes `down_to..` (innermost first).
    fn emit_unwind(&mut self, down_to: usize) {
        let mut offs = Vec::new();
        for i in (down_to..self.fs.scopes.len()).rev() {
            for (off, _) in self.fs.scopes[i].owned.iter().rev() {
                offs.push(*off);
            }
        }
        for o in offs {
            self.release_slot(o);
        }
    }

    /// `assert(cond)`, `assert(cond, "message")` and `assert_eq(a, b)`: when the check fails
    /// the enclosing function returns an error `line N: ...`. Written as the statements a
    /// program could have written itself.
    fn assert_stmt(&mut self, name: &str, args: &[Arg], line: u32) -> R<()> {
        if !matches!(self.fs.ret, Ty::Result(_)) {
            return self.err(format!("{} needs a function that returns an error: write `func name() -> error`", name));
        }
        let (min, max) = if name == "assert" { (1, 2) } else { (2, 2) };
        if args.len() < min || args.len() > max || args.iter().any(|a| a.name.is_some() || a.by_ref || a.spread) {
            return self.err(if name == "assert" {
                "assert takes a condition and an optional message"
            } else {
                "assert_eq takes two values"
            });
        }
        let mk = |kind: ExprKind| Expr { kind, line };
        let fail = |parts: Vec<TextPart>| {
            let call = mk(ExprKind::Call {
                callee: Box::new(mk(ExprKind::Ident("error".into()))),
                args: vec![Arg { name: None, by_ref: false, spread: false, value: mk(ExprKind::Text(parts)) }],
            });
            vec![Stmt { kind: StmtKind::Return(Some(call)), line }]
        };
        let prefix = TextPart::Lit(format!("line {}: ", line));
        let mut stmts: Vec<Stmt> = Vec::new();
        let (cond, body) = if name == "assert" {
            let mut parts = vec![prefix];
            match args.get(1).map(|a| &a.value) {
                None => parts.push(TextPart::Lit("assertion failed".into())),
                Some(Expr { kind: ExprKind::Text(p), .. }) => parts.extend(p.iter().cloned()),
                Some(other) => parts.push(TextPart::Expr(other.clone())),
            }
            (mk(ExprKind::Unary { op: "not", expr: Box::new(args[0].value.clone()) }), fail(parts))
        } else {
            for (var, arg) in [("__assert_left", &args[0]), ("__assert_right", &args[1])] {
                stmts.push(Stmt { kind: StmtKind::Var(VarDecl { name: var.into(), ty: None, value: Some(arg.value.clone()), mutable: false }), line });
            }
            let id = |n: &str| mk(ExprKind::Ident(n.into()));
            let parts = vec![
                prefix,
                TextPart::Lit("assertion failed: left ".into()),
                TextPart::Expr(id("__assert_left")),
                TextPart::Lit(", right ".into()),
                TextPart::Expr(id("__assert_right")),
            ];
            (mk(ExprKind::Binary { op: "!=", lhs: Box::new(id("__assert_left")), rhs: Box::new(id("__assert_right")) }), fail(parts))
        };
        stmts.push(Stmt { kind: StmtKind::Expr(mk(ExprKind::If { branches: vec![(cond, body)], else_body: None })), line });
        self.push_scope();
        for st in &stmts {
            self.stmt(st)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn push_scope(&mut self) {
        self.fs.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        let top = self.fs.scopes.len() - 1;
        self.emit_unwind(top);
        self.fs.scopes.pop();
    }

    /// Releases the statement temporaries registered after `mark`.
    fn flush_temps(&mut self, mark: usize) {
        while self.fs.stmt_temps.len() > mark {
            let s = self.fs.stmt_temps.pop().unwrap();
            self.release_slot(s);
            self.e(format!("mov qword ptr [rbp - {}], 0", s));
        }
    }

    /// Releases every pending temporary without forgetting it (for early exits).
    fn flush_all_temps_emit(&mut self) {
        let all: Vec<i32> = self.fs.stmt_temps.clone();
        for s in all.into_iter().rev() {
            self.release_slot(s);
        }
    }

    // ---------------- loading & declarations ----------------

    fn load(&mut self, main: &File) -> R<()> {
        let mut queue = vec![(main.package.clone(), main.clone())];
        let mut seen = HashSet::new();
        seen.insert(main.package.clone());
        while let Some((name, file)) = queue.pop() {
            let mut imps = HashSet::new();
            let mut wanted: Vec<String> = file.uses.iter().map(|u| u[0].clone()).collect();
            if file_uses_concurrency(&file) {
                if self.target == Target::Bare {
                    return Err("'spawn', 'channel', 'async' and 'await' need the task library, which needs Windows".into());
                }
                wanted.push("task".into());
                wanted.push("future".into());
            }
            if name != "float" && file_uses_floats(&file) {
                if self.target == Target::Bare {
                    return Err("floating point is not available in a bare-metal kernel".into());
                }
                wanted.push("float".into());
            }
            for m in wanted {
                if self.target == Target::Bare && (matches!(m.as_str(), "ui" | "task" | "future" | "fs" | "net" | "sys" | "ws")) {
                    return Err(format!("'{}' needs Windows and cannot be used in a bare-metal kernel", m));
                }
                imps.insert(m.clone());
                if m == "ui" {
                    self.uses_ui = true;
                }
                if !seen.contains(&m) {
                    if let Some(src) = std_source(&m) {
                        seen.insert(m.clone());
                        let f = parse_source(src).map_err(|d| format!("std/{}.br:{}:{}: {}", m, d.line, d.col, d.msg))?;
                        queue.push((m.clone(), f));
                    }
                }
            }
            self.imports.insert(name.clone(), imps);
            self.modules.push(Module { name, file });
        }
        Ok(())
    }

    fn declare(&mut self) -> R<()> {
        // Type names first, so records can refer to each other.
        let mods: Vec<(String, File)> = self.modules.iter().map(|m| (m.name.clone(), m.file.clone())).collect();
        for (mname, file) in &mods {
            self.cur_mod = mname.clone();
            for d in &file.decls {
                self.line = d.line;
                if let DeclKind::Type(td) = &d.kind {
                    let q = format!("{}.{}", mname, td.name);
                    if !td.generics.is_empty() {
                        match &td.body {
                            TypeBody::Record(_) => {
                                let shared = d.attrs.iter().any(|a| a.name == "shared");
                                self.tmpl_recs.insert(q, (mname.clone(), td.clone(), shared));
                                continue;
                            }
                            TypeBody::Enum(_) => return self.err(format!("generic enums are not supported ('{}')", td.name)),
                            TypeBody::Alias(_) => return self.err(format!("generic type aliases are not supported ('{}')", td.name)),
                            TypeBody::Interface(_) => return self.err(format!("generic interfaces are not supported ('{}')", td.name)),
                        }
                    }
                    match &td.body {
                        TypeBody::Record(fields) => {
                            if d.attrs.iter().any(|a| a.name == "shared") {
                                self.shared_recs.insert(q.clone());
                            }
                            self.rec_decls.insert(q, (mname.clone(), fields.clone()));
                        }
                        TypeBody::Interface(sigs) => {
                            let mut seen = HashSet::new();
                            for sg in sigs {
                                if !seen.insert(sg.name.clone()) {
                                    return self.err(format!("interface '{}' declares method '{}' twice", td.name, sg.name));
                                }
                            }
                            self.interfaces.insert(q, (mname.clone(), sigs.clone()));
                        }
                        TypeBody::Enum(vs) => {
                            if vs.iter().any(|v| !v.fields.is_empty()) {
                                self.enum_payload.insert(q.clone());
                            }
                            for (i, v) in vs.iter().enumerate() {
                                let key = (mname.clone(), v.name.clone());
                                if self.variants.contains_key(&key) {
                                    return self.err(format!("variant name '{}' is used by two enums in this module", v.name));
                                }
                                self.variants.insert(key, (q.clone(), i));
                            }
                            self.enum_decls.insert(q, (mname.clone(), vs.clone()));
                        }
                        TypeBody::Alias(t) => {
                            self.aliases.insert(q, (mname.clone(), t.clone()));
                        }
                    }
                }
            }
        }
        // Generic functions and the methods of generic records are templates.
        for (mname, file) in &mods {
            self.cur_mod = mname.clone();
            for d in &file.decls {
                let DeclKind::Func(fd) = &d.kind else { continue };
                self.line = d.line;
                if let Some(rq) = self.generic_receiver(mname, fd) {
                    if !fd.generics.is_empty() {
                        return self.err("methods with their own type parameters are not supported yet");
                    }
                    self.tmpl_methods.entry(rq).or_default().push((mname.clone(), fd.clone(), d.line));
                } else if !fd.generics.is_empty() {
                    if fd.is_async {
                        return self.err("async generic functions are not supported");
                    }
                    if fd.receiver.is_some() {
                        return self.err("methods with their own type parameters are not supported yet");
                    }
                    self.tmpl_funcs.insert((mname.clone(), fd.name.clone()), (fd.clone(), d.line));
                }
            }
        }
        // Record layouts.
        let decls: Vec<(String, String, Vec<Field>)> =
            self.rec_decls.iter().map(|(q, (m, f))| (q.clone(), m.clone(), f.clone())).collect();
        for (q, m, fields) in decls {
            self.cur_mod = m.clone();
            let info = self.layout(&fields, &m)?;
            self.records.insert(q, info);
        }
        // Enum layouts.
        let edecls: Vec<(String, String, Vec<Variant>)> =
            self.enum_decls.iter().map(|(q, (m, v))| (q.clone(), m.clone(), v.clone())).collect();
        for (q, m, vs) in edecls {
            self.cur_mod = m.clone();
            let mut variants = Vec::new();
            let mut maxf = 0;
            for v in &vs {
                let mut tys = Vec::new();
                for f in &v.fields {
                    tys.push(self.resolve(f, &m)?);
                }
                maxf = maxf.max(tys.len());
                variants.push((v.name.clone(), tys));
            }
            let payload = self.enum_payload.contains(&q);
            self.enums.insert(q, EnumInfo { variants, payload, size: 8 + 8 * maxf as u32 });
        }
        // Functions, methods, externs, globals.
        let mods: Vec<(String, Vec<Decl>)> = self.modules.iter().map(|m| (m.name.clone(), m.file.decls.clone())).collect();
        for (m, decls) in &mods {
            self.cur_mod = m.clone();
            for d in decls {
                self.line = d.line;
                match &d.kind {
                    DeclKind::Func(fd) => {
                        if !fd.generics.is_empty() || self.generic_receiver(m, fd).is_some() {
                            continue;
                        }
                        let sig = self.func_sig(m, fd)?;
                        if fd.name == "drop" && sig.receiver.is_some() && (!sig.params.is_empty() || sig.ret != Ty::Void) {
                            return self.err("a `drop` method takes no parameters and returns nothing");
                        }
                        if let Some(rt) = &sig.receiver {
                            if matches!(rt, Ty::Void | Ty::Nil | Ty::Err) {
                                return self.err(format!("methods cannot be declared on {}", rt.show()));
                            }
                            let key = method_key(rt);
                            if self.methods.contains_key(&(key.clone(), fd.name.clone())) {
                                return self.err(format!("{} already has a method '{}'", rt.show(), fd.name));
                            }
                            self.methods.insert((key, fd.name.clone()), sig);
                        } else {
                            if self.opt && !d.attrs.iter().any(|a| a.name == "naked") {
                                if let Some(body) = inlinable(fd, &sig) {
                                    self.inline_bodies.insert(sig.sym.clone(), (body, m.clone()));
                                }
                            }
                            self.funcs.insert((m.clone(), fd.name.clone()), sig);
                        }
                    }
                    DeclKind::Extern(ed) => {
                        for a in &d.attrs {
                            if a.name == "link" {
                                if let Some(Expr { kind: ExprKind::Text(parts), .. }) = a.args.first() {
                                    if let Some(TextPart::Lit(l)) = parts.first() {
                                        self.libs.insert(l.clone());
                                    }
                                }
                            }
                        }
                        let mut params = Vec::new();
                        for p in &ed.params {
                            let ty = match &p.ty {
                                Some(t) => self.resolve(t, m)?,
                                None => return self.err("extern parameters need a type"),
                            };
                            params.push(ParamInfo { name: p.name.clone(), ty, by_ref: false, default: None });
                        }
                        let ret = match &ed.ret {
                            Some(t) => self.resolve(t, m)?,
                            None => Ty::Void,
                        };
                        self.funcs.insert(
                            (m.clone(), ed.name.clone()),
                            Sig {
                                sym: ed.name.clone(),
                                module: m.clone(),
                                params,
                                ret,
                                variadic: false,
                                c_variadic: ed.c_variadic,
                                is_extern: true,
                                receiver: None,
                            },
                        );
                    }
                    DeclKind::Var(vd) => {
                        let konst = if vd.mutable { None } else { vd.value.as_ref().and_then(|e| self.const_eval(e, m)) };
                        let ty = match (&vd.ty, &konst) {
                            (Some(t), _) => self.resolve(t, m)?,
                            (None, Some(_)) => Ty::i64(),
                            (None, None) => self.infer_global(vd.value.as_ref(), m)?,
                        };
                        self.globals.insert(
                            (m.clone(), vd.name.clone()),
                            Global { sym: format!("G_{}_{}", m, vd.name), ty, konst, mutable: vd.mutable },
                        );
                    }
                    _ => {}
                }
            }
        }
        self.declared = true;
        Ok(())
    }

    fn layout(&mut self, fields: &[Field], m: &str) -> R<RecInfo> {
        let mut off = 0u32;
        let mut align = 1u32;
        let mut out = Vec::new();
        for f in fields {
            let ty = self.resolve(&f.ty, m)?;
            let sz = ty.size();
            off = (off + sz - 1) / sz * sz;
            align = align.max(sz);
            out.push((f.name.clone(), ty, off));
            off += sz;
        }
        let size = ((off + align - 1) / align * align).max(8);
        Ok(RecInfo { size, fields: out })
    }

    fn func_sig(&mut self, m: &str, fd: &FuncDecl) -> R<Sig> {
        if fd.is_async {
            if fd.params.iter().any(|p| p.by_ref) || fd.receiver.as_ref().map_or(false, |r| r.by_ref) {
                return self.err(format!(
                    "async func '{}' cannot take `ref` parameters: the task works on copies; pass a value or a channel",
                    fd.name
                ));
            }
            if !fd.generics.is_empty() {
                return self.err("async generic functions are not supported");
            }
        }
        let mut params = Vec::new();
        let mut variadic = false;
        for p in &fd.params {
            let mut ty = match &p.ty {
                Some(t) => self.resolve(t, m)?,
                None => return self.err(format!("parameter '{}' needs a type", p.name)),
            };
            if p.variadic {
                variadic = true;
                ty = Ty::List(Box::new(ty));
            }
            params.push(ParamInfo { name: p.name.clone(), ty, by_ref: p.by_ref, default: p.default.clone() });
        }
        let ret = match &fd.ret {
            Some(r) => {
                let inner = match &r.ty {
                    Some(t) => self.resolve(t, m)?,
                    None => Ty::Void,
                };
                if r.fallible {
                    Ty::Result(Box::new(inner))
                } else {
                    inner
                }
            }
            None => Ty::Void,
        };
        let ret = if fd.is_async { Ty::Future(Box::new(ret)) } else { ret };
        let receiver = match &fd.receiver {
            Some(r) => Some(self.resolve(&r.ty, m)?),
            None => None,
        };
        let sym = match &receiver {
            Some(Ty::Rec(r)) => format!("B_{}_{}", sym_of(r), fd.name),
            Some(t) => format!("B_T{}_{}", mangle(&t.show()), fd.name),
            _ => format!("B_{}_{}", m, fd.name),
        };
        Ok(Sig { sym, module: m.to_string(), params, ret, variadic, c_variadic: false, is_extern: false, receiver })
    }

    fn infer_global(&mut self, e: Option<&Expr>, m: &str) -> R<Ty> {
        let Some(e) = e else { return self.err("global variable needs a type or a value") };
        Ok(match &e.kind {
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Text(_) => Ty::Text,
            ExprKind::Int(_) | ExprKind::Char(_) => Ty::i64(),
            ExprKind::Float(_) => Ty::Float { bits: 64 },
            ExprKind::Record { path, .. } => Ty::Rec(self.record_name(path, m)?),
            ExprKind::Call { callee, .. } => match &callee.kind {
                ExprKind::Ident(f) => match self.funcs.get(&(m.to_string(), f.clone())) {
                    Some(s) => s.ret.clone(),
                    None => return self.err("cannot infer the type of this global; add a type annotation"),
                },
                _ => return self.err("cannot infer the type of this global; add a type annotation"),
            },
            _ => return self.err("cannot infer the type of this global; add a type annotation"),
        })
    }

    fn const_eval(&self, e: &Expr, m: &str) -> Option<i64> {
        match &e.kind {
            ExprKind::Int(n) => Some(*n as i64),
            ExprKind::Char(c) => Some(*c as i64),
            ExprKind::Ident(n) => self.globals.get(&(m.to_string(), n.clone())).and_then(|g| g.konst),
            ExprKind::Unary { op: "-", expr } => self.const_eval(expr, m).map(|v| v.wrapping_neg()),
            ExprKind::Unary { op: "bitnot", expr } => self.const_eval(expr, m).map(|v| !v),
            ExprKind::Binary { op, lhs, rhs } => {
                let (a, b) = (self.const_eval(lhs, m)?, self.const_eval(rhs, m)?);
                Some(match *op {
                    "+" => a.wrapping_add(b),
                    "-" => a.wrapping_sub(b),
                    "*" => a.wrapping_mul(b),
                    "/" if b != 0 => a / b,
                    "%" if b != 0 => a % b,
                    "bitor" => a | b,
                    "bitand" => a & b,
                    "bitxor" => a ^ b,
                    "<<" => a << b,
                    ">>" => a >> b,
                    _ => return None,
                })
            }
            _ => None,
        }
    }

    fn record_name(&mut self, path: &[String], m: &str) -> R<String> {
        let q = if path.len() == 1 { format!("{}.{}", m, path[0]) } else { path.join(".") };
        if self.rec_decls.contains_key(&q) {
            return Ok(q);
        }
        if let Some((am, t)) = self.aliases.get(&q).cloned() {
            if let Ty::Rec(r) = self.resolve(&t, &am)? {
                return Ok(r);
            }
        }
        self.err(format!("unknown record type '{}'", path.join(".")))
    }

    fn resolve(&mut self, t: &Type, m: &str) -> R<Ty> {
        match t {
            Type::Named { path, args } => {
                if path.len() == 1 {
                    let n = path[0].as_str();
                    if args.is_empty() {
                        if let Some(t) = self.bindings.get(n) {
                            return Ok(t.clone());
                        }
                    }
                    if let Some(i) = int_type(n) {
                        return Ok(i);
                    }
                    match n {
                        "bool" => return Ok(Ty::Bool),
                        "text" => return Ok(Ty::Text),
                        "error" => return Ok(Ty::Err),
                        "float" | "f64" => return Ok(Ty::Float { bits: 64 }),
                        "f32" => return Ok(Ty::Float { bits: 32 }),
                        "List" if args.len() == 1 => return Ok(Ty::List(Box::new(self.resolve(&args[0], m)?))),
                        "Map" if args.len() == 2 => {
                            let k = self.resolve(&args[0], m)?;
                            let v = self.resolve(&args[1], m)?;
                            return self.map_type(k, v);
                        }
                        "Channel" if args.len() == 1 => return Ok(Ty::Chan(Box::new(self.resolve(&args[0], m)?))),
                        "Future" if args.len() == 1 => return Ok(Ty::Future(Box::new(self.resolve(&args[0], m)?))),
                        _ => {}
                    }
                }
                if path.len() == 2 && path[0] == "c" {
                    if let Some(t) = c_type(&path[1]) {
                        return Ok(t);
                    }
                }
                let q = if path.len() == 1 { format!("{}.{}", m, path[0]) } else { path.join(".") };
                if self.tmpl_recs.contains_key(&q) {
                    return self.instantiate_record(&q, args, m);
                }
                if self.rec_decls.contains_key(&q) {
                    return Ok(Ty::Rec(q));
                }
                if self.interfaces.contains_key(&q) {
                    return self.err(format!(
                        "'{}' is an interface: interfaces can only bound type parameters; write `<T: {}>` and use `T`",
                        path.join("."),
                        path.last().unwrap()
                    ));
                }
                if self.enum_decls.contains_key(&q) {
                    return Ok(Ty::Enum(q));
                }
                if let Some((am, at)) = self.aliases.get(&q).cloned() {
                    return self.resolve(&at, &am);
                }
                self.err(format!("type '{}' is not supported by the native backend", path.join(".")))
            }
            Type::Optional(inner) => {
                let t = self.resolve(inner, m)?;
                if t.nullable() {
                    Ok(t)
                } else {
                    self.err(format!("optional '{}?' is only supported for reference types", t.show()))
                }
            }
            Type::Pointer(inner) => Ok(Ty::Ptr(Box::new(self.resolve(inner, m)?))),
            Type::Slice(inner) => Ok(Ty::List(Box::new(self.resolve(inner, m)?))),
            Type::Array(..) => self.err("fixed arrays are not supported by the native backend yet"),
            Type::Func { abi, params, ret } => {
                let mut ps = Vec::new();
                for p in params {
                    ps.push(self.resolve(p, m)?);
                }
                let r = match ret.as_deref() {
                    Some(rt) => {
                        let inner = match &rt.ty {
                            Some(t) => self.resolve(t, m)?,
                            None => Ty::Void,
                        };
                        if rt.fallible {
                            Ty::Result(Box::new(inner))
                        } else {
                            inner
                        }
                    }
                    None => Ty::Void,
                };
                Ok(Ty::Func { params: ps, ret: Box::new(r), ext: abi.is_some() })
            }
        }
    }

    // ---------------- code generation driver ----------------

    fn generate(&mut self) -> R<String> {
        let mods: Vec<(String, Vec<Decl>)> = self.modules.iter().map(|m| (m.name.clone(), m.file.decls.clone())).collect();
        let main_mod = mods[0].0.clone();
        let main_ret = match self.funcs.get(&(main_mod.clone(), "main".to_string())) {
            Some(s) => s.ret.clone(),
            None => return Err(format!("{}.br: no 'main' function found", main_mod)),
        };
        self.check_templates()?;
        let mut inits = Vec::new();
        for (m, decls) in mods.iter().rev() {
            self.cur_mod = m.clone();
            inits.push(self.gen_init(m, decls)?);
            for d in decls {
                self.line = d.line;
                if let DeclKind::Func(fd) = &d.kind {
                    if fd.generics.is_empty() && self.generic_receiver(m, fd).is_none() {
                        self.gen_func(m, fd, d.attrs.iter().any(|a| a.name == "naked"))?;
                    }
                }
            }
        }
        self.drain_pending()?;
        let recs: Vec<String> = self.records.keys().cloned().collect();
        for q in recs {
            self.gen_record_helpers(&q);
        }
        let enums: Vec<String> = self.enums.keys().cloned().collect();
        for q in enums {
            self.gen_enum_drop(&q);
        }

        let bare = self.target == Target::Bare;
        let mut out = String::new();
        out.push_str("; Generated by the Brasa compiler. Do not edit.
option casemap:none

");
        if !bare {
            for s in ["ExitProcess", "GetProcessHeap", "HeapAlloc", "HeapFree", "GetStdHandle", "WriteFile", "SetConsoleOutputCP"] {
                self.extern_syms.insert(s.to_string());
            }
            for s in &self.extern_syms {
                out.push_str(&format!("extern {}:proc
", s));
            }
        }
        out.push_str("
.code
");
        let main_error = if matches!(main_ret, Ty::Result(_)) {
            "    mov rax, qword ptr [rax]
    test rax, rax
    jz brasa_main_ok
    mov rax, qword ptr [rax]
    mov qword ptr [rsp + 32], rax
    lea rcx, rt_msg_error
    call rt_write_out
    mov rcx, qword ptr [rsp + 32]
    call rt_write_out
    lea rcx, rt_msg_nl
    call rt_write_out
    mov ecx, 1
    call rt_exit
brasa_main_ok:
"
        } else {
            ""
        };
        let mut start = String::new();
        if bare {
            // The loader jumps here in long mode with a flat identity map.
            start.push_str("brasa_start proc
    cli
    cld
    mov rsp, 651272
    sub rsp, 40
");
        } else {
            start.push_str("brasa_start proc
    sub rsp, 40
    mov ecx, 65001
    call SetConsoleOutputCP
");
        }
        for i in &inits {
            start.push_str(&format!("    call {}
", i));
        }
        start.push_str(&format!("    call B_{}_main
", main_mod));
        start.push_str(main_error);
        start.push_str("    xor ecx, ecx
    call rt_exit
brasa_start endp

");
        if bare {
            // the entry point is the first byte of the image
            out.push_str(&start);
            out.push_str(RUNTIME_COMMON);
            out.push_str(&RUNTIME_BARE.replace("B_CONSOLE_WRITE", &format!("B_{}_console_write", main_mod)));
        } else {
            out.push_str(RUNTIME_COMMON);
            out.push_str(RUNTIME_WIN);
            out.push('\n');
            if self.procs.iter().any(|p| p.contains("rt_map_")) {
                out.push_str(RUNTIME_MAP);
                out.push('\n');
            }
            out.push_str(&start);
        }
        for p in &self.procs {
            out.push_str(p);
            out.push('\n');
        }
        out.push_str(".data
");
        out.push_str(RUNTIME_DATA_COMMON);
        out.push_str(if bare { RUNTIME_DATA_BARE } else { RUNTIME_DATA_WIN });
        let mut globals: Vec<&Global> = self.globals.values().filter(|g| g.konst.is_none()).collect();
        globals.sort_by(|a, b| a.sym.cmp(&b.sym));
        for g in globals {
            out.push_str(&format!("{} dq 0
", g.sym));
        }
        for f in &self.fn_closures {
            out.push_str(&format!("dq {}, 0
C_{} dq {}, 0
", IMMORTAL, f, f));
        }
        for (q, i) in &self.enum_statics {
            out.push_str(&format!("dq {}, 0
E_{}_{} dq {}
", IMMORTAL, sym_of(q), i, i));
        }
        for d in &self.data {
            out.push_str(d);
            out.push('\n');
        }
        out.push_str("
end
");
        Ok(out)
    }

    fn finish_proc(&mut self, sym: &str, prologue: Vec<String>, naked: bool) -> String {
        let mut s = format!("{} proc\n", sym);
        if naked {
            for l in &self.fs.code {
                s.push_str(l);
                s.push('\n');
            }
            s.push_str(&format!("{} endp\n", sym));
            return s;
        }
        let mut saves: Vec<(&'static str, i32)> = Vec::new();
        for r in self.fs.used_regs.clone() {
            let off = self.slot();
            saves.push((r, off));
        }
        let out_area = 32 + 8 * (self.fs.max_args.max(4) - 4) as i32;
        let frame = (self.fs.next + out_area + 15) / 16 * 16;
        s.push_str("    push rbp\n    mov rbp, rsp\n");
        // Touch each page of large frames (Windows grows the stack one guard page at a time).
        let mut left = frame;
        while left > 4096 {
            s.push_str("    sub rsp, 4096\n    test qword ptr [rsp], rsp\n");
            left -= 4096;
        }
        s.push_str(&format!("    sub rsp, {}\n", left));
        // Zero every local slot: scope exits release whatever the slots hold.
        if self.fs.next > 0 && (!self.opt || self.fs.may_own || self.fs.has_asm) {
            s.push_str(&format!(
                "    lea r11, [rbp - {n}]\n    xor eax, eax\nZ_{sym}:\n    mov qword ptr [r11], rax\n    add r11, 8\n    cmp r11, rbp\n    jb Z_{sym}\n",
                n = self.fs.next,
                sym = sym
            ));
        }
        for (r, off) in &saves {
            s.push_str(&format!("    mov qword ptr [rbp - {}], {}\n", off, r));
        }
        for l in prologue {
            s.push_str(&l);
            s.push('\n');
        }
        let code = if self.opt && !self.fs.has_asm { peephole(&self.fs.code, &self.fs.ret_label) } else { self.fs.code.clone() };
        for l in &code {
            s.push_str(l);
            s.push('\n');
        }
        s.push_str(&format!("{}:\n", self.fs.ret_label));
        for (r, off) in &saves {
            s.push_str(&format!("    mov {}, qword ptr [rbp - {}]\n", r, off));
        }
        s.push_str(&format!("    mov rsp, rbp\n    pop rbp\n    ret\n{} endp\n", sym));
        s
    }

    fn gen_init(&mut self, m: &str, decls: &[Decl]) -> R<String> {
        let sym = format!("B_init_{}", m);
        let rl = self.lbl("ret");
        self.fs = FnState::new(Ty::Void, rl);
        for d in decls {
            self.line = d.line;
            if let DeclKind::Var(vd) = &d.kind {
                let g = &self.globals[&(m.to_string(), vd.name.clone())];
                if g.konst.is_some() {
                    continue;
                }
                let (gsym, gty) = (g.sym.clone(), g.ty.clone());
                match &vd.value {
                    Some(e) => {
                        let t = self.value(e, Some(&gty))?;
                        self.check(&gty, &t, "global initializer")?;
                    }
                    None => self.zero_value(&gty)?,
                }
                self.norm(&gty);
                self.e(format!("mov qword ptr {}, rax", gsym));
                self.flush_temps(0);
            }
        }
        let p = self.finish_proc(&sym, Vec::new(), false);
        self.procs.push(p);
        Ok(sym)
    }

    fn gen_func(&mut self, m: &str, fd: &FuncDecl, naked: bool) -> R<()> {
        let sig = match &fd.receiver {
            Some(_) => {
                let rt = self.resolve(&fd.receiver.as_ref().unwrap().ty, m)?;
                self.methods[&(method_key(&rt), fd.name.clone())].clone()
            }
            None => self.funcs[&(m.to_string(), fd.name.clone())].clone(),
        };
        let rl = self.lbl("ret");
        self.fs = FnState::new(sig.ret.clone(), rl);
        let main_mod = self.modules[0].name.clone();
        self.fs.allow_await = fd.is_async || (fd.name == "main" && m == main_mod);
        if self.opt && !naked && self.target != Target::Bare {
            self.fs.regs = crate::regalloc::plan(fd);
        }
        let mut incoming: Vec<(String, Ty, bool)> = Vec::new();
        if let (Some(r), Some(rt)) = (&fd.receiver, &sig.receiver) {
            incoming.push((r.name.clone(), rt.clone(), r.by_ref));
        }
        for p in &sig.params {
            incoming.push((p.name.clone(), p.ty.clone(), true));
        }
        let prologue = self.bind_params(&incoming, None);
        if fd.is_async {
            self.gen_async_body(&sig, &fd.body)?;
            self.emit_unwind(0);
            let p = self.finish_proc(&sig.sym, prologue, false);
            self.procs.push(p);
            return Ok(());
        }
        if !naked {
            self.init_defers(&fd.body)?;
        }
        for s in &fd.body {
            self.stmt(s)?;
        }
        if !naked {
            if let Ty::Result(inner) = sig.ret.clone() {
                self.e("xor eax, eax");
                self.mk_ok(&inner);
            }
            self.emit_run_defers();
            self.emit_unwind(0);
        }
        let p = self.finish_proc(&sig.sym, prologue, naked);
        self.procs.push(p);
        Ok(())
    }

    /// Moves incoming arguments into local slots (normalizing small ints).
    /// The callee owns its parameters and releases them when it returns.
    fn bind_params(&mut self, params: &[(String, Ty, bool)], env_slot: Option<i32>) -> Vec<String> {
        let mut pro = Vec::new();
        if let Some(es) = env_slot {
            pro.push(format!("    mov qword ptr [rbp - {}], r10", es));
        }
        for (i, (name, ty, _)) in params.iter().enumerate() {
            let off = self.slot();
            if i < 4 {
                pro.push(format!("    mov qword ptr [rbp - {}], {}", off, ARG_REGS[i]));
            } else {
                pro.push(format!("    mov rax, qword ptr [rbp + {}]", 16 + 8 * i));
                pro.push(format!("    mov qword ptr [rbp - {}], rax", off));
            }
            let reg = self.reg_for(name, ty);
            if let Some(n) = norm_insn(ty) {
                pro.push(format!("    mov rax, qword ptr [rbp - {}]", off));
                pro.push(format!("    {}", n));
                pro.push(format!("    mov qword ptr [rbp - {}], rax", off));
            }
            if let Some(r) = reg {
                pro.push(format!("    mov {}, qword ptr [rbp - {}]", r, off));
            }
            let rc = self.is_rc(ty);
            let sc = &mut self.fs.scopes[0];
            sc.vars.insert(name.clone(), Local { off, ty: ty.clone(), mutable: true, captured: false, reg, imm: None });
            if rc {
                sc.owned.push((off, ty.clone()));
                self.fs.may_own = true;
            }
        }
        pro
    }

    /// Per-record `B_drop_*` (releases heap fields) and `B_copy_*` (deep copy).
    fn gen_record_helpers(&mut self, q: &str) {
        let info = &self.records[q];
        let size = info.size;
        let rc_fields: Vec<(Ty, u32)> =
            info.fields.iter().filter(|f| self.is_rc(&f.1)).map(|f| (f.1.clone(), f.2)).collect();
        let s = sym_of(q);
        let drop = self.rec_drop(q);
        if drop != "0" {
            let mut d = format!("B_drop_{s} proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    mov qword ptr [rbp - 8], rcx\n");
            if let Some(hook) = self.drop_hook(q) {
                // the object is being destroyed: give the hook's receiver a reference of its own
                d.push_str(&format!("    mov qword ptr [rcx - 16], 2\n    call {}\n", hook));
            }
            for (_, off) in &rc_fields {
                d.push_str(&format!(
                    "    mov rcx, qword ptr [rbp - 8]\n    mov rcx, qword ptr [rcx + {off}]\n    call rt_release\n"
                ));
            }
            d.push_str(&format!("    mov rsp, rbp\n    pop rbp\n    ret\nB_drop_{s} endp\n"));
            self.procs.push(d);
        }
        let drop_arg = if drop == "0" { "xor edx, edx".to_string() } else { format!("lea rdx, {}", drop) };
        let mut c = format!(
            "B_copy_{s} proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    test rcx, rcx\n    jnz Lgo_{s}\n    xor eax, eax\n    jmp Lend_{s}\nLgo_{s}:\n    mov qword ptr [rbp - 8], rcx\n    mov ecx, {size}\n    {drop_arg}\n    call rt_alloc\n    mov qword ptr [rbp - 16], rax\n    mov rcx, rax\n    mov rdx, qword ptr [rbp - 8]\n    mov r8d, {size}\n    call rt_memcpy\n"
        );
        for (ty, off) in &rc_fields {
            if matches!(ty, Ty::Rec(f) if !self.shared_recs.contains(f)) {
                let Ty::Rec(f) = ty else { unreachable!() };
                c.push_str(&format!(
                    "    mov rcx, qword ptr [rbp - 16]\n    mov rcx, qword ptr [rcx + {off}]\n    call B_copy_{f}\n    mov rcx, qword ptr [rbp - 16]\n    mov qword ptr [rcx + {off}], rax\n",
                    f = sym_of(f)
                ));
            } else {
                c.push_str(&format!(
                    "    mov rcx, qword ptr [rbp - 16]\n    mov rcx, qword ptr [rcx + {off}]\n    call rt_retain\n"
                ));
            }
        }
        c.push_str(&format!(
            "    mov rax, qword ptr [rbp - 16]\nLend_{s}:\n    mov rsp, rbp\n    pop rbp\n    ret\nB_copy_{s} endp\n"
        ));
        self.procs.push(c);
    }

    /// Per-enum `B_edrop_*`: releases the heap fields of the active variant.
    fn gen_enum_drop(&mut self, q: &str) {
        if self.enum_drop(q) == "0" {
            return;
        }
        let info = self.enums[q].clone();
        let s = sym_of(q);
        let mut d = format!(
            "B_edrop_{s} proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    mov qword ptr [rbp - 8], rcx\n    mov rax, qword ptr [rcx]\n    mov qword ptr [rbp - 16], rax\n"
        );
        for (idx, (_, tys)) in info.variants.iter().enumerate() {
            if !tys.iter().any(|t| self.is_rc(t)) {
                continue;
            }
            d.push_str(&format!("    cmp qword ptr [rbp - 16], {idx}\n    jne Lnx_{s}_{idx}\n"));
            for (i, t) in tys.iter().enumerate() {
                if self.is_rc(t) {
                    d.push_str(&format!(
                        "    mov rcx, qword ptr [rbp - 8]\n    mov rcx, qword ptr [rcx + {}]\n    call rt_release\n",
                        8 + 8 * i
                    ));
                }
            }
            d.push_str(&format!("Lnx_{s}_{idx}:\n"));
        }
        d.push_str(&format!("    mov rsp, rbp\n    pop rbp\n    ret\nB_edrop_{s} endp\n"));
        self.procs.push(d);
    }

    // ---------------- helpers ----------------

    fn norm(&mut self, t: &Ty) {
        if let Some(n) = norm_insn(t) {
            self.e(n);
        }
    }

    fn load_mem(&mut self, t: &Ty, addr: &str) {
        let s = match (t.size(), t) {
            (1, Ty::Int { signed: true, .. }) => format!("movsx rax, byte ptr [{}]", addr),
            (1, _) => format!("movzx eax, byte ptr [{}]", addr),
            (2, Ty::Int { signed: true, .. }) => format!("movsx rax, word ptr [{}]", addr),
            (2, _) => format!("movzx eax, word ptr [{}]", addr),
            (4, Ty::Int { signed: true, .. }) => format!("movsxd rax, dword ptr [{}]", addr),
            (4, _) => format!("mov eax, dword ptr [{}]", addr),
            _ => format!("mov rax, qword ptr [{}]", addr),
        };
        self.e(s);
    }

    fn store_mem(&mut self, t: &Ty, addr: &str) {
        let s = match t.size() {
            1 => format!("mov byte ptr [{}], al", addr),
            2 => format!("mov word ptr [{}], ax", addr),
            4 => format!("mov dword ptr [{}], eax", addr),
            _ => format!("mov qword ptr [{}], rax", addr),
        };
        self.e(s);
    }

    fn check(&self, want: &Ty, got: &Ty, what: &str) -> R<()> {
        if compatible(want, got) {
            Ok(())
        } else {
            let hint = if want.is_float() && got.is_int() { " (write a float literal such as 3.0, or use float(x))" } else { "" };
            self.err(format!("{}: expected {}, found {}{}", what, want.show(), got.show(), hint))
        }
    }

    fn rec_size(&self, r: &str) -> u32 {
        self.records[r].size
    }

    fn field_of(&self, r: &str, name: &str) -> Option<(Ty, u32)> {
        self.records.get(r)?.fields.iter().find(|f| f.0 == name).map(|f| (f.1.clone(), f.2))
    }

    fn lookup(&self, name: &str) -> Option<Local> {
        for s in self.fs.scopes.iter().rev() {
            if let Some(l) = s.vars.get(name) {
                return Some(l.clone());
            }
        }
        None
    }

    fn is_module(&self, name: &str) -> bool {
        self.lookup(name).is_none()
            && !self.globals.contains_key(&(self.cur_mod.clone(), name.to_string()))
            && self.imports.get(&self.cur_mod).map_or(false, |i| i.contains(name))
    }

    /// Qualified name of the enum called `name` in the current module.
    fn enum_type_named(&self, name: &str) -> Option<String> {
        if self.lookup(name).is_some() {
            return None;
        }
        let q = format!("{}.{}", self.cur_mod, name);
        self.enum_decls.contains_key(&q).then_some(q)
    }

    fn zero_value(&mut self, t: &Ty) -> R<()> {
        match t {
            Ty::List(el) => self.new_list(&el.clone()),
            Ty::Map(..) => self.new_map(&t.clone()),
            Ty::Rec(r) => self.record_literal(&r.clone(), &[])?,
            Ty::Text => {
                let l = self.string_lit("");
                self.e(format!("lea rax, {}", l));
            }
            Ty::Chan(_) | Ty::Err | Ty::Result(_) => return self.err(format!("a {} needs an explicit value", t.show())),
            _ => self.e("xor eax, eax"),
        }
        Ok(())
    }

    /// Evaluates `e` to a value that is going to be stored: an owned
    /// reference, with records read from a place deep-copied.
    fn value(&mut self, e: &Expr, want: Option<&Ty>) -> R<Ty> {
        if is_place(e) {
            let (t, own) = self.operand(e, want)?;
            if matches!(&t, Ty::Rec(r) if !self.shared_recs.contains(r)) {
                let Ty::Rec(r) = &t else { unreachable!() };
                let s = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", s));
                self.e("mov rcx, rax");
                self.e(format!("call B_copy_{}", sym_of(r)));
                if own {
                    self.release_slot(s);
                }
                self.free(s);
            } else if !own {
                self.retain_if(&t);
            }
            return Ok(t);
        }
        self.expr(e, want)
    }

    /// Evaluates `e` for inspection. Locals are borrowed (`false`); anything
    /// else is an owned reference the caller must release (`true`).
    fn operand(&mut self, e: &Expr, want: Option<&Ty>) -> R<(Ty, bool)> {
        if let ExprKind::Ident(n) = &e.kind {
            if let Some(l) = self.lookup(n) {
                self.load_local("rax", &l);
                return Ok((l.ty, false));
            }
        }
        let t = self.expr(e, want)?;
        // only heap values carry a reference to give back; a raw pointer or a number does not
        let owned = self.is_rc(&t);
        Ok((t, owned))
    }

    /// Converts the owned value in rax to an owned text.
    fn to_text(&mut self, t: &Ty) -> R<()> {
        match t {
            Ty::Text => {}
            Ty::Int { .. } => {
                self.e("mov rcx, rax");
                self.e("call rt_text_from_int");
            }
            Ty::Float { bits } => {
                let sig = match self.funcs.get(&("float".to_string(), "to_text".to_string())) {
                    Some(s) => s.clone(),
                    None => return self.err("printing a float needs the float library"),
                };
                let (a, b) = (self.temp(), self.temp());
                self.e(format!("mov qword ptr [rbp - {}], rax", a));
                self.e(format!("mov qword ptr [rbp - {}], {}", b, (*bits == 32) as i32));
                self.emit_call(&[a, b], &format!("call {}", sig.sym), None);
                self.free(b);
                self.free(a);
            }
            Ty::Bool => {
                let (lt, lf) = (self.string_lit("true"), self.string_lit("false"));
                self.e("test rax, rax");
                self.e(format!("lea rax, {}", lf));
                self.e(format!("lea rcx, {}", lt));
                self.e("cmovnz rax, rcx");
            }
            Ty::Nil => {
                let l = self.string_lit("nil");
                self.e(format!("lea rax, {}", l));
            }
            Ty::Err => {
                let s = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", s));
                self.e("mov rax, qword ptr [rax]");
                self.retain();
                self.release_slot(s);
                self.free(s);
            }
            // checked again for every instance, where the real type decides
            Ty::Param { .. } => self.e("xor eax, eax"),
            other => return self.err(format!("cannot convert {} to text", other.show())),
        }
        Ok(())
    }

    /// `T` -> `T or error` (success): rax = value in, box out.
    fn mk_ok(&mut self, inner: &Ty) {
        let drop = self.box_drop(inner);
        let t = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t));
        self.alloc(16, drop);
        if *inner != Ty::Void {
            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
            self.e("mov qword ptr [rax + 8], rcx");
        }
        self.free(t);
    }

    /// error in rax -> failed `T or error` box.
    fn mk_err(&mut self, inner: &Ty) {
        let drop = self.box_drop(inner);
        let t = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t));
        self.alloc(16, drop);
        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
        self.e("mov qword ptr [rax], rcx");
        self.free(t);
    }

    // ---------------- statements ----------------

    fn block(&mut self, b: &[Stmt]) -> R<()> {
        self.push_scope();
        for s in b {
            self.stmt(s)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        self.line = s.line;
        let mark = self.fs.stmt_temps.len();
        self.stmt_inner(s)?;
        self.flush_temps(mark);
        Ok(())
    }

    fn stmt_inner(&mut self, s: &Stmt) -> R<()> {
        match &s.kind {
            StmtKind::Var(vd) => {
                let ann = match &vd.ty {
                    Some(t) => Some(self.resolve(t, &self.cur_mod.clone())?),
                    None => None,
                };
                let ty = match &vd.value {
                    Some(e) => {
                        let t = self.value(e, ann.as_ref())?;
                        if let Some(a) = &ann {
                            self.check(a, &t, &format!("'{}'", vd.name))?;
                            a.clone()
                        } else if t == Ty::Nil || t == Ty::Void {
                            return self.err(format!("cannot infer the type of '{}'", vd.name));
                        } else {
                            t
                        }
                    }
                    None => {
                        let a = ann.clone().unwrap();
                        self.zero_value(&a)?;
                        a
                    }
                };
                self.norm(&ty);
                let off = self.declare_local(&vd.name, ty, vd.mutable);
                match self.lookup(&vd.name).and_then(|l| l.reg) {
                    Some(r) => self.e(format!("mov {}, rax", r)),
                    None => self.e(format!("mov qword ptr [rbp - {}], rax", off)),
                }
            }
            StmtKind::Assign { target, op, value } => self.assign(target, op, value)?,
            StmtKind::Return(v) => {
                let ret = self.fs.ret.clone();
                match (&ret, v) {
                    (Ty::Result(inner), Some(e)) => {
                        if **inner == Ty::Void && !matches!(e.kind, ExprKind::Call { .. }) && false {
                            unreachable!();
                        }
                        let t = self.value(e, Some(inner))?;
                        match &t {
                            Ty::Err => self.mk_err(inner),
                            Ty::Result(u) => {
                                if !compatible(inner, u) {
                                    return self.err(format!("return value: expected {}, found {}", ret.show(), t.show()));
                                }
                            }
                            _ => {
                                if **inner == Ty::Void {
                                    return self.err("this function returns no value; write `return` or `return error(\"...\")`");
                                }
                                self.check(inner, &t, "return value")?;
                                self.norm(inner);
                                self.mk_ok(inner);
                            }
                        }
                    }
                    (Ty::Result(inner), None) => {
                        self.e("xor eax, eax");
                        self.mk_ok(inner);
                    }
                    (Ty::Void, Some(_)) => return self.err("this function does not return a value"),
                    (_, Some(e)) => {
                        let t = self.value(e, Some(&ret))?;
                        self.check(&ret, &t, "return value")?;
                        self.norm(&ret);
                    }
                    (_, None) => {}
                }
                self.emit_run_defers();
                self.flush_all_temps_emit();
                self.emit_unwind(0);
                let l = self.fs.ret_label.clone();
                self.e(format!("jmp {}", l));
            }
            StmtKind::Break | StmtKind::Continue => {
                let Some((cont, brk, depth)) = self.fs.loops.last().map(|l| (l.cont.clone(), l.brk.clone(), l.depth)) else {
                    return self.err("'break'/'continue' outside a loop");
                };
                self.emit_unwind(depth);
                let target = if matches!(s.kind, StmtKind::Break) { brk } else { cont };
                self.e(format!("jmp {}", target));
            }
            StmtKind::While { cond, body } => {
                let (start, end) = (self.lbl("while"), self.lbl("wend"));
                self.label(&start);
                self.cond_jump(cond, &end, false)?;
                let depth = self.fs.scopes.len();
                self.fs.loops.push(LoopCtx { cont: start.clone(), brk: end.clone(), depth });
                self.block(body)?;
                self.fs.loops.pop();
                self.e(format!("jmp {}", start));
                self.label(&end);
            }
            StmtKind::For { vars, iter, body } => self.for_loop(vars, iter, body)?,
            StmtKind::Unsafe(b) => {
                self.fs.unsafe_depth += 1;
                self.block(b)?;
                self.fs.unsafe_depth -= 1;
            }
            StmtKind::Asm(a) => self.asm_block(a)?,
            StmtKind::Expr(e) => match &e.kind {
                ExprKind::If { branches, else_body } => {
                    self.if_chain(branches, else_body.as_ref(), None, false)?;
                }
                ExprKind::Match { scrutinee, arms } => {
                    self.match_expr(scrutinee, arms, None, false)?;
                }
                ExprKind::Call { callee, args }
                    if matches!(&callee.kind, ExprKind::Ident(n) if (n == "assert" || n == "assert_eq")
                        && !self.funcs.contains_key(&(self.cur_mod.clone(), n.clone()))) =>
                {
                    let ExprKind::Ident(name) = &callee.kind else { unreachable!() };
                    self.assert_stmt(name, args, e.line)?;
                }
                _ => {
                    let t = self.expr(e, None)?;
                    if matches!(t, Ty::Result(_)) && !self.fs.allow_ignored_result {
                        return self.err("this call can fail and its result is ignored; use `try`, `match` or `??`");
                    }
                    if self.is_rc(&t) {
                        self.release_rax();
                    }
                }
            },
            StmtKind::Defer(e) => self.defer_stmt(e, s.line)?,
            StmtKind::Spawn(call) => {
                let task_spawn = self.funcs.get(&("task".to_string(), "start".to_string())).cloned();
                let Some(sig) = task_spawn else { return self.err("'spawn' needs the task library") };
                if !matches!(call.kind, ExprKind::Call { .. }) {
                    return self.err("'spawn' needs a function call, like `spawn work(x)`");
                }
                let body = vec![Stmt { kind: StmtKind::Expr(call.clone()), line: s.line }];
                let want = Ty::Func { params: vec![], ret: Box::new(Ty::Void), ext: false };
                self.lambda(&[], &body, Some(&want))?;
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                self.emit_call(&[t], &format!("call {}", sig.sym), None);
                self.free(t);
            }
        }
        Ok(())
    }

    fn assign(&mut self, target: &Expr, op: &str, value: &Expr) -> R<()> {
        // `a op= b` is compiled as `a = a op b`.
        let rhs_owned;
        let rhs = if op == "=" {
            value
        } else {
            rhs_owned = Expr {
                kind: ExprKind::Binary { op: binop_static(&op[..op.len() - 1]), lhs: Box::new(target.clone()), rhs: Box::new(value.clone()) },
                line: value.line,
            };
            &rhs_owned
        };
        match &target.kind {
            ExprKind::Ident(name) => {
                if let Some(l) = self.lookup(name) {
                    if l.captured {
                        return self.err(format!("cannot assign to '{}': closures capture variables by value", name));
                    }
                    if !l.mutable {
                        return self.err(format!("cannot assign to '{}': declared with 'let'", name));
                    }
                    let t = self.value(rhs, Some(&l.ty))?;
                    self.check(&l.ty, &t, &format!("'{}'", name))?;
                    self.norm(&l.ty);
                    if self.is_rc(&l.ty) {
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", l.off));
                        self.e(format!("mov qword ptr [rbp - {}], rax", l.off));
                        self.e("call rt_release");
                    } else {
                        self.store_local(&l);
                    }
                    return Ok(());
                }
                let key = (self.cur_mod.clone(), name.clone());
                let Some(g) = self.globals.get(&key) else { return self.err(format!("undefined variable '{}'", name)) };
                if !g.mutable {
                    return self.err(format!("cannot assign to '{}': declared with 'let'", name));
                }
                let (sym, ty) = (g.sym.clone(), g.ty.clone());
                let t = self.value(rhs, Some(&ty))?;
                self.check(&ty, &t, &format!("'{}'", name))?;
                self.norm(&ty);
                if self.is_rc(&ty) {
                    self.e(format!("mov rcx, qword ptr {}", sym));
                    self.e(format!("mov qword ptr {}, rax", sym));
                    self.e("call rt_release");
                } else {
                    self.e(format!("mov qword ptr {}, rax", sym));
                }
            }
            ExprKind::Field { expr: obj, name, .. } => {
                let (ot, own) = self.operand(obj, None)?;
                let r = match &ot {
                    Ty::Rec(r) => r.clone(),
                    Ty::Ptr(p) => match &**p {
                        Ty::Rec(r) => r.clone(),
                        _ => return self.err(format!("cannot assign field '{}' on {}", name, ot.show())),
                    },
                    _ => return self.err(format!("cannot assign field '{}' on {}", name, ot.show())),
                };
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                if let Some((fty, off)) = self.field_of(&r, name) {
                    let vt = self.value(rhs, Some(&fty))?;
                    self.check(&fty, &vt, &format!("field '{}'", name))?;
                    self.norm(&fty);
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                    if self.is_rc(&fty) {
                        let old = self.temp();
                        self.e(format!("mov rdx, qword ptr [rcx + {}]", off));
                        self.e(format!("mov qword ptr [rbp - {}], rdx", old));
                        self.store_mem(&fty, &format!("rcx + {}", off));
                        self.release_slot(old);
                        self.free(old);
                    } else {
                        self.store_mem(&fty, &format!("rcx + {}", off));
                    }
                } else if let Some(sig) = self.methods.get(&(r.clone(), format!("set_{}", name))).cloned() {
                    // property setter: `w.text = v` -> `w.set_text(v)`
                    let Some(p) = sig.params.first().cloned() else {
                        return self.err(format!("'set_{}' must take one parameter", name));
                    };
                    let vt = self.value(rhs, Some(&p.ty))?;
                    self.check(&p.ty, &vt, &format!("'{}'", name))?;
                    self.norm(&p.ty);
                    let vtmp = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", vtmp));
                    // the method takes ownership of its receiver
                    self.e(format!("mov rax, qword ptr [rbp - {}]", t));
                    if !own {
                        self.retain();
                    }
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    self.emit_call(&[t, vtmp], &format!("call {}", sig.sym), None);
                    self.free(vtmp);
                    self.free(t);
                    return Ok(());
                } else {
                    return self.err(format!("{} has no field '{}'", r, name));
                }
                if own {
                    self.release_slot(t);
                }
                self.free(t);
            }
            ExprKind::Index { expr: obj, index } => {
                let (ot, own) = self.operand(obj, None)?;
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                let key_want = if let Ty::Map(k, _) = &ot { Some((**k).clone()) } else { None };
                let ity = self.expr(index, key_want.as_ref())?;
                let it = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", it));
                match &ot {
                    Ty::Map(k, v) => {
                        self.check(k, &ity, "map key")?;
                        let vt = self.value(rhs, Some(v))?;
                        self.check(v, &vt, "map value")?;
                        self.e("mov r8, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                        self.e("call rt_map_set");
                        if **k == Ty::Text {
                            self.release_slot(it);
                        }
                    }
                    Ty::List(el) => {
                        let vt = self.value(rhs, Some(el))?;
                        self.check(el, &vt, "list element")?;
                        self.e("mov r8, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                        self.e("call rt_list_set");
                    }
                    Ty::Ptr(el) => {
                        self.require_unsafe("writing through a raw pointer")?;
                        let el = (**el).clone();
                        let vt = self.value(rhs, Some(&el))?;
                        self.check(&el, &vt, "pointer element")?;
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                        self.e(format!("imul rdx, rdx, {}", el.size()));
                        self.e("add rcx, rdx");
                        self.store_mem(&el, "rcx");
                    }
                    _ => return self.err(format!("cannot assign into {}", ot.show())),
                }
                if own {
                    self.release_slot(t);
                }
                self.free(it);
                self.free(t);
            }
            ExprKind::Deref(p) => {
                self.require_unsafe("writing through a raw pointer")?;
                let (pt, _) = self.operand(p, None)?;
                let Ty::Ptr(el) = pt else { return self.err("'^' needs a pointer") };
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                let vt = self.value(rhs, Some(&el))?;
                self.check(&el, &vt, "pointer target")?;
                self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                self.store_mem(&el, "rcx");
                self.free(t);
            }
            _ => return self.err("invalid assignment target"),
        }
        Ok(())
    }

    fn for_loop(&mut self, vars: &[String], iter: &Expr, body: &[Stmt]) -> R<()> {
        let (start, cont, end) = (self.lbl("for"), self.lbl("fnext"), self.lbl("fend"));
        self.push_scope();
        if let ExprKind::Binary { op: "..", lhs, rhs } = &iter.kind {
            let lt = self.expr(lhs, None)?;
            let ty = if lt.is_int() { lt } else { return self.err("range bounds must be integers") };
            let var = self.declare_local(&vars[0], ty.clone(), false);
            let lreg = self.lookup(&vars[0]).and_then(|l| l.reg);
            match lreg {
                Some(r) => self.e(format!("mov {}, rax", r)),
                None => self.e(format!("mov qword ptr [rbp - {}], rax", var)),
            }
            self.expr(rhs, Some(&ty))?;
            let hi = self.slot();
            self.e(format!("mov qword ptr [rbp - {}], rax", hi));
            self.label(&start);
            match lreg {
                Some(r) => self.e(format!("cmp {}, qword ptr [rbp - {}]", r, hi)),
                None => {
                    self.e(format!("mov rax, qword ptr [rbp - {}]", var));
                    self.e(format!("cmp rax, qword ptr [rbp - {}]", hi));
                }
            }
            self.e(format!("jge {}", end));
            let depth = self.fs.scopes.len();
            self.fs.loops.push(LoopCtx { cont: cont.clone(), brk: end.clone(), depth });
            self.block(body)?;
            self.fs.loops.pop();
            self.label(&cont);
            match lreg {
                Some(r) => self.e(format!("inc {}", r)),
                None => self.e(format!("inc qword ptr [rbp - {}]", var)),
            }
            self.e(format!("jmp {}", start));
            self.label(&end);
        } else {
            let lt = self.expr(iter, None)?;
            if let Ty::Map(k, v) = lt.clone() {
                let map = self.hidden_local(lt);
                self.e(format!("mov qword ptr [rbp - {}], rax", map));
                let idx = self.slot();
                self.e(format!("mov qword ptr [rbp - {}], 0", idx));
                let ent = self.slot();
                let kv = self.declare_local(&vars[0], (*k).clone(), false);
                let vv = if vars.len() >= 2 { Some(self.declare_local(&vars[1], (*v).clone(), false)) } else { None };
                self.e(format!("mov qword ptr [rbp - {}], 0", kv));
                if let Some(vv) = vv {
                    self.e(format!("mov qword ptr [rbp - {}], 0", vv));
                }
                self.label(&start);
                self.e(format!("mov rcx, qword ptr [rbp - {}]", map));
                self.e(format!("mov rax, qword ptr [rbp - {}]", idx));
                self.e("cmp rax, qword ptr [rcx]");
                self.e(format!("jae {}", end));
                self.e("shl rax, 5");
                self.e("add rax, qword ptr [rcx + 16]");
                self.e(format!("mov qword ptr [rbp - {}], rax", ent));
                self.e("cmp qword ptr [rax], 0");
                self.e(format!("je {}", cont));
                self.e("mov rax, qword ptr [rax + 16]");
                self.bind_loop_var(&k, kv);
                if let Some(vv) = vv {
                    self.e(format!("mov rax, qword ptr [rbp - {}]", ent));
                    self.e("mov rax, qword ptr [rax + 24]");
                    self.bind_loop_var(&v, vv);
                }
                let depth = self.fs.scopes.len();
                self.fs.loops.push(LoopCtx { cont: cont.clone(), brk: end.clone(), depth });
                self.block(body)?;
                self.fs.loops.pop();
                self.label(&cont);
                self.e(format!("inc qword ptr [rbp - {}]", idx));
                self.e(format!("jmp {}", start));
                self.label(&end);
                self.pop_scope();
                return Ok(());
            }
            let Ty::List(el) = lt.clone() else { return self.err(format!("cannot iterate over {}", lt.show())) };
            let list = self.hidden_local(lt);
            self.e(format!("mov qword ptr [rbp - {}], rax", list));
            let idx = self.slot();
            self.e(format!("mov qword ptr [rbp - {}], 0", idx));
            let (iv, vv) = if vars.len() >= 2 {
                (Some(self.declare_local(&vars[0], Ty::i64(), false)), self.declare_local(&vars[1], (*el).clone(), false))
            } else {
                (None, self.declare_local(&vars[0], (*el).clone(), false))
            };
            // the slots may hold values from a previous run of this loop
            self.e(format!("mov qword ptr [rbp - {}], 0", vv));
            self.label(&start);
            self.e(format!("mov rdx, qword ptr [rbp - {}]", idx));
            self.e(format!("mov rcx, qword ptr [rbp - {}]", list));
            self.e("cmp rdx, qword ptr [rcx]");
            self.e(format!("jge {}", end));
            self.e("call rt_list_get");
            if matches!(&*el, Ty::Rec(r) if !self.shared_recs.contains(r)) {
                let Ty::Rec(r) = &*el else { unreachable!() };
                self.e("mov rcx, rax");
                self.e(format!("call B_copy_{}", sym_of(r)));
            } else {
                self.retain_if(&el);
            }
            if self.is_rc(&el) {
                self.e(format!("mov rcx, qword ptr [rbp - {}]", vv));
                self.e(format!("mov qword ptr [rbp - {}], rax", vv));
                self.e("call rt_release");
            } else {
                self.e(format!("mov qword ptr [rbp - {}], rax", vv));
            }
            if let Some(iv) = iv {
                self.e(format!("mov rax, qword ptr [rbp - {}]", idx));
                self.e(format!("mov qword ptr [rbp - {}], rax", iv));
            }
            let depth = self.fs.scopes.len();
            self.fs.loops.push(LoopCtx { cont: cont.clone(), brk: end.clone(), depth });
            self.block(body)?;
            self.fs.loops.pop();
            self.label(&cont);
            self.e(format!("inc qword ptr [rbp - {}]", idx));
            self.e(format!("jmp {}", start));
            self.label(&end);
        }
        self.pop_scope();
        Ok(())
    }

    /// Stores the borrowed element in rax into the loop variable at `slot`: a copy for plain
    /// records, a retained reference for other heap values; the previous value is released.
    fn bind_loop_var(&mut self, el: &Ty, slot: i32) {
        if matches!(el, Ty::Rec(r) if !self.shared_recs.contains(r)) {
            let Ty::Rec(r) = el else { unreachable!() };
            self.e("mov rcx, rax");
            self.e(format!("call B_copy_{}", sym_of(r)));
        } else {
            self.retain_if(el);
        }
        if self.is_rc(el) {
            self.e(format!("mov rcx, qword ptr [rbp - {}]", slot));
            self.e(format!("mov qword ptr [rbp - {}], rax", slot));
            self.e("call rt_release");
        } else {
            self.e(format!("mov qword ptr [rbp - {}], rax", slot));
        }
    }

    fn require_unsafe(&self, what: &str) -> R<()> {
        if self.fs.unsafe_depth == 0 {
            self.err(format!("{} requires an 'unsafe' block", what))
        } else {
            Ok(())
        }
    }

    fn asm_block(&mut self, a: &AsmBlock) -> R<()> {
        self.fs.has_asm = true;
        if self.fs.unsafe_depth == 0 && !self.fs.code.is_empty() {
            self.require_unsafe("inline 'asm'")?;
        }
        const POOL: [&str; 4] = ["r10", "r11", "r8", "r9"];
        let mut regs: HashMap<String, String> = HashMap::new();
        let mut next = 0;
        for o in &a.operands {
            let r = if is_register(&o.name) {
                o.name.clone()
            } else {
                if next >= POOL.len() {
                    return self.err("too many asm operands");
                }
                next += 1;
                POOL[next - 1].to_string()
            };
            regs.insert(o.name.clone(), r);
        }
        // Inputs: evaluate all, then load registers.
        let mut loads = Vec::new();
        for o in &a.operands {
            if o.dir != AsmDir::Out {
                self.expr(&o.value, None)?;
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                loads.push((t, reg64(&regs[&o.name])));
            }
        }
        for (t, r) in &loads {
            self.e(format!("mov {}, qword ptr [rbp - {}]", r, t));
        }
        for (t, _) in loads {
            self.free(t);
        }
        for line in &a.lines {
            let mut l = line.clone();
            let mut names: Vec<&String> = regs.keys().collect();
            names.sort_by_key(|n| std::cmp::Reverse(n.len()));
            for n in names {
                l = l.replace(&format!("@{}", n), &regs[n]);
            }
            self.e(l);
        }
        // Outputs: copy registers back into variables.
        for o in &a.operands {
            if o.dir != AsmDir::In {
                let ExprKind::Ident(v) = &o.value.kind else { return self.err("asm output must be a variable") };
                let Some(l) = self.lookup(v) else { return self.err(format!("unknown variable '{}'", v)) };
                self.e(format!("mov rax, {}", reg64(&regs[&o.name])));
                self.norm(&l.ty);
                self.e(format!("mov qword ptr [rbp - {}], rax", l.off));
            }
        }
        Ok(())
    }

    // ---------------- expressions ----------------

    /// Evaluates `e`. The result is in rax; heap values are owned (+1).
    fn expr(&mut self, e: &Expr, want: Option<&Ty>) -> R<Ty> {
        self.line = e.line;
        if self.opt && matches!(e.kind, ExprKind::Binary { .. } | ExprKind::Unary { .. }) {
            // literals are typed from the expected type, as the code generator would do
            let t = match want {
                Some(t) if t.is_int() => t.clone(),
                _ => Ty::i64(),
            };
            let fbits = if matches!(want, Some(Ty::Float { bits: 32 })) { 32 } else { 64 };
            if let Some(c) = self.fold(e, &t, fbits) {
                return Ok(self.emit_const(c, &t, fbits));
            }
        }
        match &e.kind {
            ExprKind::Int(n) => {
                self.e(format!("mov rax, {}", n));
                Ok(match want {
                    Some(t) if t.is_int() => t.clone(),
                    _ => Ty::i64(),
                })
            }
            ExprKind::Char(c) => {
                self.e(format!("mov eax, {}", *c as u32));
                Ok(Ty::Int { bits: 32, signed: false })
            }
            ExprKind::Bool(b) => {
                self.e(format!("mov eax, {}", *b as i32));
                Ok(Ty::Bool)
            }
            ExprKind::Nil => {
                self.e("xor eax, eax");
                Ok(Ty::Nil)
            }
            ExprKind::Float(f) => {
                if matches!(want, Some(Ty::Float { bits: 32 })) {
                    let b = (*f as f32).to_bits();
                    if b == 0 {
                        self.e("xor eax, eax");
                    } else {
                        self.e(format!("mov eax, {}", b));
                    }
                    Ok(Ty::Float { bits: 32 })
                } else {
                    let b = f.to_bits();
                    if b == 0 {
                        self.e("xor eax, eax");
                    } else {
                        self.e(format!("mov rax, {}", b));
                    }
                    Ok(Ty::Float { bits: 64 })
                }
            }
            ExprKind::Text(parts) => self.text(parts),
            ExprKind::Ident(name) => self.ident(name, want),
            ExprKind::List(items) => {
                let mut el_ty = match want {
                    Some(Ty::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                let lt = self.temp();
                let mut created = false;
                for it in items {
                    let t = self.value(it, el_ty.as_ref())?;
                    match &el_ty {
                        Some(w) => self.check(w, &t, "list element")?,
                        None => el_ty = Some(t),
                    }
                    let first = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", first));
                    if !created {
                        let el = el_ty.clone().unwrap();
                        self.new_list(&el);
                        self.e(format!("mov qword ptr [rbp - {}], rax", lt));
                        created = true;
                    }
                    self.e(format!("mov rdx, qword ptr [rbp - {}]", first));
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", lt));
                    self.e("call rt_list_push");
                    self.free(first);
                }
                let Some(el) = el_ty else {
                    return self.err("cannot infer the element type of an empty list; add a type annotation");
                };
                if !created {
                    self.new_list(&el);
                    self.e(format!("mov qword ptr [rbp - {}], rax", lt));
                }
                self.e(format!("mov rax, qword ptr [rbp - {}]", lt));
                self.free(lt);
                Ok(Ty::List(Box::new(el)))
            }
            ExprKind::Map(pairs) => {
                let (mut kt, mut vt) = match want {
                    Some(Ty::Map(k, v)) => (Some((**k).clone()), Some((**v).clone())),
                    _ => (None, None),
                };
                let mt = self.temp();
                let mut created = false;
                if let (Some(k), Some(v)) = (&kt, &vt) {
                    let t = self.map_type(k.clone(), v.clone())?;
                    self.new_map(&t);
                    self.e(format!("mov qword ptr [rbp - {}], rax", mt));
                    created = true;
                }
                for (ke, ve) in pairs {
                    let kty = self.value(ke, kt.as_ref())?;
                    let ks = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                    let vty = self.value(ve, vt.as_ref())?;
                    match (&kt, &vt) {
                        (Some(k), Some(v)) => {
                            self.check(k, &kty, "map key")?;
                            self.check(v, &vty, "map value")?;
                        }
                        _ => {
                            kt = Some(kty);
                            vt = Some(vty);
                        }
                    }
                    if !created {
                        let t = self.map_type(kt.clone().unwrap(), vt.clone().unwrap())?;
                        let vs = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", vs));
                        self.new_map(&t);
                        self.e(format!("mov qword ptr [rbp - {}], rax", mt));
                        self.e(format!("mov rax, qword ptr [rbp - {}]", vs));
                        self.free(vs);
                        created = true;
                    }
                    self.e("mov r8, rax");
                    self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", mt));
                    self.e("call rt_map_set");
                    if kt == Some(Ty::Text) {
                        self.release_slot(ks);
                    }
                    self.free(ks);
                }
                let (Some(k), Some(v)) = (kt, vt) else {
                    return self.err("cannot infer the key and value types of an empty map; add a type annotation");
                };
                self.e(format!("mov rax, qword ptr [rbp - {}]", mt));
                self.free(mt);
                Ok(Ty::Map(Box::new(k), Box::new(v)))
            }
            ExprKind::Record { path, fields } => {
                let r = match self.generic_literal(path, want)? {
                    Some(r) => r,
                    None => self.record_name(path, &self.cur_mod.clone())?,
                };
                self.record_literal(&r, fields)?;
                Ok(Ty::Rec(r))
            }
            ExprKind::Lambda { params, body } => self.lambda(params, body, want),
            ExprKind::If { branches, else_body } => self.if_chain(branches, else_body.as_ref(), want, true),
            ExprKind::Match { scrutinee, arms } => self.match_expr(scrutinee, arms, want, true),
            ExprKind::New(_) => self.err("'new' is not supported by the native backend yet"),
            ExprKind::Propagate(inner) => self.try_expr(inner),
            ExprKind::Await(inner) => {
                if !self.fs.allow_await {
                    return self.err("`await` can only be used inside an `async func` or inside `main`");
                }
                if let ExprKind::Call { callee, args } = &inner.kind {
                    if let ExprKind::Ident(n) = &callee.kind {
                        if (n == "all" || n == "race")
                            && args.len() == 1
                            && self.lookup(n).is_none()
                            && !self.funcs.contains_key(&(self.cur_mod.clone(), n.clone()))
                        {
                            return self.await_combinator(n, &args[0].value);
                        }
                    }
                }
                let t = self.expr(inner, None)?;
                let Ty::Future(v) = t.clone() else {
                    return self.err(format!("`await` needs a Future, found {}", t.show()));
                };
                let s = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", s));
                let sig = self.future_fn("wait")?;
                self.emit_call(&[s], &format!("call {}", sig.sym), None);
                self.free(s);
                self.norm(&v);
                Ok(*v)
            }
            ExprKind::Unary { op, expr: inner } => match *op {
                "not" => {
                    self.expr(inner, Some(&Ty::Bool))?;
                    self.e("test rax, rax");
                    self.e("sete al");
                    self.e("movzx eax, al");
                    Ok(Ty::Bool)
                }
                "-" => {
                    let t = self.expr(inner, want)?;
                    if let Ty::Float { bits } = t {
                        if bits == 32 {
                            self.e("xor eax, 80000000h");
                        } else {
                            self.e("mov rcx, 8000000000000000h");
                            self.e("xor rax, rcx");
                        }
                        return Ok(t);
                    }
                    if !t.is_int() {
                        return self.err(format!("unary '-' needs a number, found {}", t.show()));
                    }
                    self.e("neg rax");
                    self.norm(&t);
                    Ok(t)
                }
                "bitnot" => {
                    let t = self.expr(inner, want)?;
                    self.e("not rax");
                    self.norm(&t);
                    Ok(t)
                }
                "&" => self.address_of(inner),
                "*" => self.deref(inner),
                _ => self.err(format!("unknown operator '{}'", op)),
            },
            ExprKind::Deref(inner) => self.deref(inner),
            ExprKind::Binary { op, lhs, rhs } => self.binary(op, lhs, rhs, want),
            ExprKind::Field { expr: obj, name, safe } => self.field(obj, name, *safe, want),
            ExprKind::Index { expr: obj, index } => {
                let (ot, own) = self.operand(obj, None)?;
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                let key_want = if let Ty::Map(k, _) = &ot { Some((**k).clone()) } else { None };
                let ity = self.expr(index, key_want.as_ref())?;
                let res = match &ot {
                    Ty::Map(k, v) => {
                        self.check(k, &ity, "map key")?;
                        let ks = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                        self.e("mov rdx, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e("call rt_map_get");
                        self.retain_if(v);
                        if **k == Ty::Text {
                            self.release_slot(ks);
                        }
                        self.free(ks);
                        (**v).clone()
                    }
                    Ty::List(el) => {
                        self.e("mov rdx, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e("call rt_list_get");
                        self.retain_if(el);
                        (**el).clone()
                    }
                    Ty::Text => {
                        self.e("mov rdx, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e("call rt_text_byte");
                        Ty::Int { bits: 8, signed: false }
                    }
                    Ty::Ptr(el) => {
                        self.require_unsafe("reading through a raw pointer")?;
                        let el = (**el).clone();
                        self.e(format!("imul rax, rax, {}", el.size()));
                        self.e(format!("add rax, qword ptr [rbp - {}]", t));
                        self.load_mem(&el, "rax");
                        self.retain_if(&el);
                        el
                    }
                    _ => return self.err(format!("cannot index {}", ot.show())),
                };
                if own {
                    self.release_slot(t);
                }
                self.free(t);
                Ok(res)
            }
            ExprKind::Call { callee, args } => self.call(callee, args, want),
        }
    }

    fn text(&mut self, parts: &[TextPart]) -> R<Ty> {
        if parts.iter().all(|p| matches!(p, TextPart::Lit(_))) {
            let s: String = parts.iter().map(|p| if let TextPart::Lit(l) = p { l.as_str() } else { "" }).collect();
            let l = self.string_lit(&s);
            self.e(format!("lea rax, {}", l));
            return Ok(Ty::Text);
        }
        let acc = self.temp();
        let part = self.temp();
        let nw = self.temp();
        for (i, p) in parts.iter().enumerate() {
            match p {
                TextPart::Lit(l) => {
                    let lb = self.string_lit(l);
                    self.e(format!("lea rax, {}", lb));
                }
                TextPart::Expr(x) => {
                    let t = self.expr(x, None)?;
                    self.to_text(&t)?;
                }
            }
            if i == 0 {
                self.e(format!("mov qword ptr [rbp - {}], rax", acc));
            } else {
                self.e(format!("mov qword ptr [rbp - {}], rax", part));
                self.e(format!("mov rcx, qword ptr [rbp - {}]", acc));
                self.e("mov rdx, rax");
                self.e("call rt_concat");
                self.e(format!("mov qword ptr [rbp - {}], rax", nw));
                self.release_slot(acc);
                self.release_slot(part);
                self.e(format!("mov rax, qword ptr [rbp - {}]", nw));
                self.e(format!("mov qword ptr [rbp - {}], rax", acc));
            }
        }
        self.e(format!("mov rax, qword ptr [rbp - {}]", acc));
        self.free(nw);
        self.free(part);
        self.free(acc);
        Ok(Ty::Text)
    }

    fn ident(&mut self, name: &str, want: Option<&Ty>) -> R<Ty> {
        if let Some(l) = self.lookup(name) {
            self.load_local("rax", &l);
            self.retain_if(&l.ty);
            return Ok(l.ty);
        }
        let m = self.cur_mod.clone();
        self.member(&m, name, want)
    }

    /// A global, constant, enum variant or function of module `m`, used as a value.
    fn member(&mut self, m: &str, name: &str, want: Option<&Ty>) -> R<Ty> {
        let key = (m.to_string(), name.to_string());
        if let Some(g) = self.globals.get(&key) {
            let ty = g.ty.clone();
            match g.konst {
                Some(k) => {
                    self.e(format!("mov rax, {}", k));
                    return Ok(match want {
                        Some(t) if t.is_int() => t.clone(),
                        _ => ty,
                    });
                }
                None => {
                    let s = g.sym.clone();
                    self.e(format!("mov rax, qword ptr {}", s));
                    self.retain_if(&ty);
                    return Ok(ty);
                }
            }
        }
        if let Some(sig) = self.funcs.get(&key).cloned() {
            if sig.is_extern {
                return self.err(format!("extern function '{}' cannot be used as a value", name));
            }
            let ext = matches!(want, Some(Ty::Func { ext: true, .. }));
            if ext {
                self.e(format!("lea rax, {}", sig.sym));
            } else {
                self.fn_closures.insert(sig.sym.clone());
                self.e(format!("lea rax, C_{}", sig.sym));
            }
            return Ok(sig.func_ty(ext));
        }
        if let Some((q, idx)) = self.variants.get(&key).cloned() {
            return self.enum_construct(&q, idx, &[]);
        }
        if self.imports.get(&self.cur_mod).map_or(false, |i| i.contains(name)) {
            return self.err(format!("module '{}' cannot be used as a value", name));
        }
        if m != self.cur_mod {
            return self.err(format!("module '{}' has no member '{}'", m, name));
        }
        self.err(format!("undefined identifier '{}'", name))
    }

    /// Builds a value of enum `q`, variant `idx`, from call arguments.
    fn enum_construct(&mut self, q: &str, idx: usize, args: &[Arg]) -> R<Ty> {
        let info = self.enums[q].clone();
        let (vname, tys) = info.variants[idx].clone();
        if args.len() != tys.len() {
            return self.err(format!("'{}' takes {} value(s), found {}", vname, tys.len(), args.len()));
        }
        if !info.payload {
            self.e(format!("mov eax, {}", idx));
        } else if tys.is_empty() {
            self.enum_statics.insert((q.to_string(), idx));
            self.e(format!("lea rax, E_{}_{}", sym_of(q), idx));
        } else {
            let drop = self.enum_drop(q);
            self.alloc(info.size, &drop);
            let t = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", t));
            self.e(format!("mov qword ptr [rax], {}", idx));
            for (i, (a, ty)) in args.iter().zip(&tys).enumerate() {
                let vt = self.value(&a.value, Some(ty))?;
                self.check(ty, &vt, &format!("value {} of '{}'", i + 1, vname))?;
                self.norm(ty);
                self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                self.e(format!("mov qword ptr [rcx + {}], rax", 8 + 8 * i));
            }
            self.e(format!("mov rax, qword ptr [rbp - {}]", t));
            self.free(t);
        }
        Ok(Ty::Enum(q.to_string()))
    }

    fn address_of(&mut self, inner: &Expr) -> R<Ty> {
        match &inner.kind {
            ExprKind::Ident(name) => {
                if let Some(l) = self.lookup(name) {
                    if let Ty::Rec(_) = &l.ty {
                        self.e(format!("mov rax, qword ptr [rbp - {}]", l.off));
                    } else {
                        if l.ty.size() < 8 {
                            return self.err(format!("'&{}': address of a {} variable is not supported; use a record field", name, l.ty.show()));
                        }
                        self.e(format!("lea rax, [rbp - {}]", l.off));
                    }
                    return Ok(Ty::Ptr(Box::new(l.ty)));
                }
                let key = (self.cur_mod.clone(), name.clone());
                if let Some(g) = self.globals.get(&key) {
                    if g.konst.is_none() {
                        let (s, t) = (g.sym.clone(), g.ty.clone());
                        if let Ty::Rec(_) = &t {
                            self.e(format!("mov rax, qword ptr {}", s));
                        } else {
                            self.e(format!("lea rax, {}", s));
                        }
                        return Ok(Ty::Ptr(Box::new(t)));
                    }
                }
                self.err(format!("cannot take the address of '{}'", name))
            }
            ExprKind::Field { expr: obj, name, .. } => {
                let (ot, _) = self.operand(obj, None)?;
                let r = match &ot {
                    Ty::Rec(r) => r.clone(),
                    Ty::Ptr(p) if matches!(&**p, Ty::Rec(_)) => match &**p {
                        Ty::Rec(r) => r.clone(),
                        _ => unreachable!(),
                    },
                    _ => return self.err("'&' needs a variable or record field"),
                };
                let Some((fty, off)) = self.field_of(&r, name) else { return self.err(format!("{} has no field '{}'", r, name)) };
                if let Ty::Rec(_) = &fty {
                    self.e(format!("mov rax, qword ptr [rax + {}]", off));
                } else {
                    self.e(format!("lea rax, [rax + {}]", off));
                }
                Ok(Ty::Ptr(Box::new(fty)))
            }
            _ => self.err("'&' needs a variable or record field"),
        }
    }

    fn deref(&mut self, inner: &Expr) -> R<Ty> {
        let (t, _) = self.operand(inner, None)?;
        let Ty::Ptr(el) = t else { return self.err(format!("'^' needs a pointer, found {}", t.show())) };
        if let Ty::Rec(_) = &*el {
            self.retain();
            return Ok(*el);
        }
        self.require_unsafe("dereferencing a raw pointer")?;
        self.load_mem(&el, "rax");
        self.retain_if(&el);
        Ok(*el)
    }

    fn binary(&mut self, op: &str, lhs: &Expr, rhs: &Expr, want: Option<&Ty>) -> R<Ty> {
        match op {
            "and" | "or" => {
                let (skip, end) = (self.lbl("sc"), self.lbl("scend"));
                self.expr(lhs, Some(&Ty::Bool))?;
                self.e("test rax, rax");
                self.e(format!("{} {}", if op == "and" { "jz" } else { "jnz" }, skip));
                self.expr(rhs, Some(&Ty::Bool))?;
                self.e("test rax, rax");
                self.e(format!("{} {}", if op == "and" { "jz" } else { "jnz" }, skip));
                self.e(format!("mov eax, {}", if op == "and" { 1 } else { 0 }));
                self.e(format!("jmp {}", end));
                self.label(&skip);
                self.e(format!("mov eax, {}", if op == "and" { 0 } else { 1 }));
                self.label(&end);
                return Ok(Ty::Bool);
            }
            "??" => return self.coalesce(lhs, rhs, want),
            ".." => return self.err("ranges are only supported in 'for' loops"),
            _ => {}
        }
        let is_cmp = matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=");
        // A plain local on the left cannot change while the right side runs (an expression cannot
        // assign), so the right side goes first and the local is read last: no temporary.
        if self.opt {
            if let ExprKind::Ident(n) = &lhs.kind {
                if let Some(l) = self.lookup(n) {
                    if !l.captured && !self.is_rc(&l.ty) && !self.rhs_is_simple(rhs) {
                        let lt = l.ty.clone();
                        let (rt, rown) = self.operand(rhs, Some(&lt))?;
                        let owned = if rown && self.is_rc(&rt) {
                            let t = self.temp();
                            self.e(format!("mov qword ptr [rbp - {}], rax", t));
                            Some(t)
                        } else {
                            None
                        };
                        self.e("mov rcx, rax");
                        self.load_local("rax", &l);
                        let res = self.binary_core(op, &lt, &rt, is_cmp)?;
                        if let Some(t) = owned {
                            self.release_slot(t);
                            self.free(t);
                        }
                        return Ok(res);
                    }
                }
            }
        }
        let (lt, lown) = self.operand(lhs, if is_cmp { None } else { want })?;
        // a literal or a plain local on the right needs no temporary: load it straight into rcx
        if self.opt && !(lown && self.is_rc(&lt)) {
            if let Some(rt) = self.simple_rhs(rhs, &lt) {
                return self.binary_core(op, &lt, &rt, is_cmp);
            }
        }
        let t1 = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t1));
        let (rt, rown) = self.operand(rhs, Some(&lt))?;
        let t2 = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t2));
        self.e("mov rcx, rax");
        self.e(format!("mov rax, qword ptr [rbp - {}]", t1));
        let res = self.binary_core(op, &lt, &rt, is_cmp)?;
        if lown && self.is_rc(&lt) {
            self.release_slot(t1);
        }
        if rown && self.is_rc(&rt) {
            self.release_slot(t2);
        }
        self.free(t2);
        self.free(t1);
        Ok(res)
    }

    /// Float arithmetic and comparison; rax = lhs bits, rcx = rhs bits.
    fn float_binary(&mut self, op: &str, lt: &Ty, rt: &Ty, is_cmp: bool) -> R<Ty> {
        if lt != rt {
            return if is_cmp {
                self.err(format!("cannot compare {} with {}", lt.show(), rt.show()))
            } else {
                self.err(format!("operator '{}' is not defined for {} and {}; convert one side first", op, lt.show(), rt.show()))
            };
        }
        let Ty::Float { bits } = lt else { unreachable!() };
        let d = *bits == 64;
        let (mv, sfx) = if d { ("movq", "sd") } else { ("movd", "ss") };
        let (a, b) = if d { ("rax", "rcx") } else { ("eax", "ecx") };
        self.e(format!("{} xmm0, {}", mv, a));
        self.e(format!("{} xmm1, {}", mv, b));
        if is_cmp {
            let uc = format!("ucomi{}", sfx);
            match op {
                "==" | "!=" => {
                    self.e(format!("{} xmm0, xmm1", uc));
                    if op == "==" {
                        self.e("sete al");
                        self.e("setnp cl");
                        self.e("and al, cl");
                    } else {
                        self.e("setne al");
                        self.e("setp cl");
                        self.e("or al, cl");
                    }
                }
                ">" | ">=" => {
                    self.e(format!("{} xmm0, xmm1", uc));
                    self.e(format!("set{} al", if op == ">" { "a" } else { "ae" }));
                }
                _ => {
                    // a < b is b > a; unordered sets CF, so `above` stays false
                    self.e(format!("{} xmm1, xmm0", uc));
                    self.e(format!("set{} al", if op == "<" { "a" } else { "ae" }));
                }
            }
            self.e("movzx eax, al");
            return Ok(Ty::Bool);
        }
        let ins = match op {
            "+" => "add",
            "-" => "sub",
            "*" => "mul",
            "/" => "div",
            "%" => return self.err("'%' is not defined for floats"),
            _ => return self.err(format!("operator '{}' is not defined for {}", op, lt.show())),
        };
        self.e(format!("{}{} xmm0, xmm1", ins, sfx));
        self.e(format!("{} {}, xmm0", mv, a));
        Ok(lt.clone())
    }

    /// When `e` is a number literal or a plain local variable, loads it into rcx (as `operand`
    /// would leave it in rax) and returns its type. Nothing else is evaluated, so the left
    /// operand waiting in rax cannot be disturbed.
    fn simple_rhs(&mut self, e: &Expr, lt: &Ty) -> Option<Ty> {
        match &e.kind {
            ExprKind::Int(n) => {
                self.e(format!("mov rcx, {}", imm64(*n as i64)));
                Some(if lt.is_int() { lt.clone() } else { Ty::i64() })
            }
            ExprKind::Float(f) => {
                if matches!(lt, Ty::Float { bits: 32 }) {
                    self.e(format!("mov ecx, {}", imm64((*f as f32).to_bits() as i64)));
                    Some(Ty::Float { bits: 32 })
                } else {
                    self.e(format!("mov rcx, {}", imm64(f.to_bits() as i64)));
                    Some(Ty::Float { bits: 64 })
                }
            }
            ExprKind::Ident(n) => {
                let l = self.lookup(n)?;
                if l.captured {
                    return None;
                }
                self.load_local("rcx", &l);
                Some(l.ty)
            }
            _ => None,
        }
    }

    /// rax = lhs, rcx = rhs. Leaves the result in rax.
    fn binary_core(&mut self, op: &str, lt: &Ty, rt: &Ty, is_cmp: bool) -> R<Ty> {
        if *lt == Ty::Text && *rt == Ty::Text {
            match op {
                "+" => {
                    self.e("mov rdx, rcx");
                    self.e("mov rcx, rax");
                    self.e("call rt_concat");
                    return Ok(Ty::Text);
                }
                "==" | "!=" => {
                    self.e("mov rdx, rcx");
                    self.e("mov rcx, rax");
                    self.e("call rt_text_eq");
                    if op == "!=" {
                        self.e("xor eax, 1");
                    }
                    return Ok(Ty::Bool);
                }
                _ => return self.err(format!("operator '{}' is not defined for text", op)),
            }
        }
        if matches!(lt, Ty::Param { .. }) || matches!(rt, Ty::Param { .. }) {
            let (a, b) = (lt.show(), rt.show());
            return self.err(format!(
                "operator '{}' is not available for {}: a type parameter only has the methods of its bound{}",
                op,
                if matches!(lt, Ty::Param { .. }) { a } else { b },
                if (matches!(lt, Ty::Param { .. }) && matches!(rt, Ty::Param { .. })) || true { "" } else { "" }
            ));
        }
        if lt.is_float() || rt.is_float() {
            return self.float_binary(op, lt, rt, is_cmp);
        }
        if is_cmp {
            if !(compatible(lt, rt) || compatible(rt, lt)) {
                return self.err(format!("cannot compare {} with {}", lt.show(), rt.show()));
            }
            if let (Ty::Enum(a), Ty::Enum(_)) = (lt, rt) {
                if self.enum_payload.contains(a) {
                    return self.err("enums with data cannot be compared with '=='; use `match`");
                }
            }
            if matches!(lt, Ty::Result(_) | Ty::Err) || matches!(rt, Ty::Result(_) | Ty::Err) {
                return self.err("a result cannot be compared; use `match`, `try` or `??`");
            }
            let unsigned = lt.is_unsigned() || rt.is_unsigned();
            let cc = match (op, unsigned) {
                ("==", _) => "e",
                ("!=", _) => "ne",
                ("<", false) => "l",
                ("<=", false) => "le",
                (">", false) => "g",
                (">=", false) => "ge",
                ("<", true) => "b",
                ("<=", true) => "be",
                (">", true) => "a",
                (">=", true) => "ae",
                _ => unreachable!(),
            };
            self.e("cmp rax, rcx");
            self.e(format!("set{} al", cc));
            self.e("movzx eax, al");
            return Ok(Ty::Bool);
        }
        let ty = match (lt, rt) {
            (Ty::Int { .. }, Ty::Int { .. }) => lt.clone(),
            (Ty::Ptr(_), Ty::Int { .. }) if op == "+" || op == "-" => lt.clone(),
            _ => return self.err(format!("operator '{}' is not defined for {} and {}", op, lt.show(), rt.show())),
        };
        let unsigned = ty.is_unsigned();
        match op {
            "+" => {
                if let Ty::Ptr(el) = &ty {
                    self.e(format!("imul rcx, rcx, {}", el.size()));
                }
                self.e("add rax, rcx")
            }
            "-" => {
                if let Ty::Ptr(el) = &ty {
                    self.e(format!("imul rcx, rcx, {}", el.size()));
                }
                self.e("sub rax, rcx")
            }
            "*" => self.e("imul rax, rcx"),
            "/" | "%" => {
                if unsigned {
                    self.e("xor edx, edx");
                    self.e("div rcx");
                } else {
                    self.e("cqo");
                    self.e("idiv rcx");
                }
                if op == "%" {
                    self.e("mov rax, rdx");
                }
            }
            "bitand" => self.e("and rax, rcx"),
            "bitor" => self.e("or rax, rcx"),
            "bitxor" => self.e("xor rax, rcx"),
            "<<" => self.e("shl rax, cl"),
            ">>" => self.e(if unsigned { "shr rax, cl" } else { "sar rax, cl" }),
            _ => return self.err(format!("unknown operator '{}'", op)),
        }
        self.norm(&ty);
        Ok(ty)
    }

    /// `a ?? b`: a default for an optional value or for a failed call.
    fn coalesce(&mut self, lhs: &Expr, rhs: &Expr, want: Option<&Ty>) -> R<Ty> {
        let (lt, own) = self.operand(lhs, want)?;
        if let Ty::Result(inner) = &lt {
            let inner = (**inner).clone();
            if inner == Ty::Void {
                return self.err("'??' needs a call that returns a value");
            }
            let (other, end) = (self.lbl("coalt"), self.lbl("coalend"));
            let s = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", s));
            self.e("mov rax, qword ptr [rax]");
            self.e("test rax, rax");
            self.e(format!("jnz {}", other));
            self.e(format!("mov rax, qword ptr [rbp - {}]", s));
            self.e("mov rax, qword ptr [rax + 8]");
            self.retain_if(&inner);
            if own {
                self.release_slot(s);
            }
            self.e(format!("jmp {}", end));
            self.label(&other);
            if own {
                self.release_slot(s);
            }
            let rt = self.value(rhs, Some(&inner))?;
            self.check(&inner, &rt, "'??'")?;
            self.label(&end);
            self.free(s);
            return Ok(inner);
        }
        if !lt.nullable() {
            return self.err(format!("'??' needs an optional value, found {}", lt.show()));
        }
        if !own {
            self.retain_if(&lt);
        }
        let end = self.lbl("coal");
        self.e("test rax, rax");
        self.e(format!("jnz {}", end));
        let rt = self.value(rhs, Some(&lt))?;
        self.check(&lt, &rt, "'??'")?;
        self.label(&end);
        Ok(lt)
    }

    fn field(&mut self, obj: &Expr, name: &str, safe: bool, want: Option<&Ty>) -> R<Ty> {
        if let ExprKind::Ident(m) = &obj.kind {
            if self.is_module(m) {
                if m == "c" {
                    return self.err(format!("'c.{}' must be called", name));
                }
                // `want` lets a function of the module be passed where the system wants a callback
                return self.member(&m.clone(), name, want);
            }
            if let Some(q) = self.enum_type_named(m) {
                let Some(idx) = self.enums[&q].variants.iter().position(|v| v.0 == name) else {
                    return self.err(format!("enum {} has no variant '{}'", m, name));
                };
                return self.enum_construct(&q, idx, &[]);
            }
        }
        let (ot, own) = self.operand(obj, None)?;
        let s = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", s));
        let end = self.lbl("safe");
        if safe {
            self.e("test rax, rax");
            self.e(format!("jz {}", end));
        }
        let res = match &ot {
            Ty::Rec(r) => self.rec_field(&r.clone(), name, own)?,
            Ty::Ptr(p) if matches!(&**p, Ty::Rec(_)) => {
                let Ty::Rec(r) = &**p else { unreachable!() };
                let f = self.field_of(r, name);
                match f {
                    Some((fty, off)) => {
                        self.load_mem(&fty, &format!("rax + {}", off));
                        self.retain_if(&fty);
                        fty
                    }
                    None => return self.err(format!("{} has no field '{}'", r, name)),
                }
            }
            Ty::List(_) | Ty::Text if name == "len" => {
                self.e("mov rax, qword ptr [rax]");
                Ty::i64()
            }
            Ty::Map(..) if name == "len" => {
                self.e("mov rax, qword ptr [rax + 48]");
                Ty::i64()
            }
            Ty::Err if name == "message" => {
                self.e("mov rax, qword ptr [rax]");
                self.retain();
                Ty::Text
            }
            _ => return self.err(format!("{} has no field '{}'", ot.show(), name)),
        };
        // property getters consume their receiver themselves
        let consumed = matches!(&ot, Ty::Rec(r) if self.field_of(r, name).is_none());
        if own && self.is_rc(&ot) && !consumed {
            let keep = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", keep));
            self.release_slot(s);
            self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
            self.free(keep);
        }
        if safe {
            self.label(&end);
        }
        self.free(s);
        Ok(res)
    }

    /// Field of the record in rax, or a property getter (a method without
    /// parameters). `own`: the receiver is an owned reference.
    fn rec_field(&mut self, r: &str, name: &str, own: bool) -> R<Ty> {
        if let Some((fty, off)) = self.field_of(r, name) {
            self.load_mem(&fty, &format!("rax + {}", off));
            self.retain_if(&fty);
            return Ok(fty);
        }
        if let Some(sig) = self.methods.get(&(r.to_string(), name.to_string())).cloned() {
            if sig.params.is_empty() && sig.ret != Ty::Void {
                if !own {
                    self.retain();
                }
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                self.emit_call(&[t], &format!("call {}", sig.sym), None);
                self.free(t);
                return Ok(sig.ret);
            }
        }
        self.err(format!("{} has no field '{}'", r, name))
    }

    /// Allocates a record and initializes it from `fields`.
    fn record_literal(&mut self, r: &str, fields: &[(String, Expr)]) -> R<()> {
        let size = self.rec_size(r);
        let drop = self.rec_drop(r);
        self.alloc(size, &drop);
        let t = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t));
        let decl: Vec<(String, Ty, u32)> = self.records[r].fields.clone();
        for (fname, _) in fields {
            if !decl.iter().any(|d| &d.0 == fname) {
                return self.err(format!("{} has no field '{}'", r, fname));
            }
        }
        for (fname, fty, off) in decl {
            match fields.iter().find(|f| f.0 == fname) {
                Some((_, fe)) => {
                    let vt = self.value(fe, Some(&fty))?;
                    self.check(&fty, &vt, &format!("field '{}'", fname))?;
                    self.norm(&fty);
                }
                None => match &fty {
                    Ty::List(el) => self.new_list(&el.clone()),
                    Ty::Text => {
                        let l = self.string_lit("");
                        self.e(format!("lea rax, {}", l));
                    }
                    _ => continue,
                },
            }
            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
            self.store_mem(&fty, &format!("rcx + {}", off));
        }
        self.e(format!("mov rax, qword ptr [rbp - {}]", t));
        self.free(t);
        Ok(())
    }

    fn if_chain(&mut self, branches: &[(Expr, Block)], else_body: Option<&Block>, want: Option<&Ty>, as_value: bool) -> R<Ty> {
        let end = self.lbl("ifend");
        let mut ty: Option<Ty> = None;
        for (cond, body) in branches {
            let next = self.lbl("ifnext");
            self.cond_jump(cond, &next, false)?;
            let t = self.block_value(body, want, as_value)?;
            if ty.is_none() {
                ty = Some(t);
            }
            self.e(format!("jmp {}", end));
            self.label(&next);
        }
        match else_body {
            Some(b) => {
                self.block_value(b, want, as_value)?;
            }
            None if as_value => self.e("xor eax, eax"),
            None => {}
        }
        self.label(&end);
        Ok(if as_value { ty.unwrap_or(Ty::Void) } else { Ty::Void })
    }

    /// Runs a block; with `as_value`, a trailing expression is its value (in rax).
    fn block_value(&mut self, b: &[Stmt], want: Option<&Ty>, as_value: bool) -> R<Ty> {
        self.push_scope();
        let mut ty = Ty::Void;
        for (i, s) in b.iter().enumerate() {
            if as_value && i + 1 == b.len() {
                if let StmtKind::Expr(e) = &s.kind {
                    self.line = s.line;
                    let mark = self.fs.stmt_temps.len();
                    ty = self.value(e, want)?;
                    self.flush_temps(mark);
                    continue;
                }
            }
            self.stmt(s)?;
        }
        self.pop_scope();
        Ok(ty)
    }

    // ---------------- match ----------------

    fn match_expr(&mut self, scrut: &Expr, arms: &[MatchArm], want: Option<&Ty>, as_value: bool) -> R<Ty> {
        self.push_scope();
        let (st, own) = self.operand(scrut, None)?;
        let s = if own {
            let h = self.hidden_local(st.clone());
            h
        } else {
            self.slot()
        };
        self.e(format!("mov qword ptr [rbp - {}], rax", s));
        let end = self.lbl("mend");
        let mut result = Ty::Void;
        let mut got_result = false;
        let mut covered: Vec<String> = Vec::new();
        let mut catch_all = false;

        for arm in arms {
            let body_l = self.lbl("mbody");
            let next_l = self.lbl("mnext");
            let mut binds: Option<(String, Vec<String>)> = None;
            let mut catch_name: Option<String> = None;
            for atom in &arm.pattern {
                match atom {
                    PatAtom::Wildcard => {
                        catch_all = true;
                        self.e(format!("jmp {}", body_l));
                    }
                    PatAtom::Literal(lit) => {
                        self.pattern_literal(&st, s, lit, &body_l)?;
                    }
                    PatAtom::Name { name, bindings } => match &st {
                        Ty::Enum(q) => {
                            let info = self.enums[q].clone();
                            if let Some(idx) = info.variants.iter().position(|v| &v.0 == name) {
                                let want_n = info.variants[idx].1.len();
                                let got_n = bindings.as_ref().map_or(0, |b| b.len());
                                if got_n != want_n {
                                    return self.err(format!("'{}' has {} value(s), the pattern names {}", name, want_n, got_n));
                                }
                                covered.push(name.clone());
                                self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                                if info.payload {
                                    self.e("mov rax, qword ptr [rax]");
                                }
                                self.e(format!("cmp rax, {}", idx));
                                self.e(format!("je {}", body_l));
                                if let Some(b) = bindings {
                                    binds = Some((name.clone(), b.clone()));
                                }
                            } else if bindings.is_none() {
                                catch_all = true;
                                catch_name = Some(name.clone());
                                self.e(format!("jmp {}", body_l));
                            } else {
                                return self.err(format!("enum {} has no variant '{}'", st.show(), name));
                            }
                        }
                        Ty::Result(inner) => {
                            let n = bindings.as_ref().map_or(0, |b| b.len());
                            match name.as_str() {
                                "ok" => {
                                    if n > 1 || (n == 1 && **inner == Ty::Void) {
                                        return self.err("'ok' binds the value of the call (none for `-> error`)");
                                    }
                                    covered.push("ok".into());
                                    self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                                    self.e("mov rax, qword ptr [rax]");
                                    self.e("test rax, rax");
                                    self.e(format!("jz {}", body_l));
                                }
                                "err" => {
                                    if n > 1 {
                                        return self.err("'err' binds one value: the error");
                                    }
                                    covered.push("err".into());
                                    self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                                    self.e("mov rax, qword ptr [rax]");
                                    self.e("test rax, rax");
                                    self.e(format!("jnz {}", body_l));
                                }
                                _ => return self.err("a result is matched with `ok(value)` and `err(e)`"),
                            }
                            if let Some(b) = bindings {
                                binds = Some((name.clone(), b.clone()));
                            }
                        }
                        _ => {
                            if bindings.is_some() {
                                return self.err(format!("cannot match {} against '{}(...)'", st.show(), name));
                            }
                            catch_all = true;
                            catch_name = Some(name.clone());
                            self.e(format!("jmp {}", body_l));
                        }
                    },
                }
            }
            self.e(format!("jmp {}", next_l));
            self.label(&body_l);
            self.push_scope();
            if let Some(n) = catch_name {
                if n != "_" {
                    self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                    self.retain_if(&st);
                    let off = self.declare_local(&n, st.clone(), false);
                    self.e(format!("mov qword ptr [rbp - {}], rax", off));
                }
            }
            if let Some((vname, names)) = binds {
                let tys: Vec<Ty> = match &st {
                    Ty::Enum(q) => self.enums[q].variants.iter().find(|v| v.0 == vname).map(|v| v.1.clone()).unwrap_or_default(),
                    Ty::Result(inner) => {
                        if vname == "ok" {
                            vec![(**inner).clone()]
                        } else {
                            vec![Ty::Err]
                        }
                    }
                    _ => vec![],
                };
                for (i, n) in names.iter().enumerate() {
                    if n == "_" {
                        continue;
                    }
                    let ty = tys[i].clone();
                    self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                    match &st {
                        Ty::Enum(_) => self.e(format!("mov rax, qword ptr [rax + {}]", 8 + 8 * i)),
                        Ty::Result(_) if vname == "ok" => self.e("mov rax, qword ptr [rax + 8]"),
                        _ => self.e("mov rax, qword ptr [rax]"),
                    }
                    self.retain_if(&ty);
                    let off = self.declare_local(n, ty, false);
                    self.e(format!("mov qword ptr [rbp - {}], rax", off));
                }
            }
            let t = self.block_value(&arm.body, want, as_value)?;
            self.pop_scope();
            if !got_result && t != Ty::Void {
                result = t;
                got_result = true;
            }
            self.e(format!("jmp {}", end));
            self.label(&next_l);
        }

        // exhaustiveness
        if !catch_all {
            let missing: Vec<String> = match &st {
                Ty::Enum(q) => self.enums[q].variants.iter().map(|v| v.0.clone()).filter(|n| !covered.contains(n)).collect(),
                Ty::Result(_) => ["ok", "err"].iter().map(|s| s.to_string()).filter(|n| !covered.contains(n)).collect(),
                _ => vec![],
            };
            if !missing.is_empty() {
                return self.err(format!("'match' is not exhaustive: missing {}; add the cases or `_ ->`", missing.join(", ")));
            }
        }
        if as_value {
            self.e("xor eax, eax");
        }
        self.label(&end);
        self.pop_scope();
        Ok(if as_value { result } else { Ty::Void })
    }

    fn pattern_literal(&mut self, st: &Ty, s: i32, lit: &Expr, body: &str) -> R<()> {
        match &lit.kind {
            ExprKind::Text(parts) if *st == Ty::Text => {
                let txt: String = parts.iter().map(|p| if let TextPart::Lit(l) = p { l.as_str() } else { "" }).collect();
                let l = self.string_lit(&txt);
                self.e(format!("mov rcx, qword ptr [rbp - {}]", s));
                self.e(format!("lea rdx, {}", l));
                self.e("call rt_text_eq");
                self.e("test rax, rax");
                self.e(format!("jnz {}", body));
            }
            ExprKind::Nil if st.nullable() => {
                self.e(format!("cmp qword ptr [rbp - {}], 0", s));
                self.e(format!("je {}", body));
            }
            _ => {
                let v = match &lit.kind {
                    ExprKind::Bool(b) => Some(*b as i64),
                    _ => self.const_eval(lit, &self.cur_mod.clone()),
                };
                let Some(v) = v else { return self.err(format!("this pattern cannot be matched against {}", st.show())) };
                if !(st.is_int() || *st == Ty::Bool) {
                    return self.err(format!("a number pattern cannot match {}", st.show()));
                }
                self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                self.e(format!("mov rcx, {}", v));
                self.e("cmp rax, rcx");
                self.e(format!("je {}", body));
            }
        }
        Ok(())
    }

    // ---------------- try ----------------

    fn try_expr(&mut self, inner: &Expr) -> R<Ty> {
        let Ty::Result(ret_inner) = self.fs.ret.clone() else {
            return self.err("'try' can only be used inside a function that ends with `or error`");
        };
        let (t, own) = self.operand(inner, None)?;
        let Ty::Result(v) = t.clone() else {
            return self.err(format!("'try' needs a call that can fail, found {}", t.show()));
        };
        let s = self.temp();
        let (ok_l,) = (self.lbl("tryok"),);
        self.e(format!("mov qword ptr [rbp - {}], rax", s));
        self.e("mov rcx, qword ptr [rax]");
        self.e("test rcx, rcx");
        self.e(format!("jz {}", ok_l));
        // failure: hand the same error to our caller
        self.e("mov rax, rcx");
        self.retain();
        let e2 = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", e2));
        if own {
            self.release_slot(s);
        }
        self.e(format!("mov rax, qword ptr [rbp - {}]", e2));
        self.mk_err(&ret_inner);
        self.emit_run_defers();
        self.flush_all_temps_emit();
        self.emit_unwind(0);
        let rl = self.fs.ret_label.clone();
        self.e(format!("jmp {}", rl));
        self.free(e2);
        self.label(&ok_l);
        if *v != Ty::Void {
            self.e(format!("mov rax, qword ptr [rbp - {}]", s));
            self.e("mov rax, qword ptr [rax + 8]");
            self.retain_if(&v);
            if own {
                let keep = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                self.release_slot(s);
                self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
                self.free(keep);
            }
        } else if own {
            self.release_slot(s);
        }
        self.free(s);
        Ok(*v)
    }

    // ---------------- closures ----------------

    fn lambda(&mut self, params: &[Param], body: &[Stmt], want: Option<&Ty>) -> R<Ty> {
        let allow_await = std::mem::take(&mut self.next_lambda_await);
        let share_records = std::mem::take(&mut self.next_lambda_share);
        let ignore_result = std::mem::take(&mut self.next_lambda_ignore_result);
        let (wparams, wret) = match want {
            Some(Ty::Func { params, ret, ext: false }) => (Some(params.clone()), (**ret).clone()),
            Some(Ty::Func { ext: true, .. }) => return self.err("a closure cannot be used as an extern callback; pass a top-level function"),
            _ => (None, Ty::Void),
        };
        // the result type is worked out from the body when nothing is expected, or when only
        // the parameter types are known (the result is the `?` placeholder of generic inference)
        let infer_ret = !matches!(want, Some(Ty::Func { .. })) || matches!(&wret, Ty::Param { name, .. } if name == "?");
        let wret = if infer_ret { Ty::Void } else { wret };
        let mut ptys = Vec::new();
        for (i, p) in params.iter().enumerate() {
            let t = match (&p.ty, &wparams) {
                (Some(t), _) => self.resolve(t, &self.cur_mod.clone())?,
                (None, Some(wp)) if i < wp.len() => wp[i].clone(),
                _ => return self.err(format!("cannot infer the type of lambda parameter '{}'", p.name)),
            };
            ptys.push((p.name.clone(), t, true));
        }
        if let Some(wp) = &wparams {
            if wp.len() != params.len() {
                return self.err(format!("expected a function with {} parameter(s), found {}", wp.len(), params.len()));
            }
        }
        // Free variables that are locals of the enclosing function.
        let mut used = Vec::new();
        collect_block(body, &mut used);
        let pnames: HashSet<&String> = params.iter().map(|p| &p.name).collect();
        let mut caps: Vec<(String, Local)> = Vec::new();
        for n in used {
            if pnames.contains(&n) || caps.iter().any(|c| c.0 == n) {
                continue;
            }
            if let Some(l) = self.lookup(&n) {
                caps.push((n, l));
            }
        }
        let env_rc = caps.iter().any(|c| self.is_rc(&c.1.ty));
        let sym = self.lbl("L");
        let env_drop = if env_rc { format!("{}_env", sym) } else { "0".to_string() };
        // Environment block with captured values (records are copied).
        let env = self.temp();
        if caps.is_empty() {
            self.e(format!("mov qword ptr [rbp - {}], 0", env));
        } else {
            self.alloc(8 * caps.len() as u32, &env_drop);
            self.e(format!("mov qword ptr [rbp - {}], rax", env));
            for (i, (_, l)) in caps.iter().enumerate() {
                self.load_local("rax", &l);
                if !share_records && matches!(&l.ty, Ty::Rec(r) if !self.shared_recs.contains(r)) {
                    let Ty::Rec(r) = &l.ty else { unreachable!() };
                    self.e("mov rcx, rax");
                    self.e(format!("call B_copy_{}", sym_of(r)));
                } else {
                    self.retain_if(&l.ty);
                }
                self.e(format!("mov rcx, qword ptr [rbp - {}]", env));
                self.e(format!("mov qword ptr [rcx + {}], rax", 8 * i));
            }
        }
        // Compile the body as its own procedure.
        let rl = self.lbl("ret");
        let saved = std::mem::replace(&mut self.fs, FnState::new(wret.clone(), rl));
        self.fs.unsafe_depth = saved.unsafe_depth;
        self.fs.allow_await = allow_await;
        self.fs.allow_ignored_result = ignore_result;
        let env_slot = self.slot();
        let mut pro = self.bind_params(&ptys, Some(env_slot));
        for (i, (n, l)) in caps.iter().enumerate() {
            let off = self.slot();
            pro.push(format!("    mov rax, qword ptr [rbp - {}]", env_slot));
            pro.push(format!("    mov rax, qword ptr [rax + {}]", 8 * i));
            if self.is_rc(&l.ty) {
                pro.push("    mov rcx, rax".to_string());
                pro.push("    call rt_retain".to_string());
            }
            pro.push(format!("    mov qword ptr [rbp - {}], rax", off));
            let rc = self.is_rc(&l.ty);
            let sc = &mut self.fs.scopes[0];
            sc.vars.insert(n.clone(), Local { off, ty: l.ty.clone(), mutable: false, captured: true, reg: None, imm: None });
            if rc {
                sc.owned.push((off, l.ty.clone()));
                self.fs.may_own = true;
            }
        }
        let mut inferred: Option<Ty> = None;
        let r = (|| -> R<()> {
            self.init_defers(body)?;
            if infer_ret {
                let t = self.block_value(body, None, true)?;
                if t != Ty::Void {
                    self.norm(&t);
                    inferred = Some(t);
                }
            } else if let Ty::Result(inner) = wret.clone() {
                self.block_value(body, Some(&wret), false)?;
                self.e("xor eax, eax");
                self.mk_ok(&inner);
            } else {
                let t = self.block_value(body, Some(&wret), wret != Ty::Void)?;
                if wret != Ty::Void && t != Ty::Void {
                    self.check(&wret, &t, "lambda result")?;
                    self.norm(&wret);
                }
            }
            self.emit_run_defers();
            self.emit_unwind(0);
            Ok(())
        })();
        let proc_text = self.finish_proc(&sym, pro, false);
        self.fs = saved;
        r?;
        self.procs.push(proc_text);
        if env_rc {
            let mut d = format!("{}_env proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    mov qword ptr [rbp - 8], rcx\n", sym);
            for (i, (_, l)) in caps.iter().enumerate() {
                if self.is_rc(&l.ty) {
                    d.push_str(&format!(
                        "    mov rcx, qword ptr [rbp - 8]\n    mov rcx, qword ptr [rcx + {}]\n    call rt_release\n",
                        8 * i
                    ));
                }
            }
            d.push_str(&format!("    mov rsp, rbp\n    pop rbp\n    ret\n{}_env endp\n", sym));
            self.procs.push(d);
        }
        // Closure object [code, env]; it owns the environment.
        self.alloc(16, "rt_drop_closure");
        self.e(format!("lea rcx, {}", sym));
        self.e("mov qword ptr [rax], rcx");
        self.e(format!("mov rcx, qword ptr [rbp - {}]", env));
        self.e("mov qword ptr [rax + 8], rcx");
        self.free(env);
        let wret = inferred.unwrap_or(wret);
        Ok(Ty::Func { params: ptys.into_iter().map(|p| p.1).collect(), ret: Box::new(wret), ext: false })
    }

    // ---------------- calls ----------------

    /// Loads argument temps into registers / stack and emits the call.
    fn emit_call(&mut self, temps: &[i32], call: &str, closure: Option<(i32, bool)>) {
        self.fs.max_args = self.fs.max_args.max(temps.len());
        for (i, t) in temps.iter().enumerate().skip(4) {
            self.e(format!("mov rax, qword ptr [rbp - {}]", t));
            self.e(format!("mov qword ptr [rsp + {}], rax", 32 + 8 * (i - 4)));
        }
        for (i, t) in temps.iter().enumerate().take(4) {
            self.e(format!("mov {}, qword ptr [rbp - {}]", ARG_REGS[i], t));
        }
        match closure {
            Some((c, false)) => {
                self.e(format!("mov rax, qword ptr [rbp - {}]", c));
                self.e("mov r10, qword ptr [rax + 8]");
                self.e("call qword ptr [rax]");
            }
            Some((c, true)) => {
                self.e(format!("mov rax, qword ptr [rbp - {}]", c));
                self.e("call rax");
            }
            None => self.e(call),
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Arg], want: Option<&Ty>) -> R<Ty> {
        match &callee.kind {
            ExprKind::Ident(name) if self.lookup(name).is_none() && matches!(name.as_str(), "__from_bits" | "__bits") => {
                if args.len() != 1 {
                    return self.err(format!("{}() takes one argument", name));
                }
                // reinterpret the 64 bits: a float and an int share the register
                if name == "__from_bits" {
                    let at = self.expr(&args[0].value, Some(&Ty::i64()))?;
                    self.check(&Ty::i64(), &at, name)?;
                    Ok(Ty::Float { bits: 64 })
                } else {
                    let at = self.expr(&args[0].value, Some(&Ty::Float { bits: 64 }))?;
                    self.check(&Ty::Float { bits: 64 }, &at, name)?;
                    Ok(Ty::i64())
                }
            }
            ExprKind::Ident(name) if self.lookup(name).is_none() && matches!(name.as_str(), "__sqrt" | "__floor" | "__ceil" | "__trunc") => {
                if args.len() != 1 {
                    return self.err(format!("{}() takes one argument", name));
                }
                let at = self.expr(&args[0].value, Some(&Ty::Float { bits: 64 }))?;
                self.check(&Ty::Float { bits: 64 }, &at, name)?;
                self.e("movq xmm0, rax");
                self.e(match name.as_str() {
                    "__sqrt" => "sqrtsd xmm0, xmm0",
                    "__floor" => "roundsd xmm0, xmm0, 9",
                    "__ceil" => "roundsd xmm0, xmm0, 10",
                    _ => "roundsd xmm0, xmm0, 11",
                });
                self.e("movq rax, xmm0");
                Ok(Ty::Float { bits: 64 })
            }
            ExprKind::Ident(name) if self.lookup(name).is_none() && matches!(name.as_str(), "float" | "f64" | "f32") => {
                if args.len() != 1 {
                    return self.err(format!("{}() takes one argument", name));
                }
                let t = Ty::Float { bits: if name == "f32" { 32 } else { 64 } };
                let Ty::Float { bits: tb } = t else { unreachable!() };
                let at = self.expr(&args[0].value, None)?;
                match at {
                    Ty::Int { bits: 64, signed: false } => {
                        // values above i64::MAX: halve (keeping the low bit), convert, double
                        let (big, end) = (self.lbl("ubig"), self.lbl("uend"));
                        let cvt = if tb == 32 { "cvtsi2ss" } else { "cvtsi2sd" };
                        let mv = if tb == 32 { "movd" } else { "movq" };
                        let (add, ra) = if tb == 32 { ("addss", "eax") } else { ("addsd", "rax") };
                        self.e("test rax, rax");
                        self.e(format!("js {}", big));
                        self.e(format!("{} xmm0, rax", cvt));
                        self.e(format!("jmp {}", end));
                        self.label(&big);
                        self.e("mov rcx, rax");
                        self.e("shr rax, 1");
                        self.e("and ecx, 1");
                        self.e("or rax, rcx");
                        self.e(format!("{} xmm0, rax", cvt));
                        self.e(format!("{} xmm0, xmm0", add));
                        self.label(&end);
                        self.e(format!("{} {}, xmm0", mv, ra));
                    }
                    Ty::Int { .. } | Ty::Bool => {
                        self.e(format!("{} xmm0, rax", if tb == 32 { "cvtsi2ss" } else { "cvtsi2sd" }));
                        self.e(if tb == 32 { "movd eax, xmm0" } else { "movq rax, xmm0" });
                    }
                    Ty::Float { bits: sb } if sb == tb => {}
                    Ty::Float { bits: 64 } => {
                        self.e("movq xmm0, rax");
                        self.e("cvtsd2ss xmm0, xmm0");
                        self.e("movd eax, xmm0");
                    }
                    Ty::Float { .. } => {
                        self.e("movd xmm0, eax");
                        self.e("cvtss2sd xmm0, xmm0");
                        self.e("movq rax, xmm0");
                    }
                    other => return self.err(format!("cannot convert {} to {}", other.show(), name)),
                }
                Ok(t)
            }
            ExprKind::Ident(name) if self.lookup(name).is_none() => {
                if let Some(t) = int_type(name).or(if name == "bool" { Some(Ty::Bool) } else { None }) {
                    if args.len() != 1 {
                        return self.err(format!("{}() takes one argument", name));
                    }
                    let at = self.expr(&args[0].value, None)?;
                    if let Ty::Float { bits } = at {
                        // truncate toward zero; NaN, infinities and out-of-range values panic
                        let ok = self.lbl("f2iok");
                        if bits == 32 {
                            self.e("movd xmm0, eax");
                            self.e("cvtss2sd xmm0, xmm0");
                        } else {
                            self.e("movq xmm0, rax");
                        }
                        self.e("cvttsd2si rax, xmm0");
                        self.e("mov rcx, 8000000000000000h");
                        self.e("cmp rax, rcx");
                        self.e(format!("jne {}", ok));
                        self.e("movq rdx, xmm0");
                        self.e("mov rcx, 0C3E0000000000000h");
                        self.e("cmp rdx, rcx");
                        self.e(format!("je {}", ok));
                        self.e("lea rcx, rt_msg_floatrange");
                        self.e("call rt_panic");
                        self.label(&ok);
                        if t == Ty::Bool {
                            return self.err("cannot convert float to bool");
                        }
                        self.norm(&t);
                        return Ok(t);
                    }
                    if !(at.is_int() || matches!(at, Ty::Ptr(_) | Ty::Bool | Ty::Func { ext: true, .. })) {
                        return self.err(format!("cannot convert {} to {}", at.show(), name));
                    }
                    if t == Ty::Bool {
                        self.e("test rax, rax");
                        self.e("setnz al");
                        self.e("movzx eax, al");
                    }
                    self.norm(&t);
                    return Ok(t);
                }
                if name == "panic" && args.len() == 1 && !self.funcs.contains_key(&(self.cur_mod.clone(), name.clone())) {
                    let t = self.expr(&args[0].value, Some(&Ty::Text))?;
                    self.check(&Ty::Text, &t, "panic message")?;
                    self.e("mov rcx, rax");
                    self.e("call rt_panic");
                    return Ok(Ty::Void);
                }
                if name == "print" || name == "println" {
                    let mut first = true;
                    for a in args {
                        if !first {
                            let sp = self.string_lit(" ");
                            self.e(format!("lea rcx, {}", sp));
                            self.e("call rt_write_out");
                        }
                        first = false;
                        let t = self.expr(&a.value, None)?;
                        self.to_text(&t)?;
                        let keep = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                        self.e("mov rcx, rax");
                        self.e("call rt_write_out");
                        self.release_slot(keep);
                        self.free(keep);
                    }
                    let nl = self.string_lit("\n");
                    self.e(format!("lea rcx, {}", nl));
                    self.e("call rt_write_out");
                    return Ok(Ty::Void);
                }
                if name == "error" && args.len() == 1 {
                    let t = self.expr(&args[0].value, Some(&Ty::Text))?;
                    self.check(&Ty::Text, &t, "error message")?;
                    let tmp = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", tmp));
                    self.alloc(8, "rt_drop_error");
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", tmp));
                    self.e("mov qword ptr [rax], rcx");
                    self.free(tmp);
                    return Ok(Ty::Err);
                }
                if name == "chr" && args.len() == 1 {
                    let t = self.expr(&args[0].value, None)?;
                    if !t.is_int() {
                        return self.err(format!("chr() needs a number, found {}", t.show()));
                    }
                    self.e("mov rcx, rax");
                    self.e("call rt_text_chr");
                    return Ok(Ty::Text);
                }
                if name == "heap_bytes" && args.is_empty() {
                    if self.target != Target::Bare {
                        return self.err("heap_bytes() is only available in a bare-metal kernel");
                    }
                    self.e("mov rax, qword ptr rt_brk");
                    self.e("sub rax, 2097152");
                    return Ok(Ty::i64());
                }
                if name == "live_objects" && args.is_empty() {
                    self.e("mov rax, qword ptr rt_live");
                    return Ok(Ty::i64());
                }
                if name == "channel" {
                    return self.channel_new(args);
                }
                if name == "timeout" && args.len() == 2 && !self.funcs.contains_key(&(self.cur_mod.clone(), name.clone())) {
                    return self.timeout_future(&args[0].value, &args[1].value);
                }
                if name == "sleep" && args.len() == 1 && !self.funcs.contains_key(&(self.cur_mod.clone(), name.clone())) {
                    let t = self.expr(&args[0].value, Some(&Ty::i64()))?;
                    if !t.is_int() {
                        return self.err(format!("sleep() needs a number of milliseconds, found {}", t.show()));
                    }
                    let s = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", s));
                    let sig = self.future_fn("sleep_future")?;
                    self.emit_call(&[s], &format!("call {}", sig.sym), None);
                    self.free(s);
                    return Ok(Ty::Future(Box::new(Ty::Void)));
                }
                let key = (self.cur_mod.clone(), name.clone());
                if let Some(sig) = self.funcs.get(&key).cloned() {
                    return self.call_sig(&sig, None, args);
                }
                if let Some((fd, line)) = self.tmpl_funcs.get(&key).cloned() {
                    return self.call_generic(&key.0, &fd, line, args, want);
                }
                if let Some((q, idx)) = self.variants.get(&key).cloned() {
                    return self.enum_construct(&q, idx, args);
                }
                if self.globals.contains_key(&key) {
                    let t = self.expr(callee, None)?;
                    return self.call_value(t, args);
                }
                self.err(format!("undefined function '{}'", name))
            }
            ExprKind::Field { expr: obj, name, .. } => {
                if let ExprKind::Ident(m) = &obj.kind {
                    if self.is_module(m) {
                        if m == "c" {
                            return self.c_intrinsic(name, args);
                        }
                        // `format.fmt(template, a, b)` is `format.fmt2(template, a, b)`
                        let renamed;
                        let name = if m == "format" && name == "fmt" && !self.funcs.contains_key(&(m.clone(), name.clone())) {
                            if args.is_empty() {
                                return self.err("format.fmt needs a template");
                            }
                            if args.len() > 9 {
                                return self.err("format.fmt takes at most eight values");
                            }
                            renamed = format!("fmt{}", args.len() - 1);
                            &renamed
                        } else {
                            name
                        };
                        let key = (m.clone(), name.clone());
                        if let Some((fd, line)) = self.tmpl_funcs.get(&key).cloned() {
                            return self.call_generic(&key.0, &fd, line, args, want);
                        }
                        return match self.funcs.get(&key).cloned() {
                            Some(sig) => self.call_sig(&sig, None, args),
                            None if self.modules.iter().any(|x| &x.name == m) => {
                                if let Some((q, idx)) = self.variants.get(&key).cloned() {
                                    return self.enum_construct(&q, idx, args);
                                }
                                if self.globals.contains_key(&key) {
                                    let t = self.member(&m.clone(), name, None)?;
                                    return self.call_value(t, args);
                                }
                                self.err(format!("module '{}' has no function '{}'", m, name))
                            }
                            None => self.err(format!("module '{}' is not available in the native backend", m)),
                        };
                    }
                    if let Some(q) = self.enum_type_named(m) {
                        let Some(idx) = self.enums[&q].variants.iter().position(|v| &v.0 == name) else {
                            return self.err(format!("enum {} has no variant '{}'", m, name));
                        };
                        return self.enum_construct(&q, idx, args);
                    }
                }
                let ot = self.expr(obj, None)?;
                if let Ty::Param { name: tname, bound } = &ot {
                    return self.param_method_call("", bound, tname, name, args);
                }
                if !matches!(ot, Ty::Rec(_)) {
                    if let Some(sig) = self.methods.get(&(method_key(&ot), name.clone())).cloned() {
                        let t = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", t));
                        let res = self.call_sig(&sig, Some(t), args);
                        self.free(t);
                        return res;
                    }
                }
                let r = match &ot {
                    Ty::Rec(r) => Some(r.clone()),
                    _ => None,
                };
                if let Some(r) = r {
                    if let Some(sig) = self.methods.get(&(r.clone(), name.clone())).cloned() {
                        // the method owns its receiver, which expr() handed over
                        let t = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", t));
                        let res = self.call_sig(&sig, Some(t), args);
                        self.free(t);
                        return res;
                    }
                    if let Some((fty @ Ty::Func { .. }, off)) = self.field_of(&r, name) {
                        // closure stored in a field: keep it alive for the call
                        let s = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", s));
                        self.e(format!("mov rax, qword ptr [rax + {}]", off));
                        self.retain_if(&fty);
                        let res = self.call_value(fty, args);
                        self.release_slot(s);
                        self.free(s);
                        return res;
                    }
                    return self.err(format!("{} has no method '{}'", r, name));
                }
                if ot == Ty::Text && name == "slice" {
                    if args.len() != 2 {
                        return self.err("slice(start, end) takes two numbers");
                    }
                    let t = self.temp();
                    let a = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    self.expr(&args[0].value, Some(&Ty::i64()))?;
                    self.e(format!("mov qword ptr [rbp - {}], rax", a));
                    self.expr(&args[1].value, Some(&Ty::i64()))?;
                    self.e("mov r8, rax");
                    self.e(format!("mov rdx, qword ptr [rbp - {}]", a));
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                    self.e("call rt_text_slice");
                    let keep = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                    self.release_slot(t);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
                    self.free(keep);
                    self.free(a);
                    self.free(t);
                    return Ok(Ty::Text);
                }
                if let Ty::Chan(el) = &ot {
                    return self.chan_method(el, name, args);
                }
                if let Ty::List(el) = &ot {
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    let res = (|| -> R<Ty> {
                        if name == "add" || name == "push" {
                            if args.len() != 1 {
                                return self.err(format!("'{}' takes one argument", name));
                            }
                            let vt = self.value(&args[0].value, Some(el))?;
                            self.check(el, &vt, "list element")?;
                            self.e("mov rdx, rax");
                            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                            self.e("call rt_list_push");
                            return Ok(Ty::Void);
                        }
                        if name == "clear" {
                            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                            self.e("call rt_list_clear");
                            return Ok(Ty::Void);
                        }
                        if name == "take" {
                            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                            self.e("call rt_list_take");
                            return Ok((**el).clone());
                        }
                        self.err(format!("{} has no method '{}'", ot.show(), name))
                    })();
                    self.release_slot(t);
                    self.free(t);
                    return res;
                }
                if let Ty::Map(k, v) = &ot {
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    let res = self.map_method(t, k, v, name, args);
                    self.release_slot(t);
                    self.free(t);
                    return res;
                }
                self.err(format!("{} has no method '{}'", ot.show(), name))
            }
            _ => {
                let _ = want;
                let t = self.expr(callee, None)?;
                self.call_value(t, args)
            }
        }
    }

    /// `has`, `get`, `remove` and `clear` on the map in slot `t` (borrowed).
    fn map_method(&mut self, t: i32, k: &Ty, v: &Ty, name: &str, args: &[Arg]) -> R<Ty> {
        let argc = match name {
            "has" | "remove" => 1,
            "get" => 2,
            "clear" => 0,
            _ => return self.err(format!("{} has no method '{}'", Ty::Map(Box::new(k.clone()), Box::new(v.clone())).show(), name)),
        };
        if args.len() != argc {
            return self.err(format!("'{}' takes {} argument(s)", name, argc));
        }
        if name == "clear" {
            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
            self.e("call rt_map_clear");
            return Ok(Ty::Void);
        }
        let kty = self.value(&args[0].value, Some(k))?;
        self.check(k, &kty, "map key")?;
        let ks = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", ks));
        let res = match name {
            "has" | "remove" => {
                self.e("mov rdx, rax");
                self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                self.e(if name == "has" { "call rt_map_has" } else { "call rt_map_remove" });
                if name == "has" { Ty::Bool } else { Ty::Void }
            }
            _ => {
                let dty = self.value(&args[1].value, Some(v))?;
                self.check(v, &dty, "default value")?;
                let ds = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", ds));
                let (miss, end) = (self.lbl("mgmiss"), self.lbl("mgend"));
                self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.e("call rt_map_find");
                self.e("test rax, rax");
                self.e(format!("jz {}", miss));
                self.e("mov rax, qword ptr [rax + 24]");
                self.retain_if(v);
                if self.is_rc(v) {
                    self.release_slot(ds);
                }
                self.e(format!("jmp {}", end));
                self.label(&miss);
                self.e(format!("mov rax, qword ptr [rbp - {}]", ds));
                self.label(&end);
                self.free(ds);
                v.clone()
            }
        };
        if *k == Ty::Text {
            self.release_slot(ks);
        }
        self.free(ks);
        Ok(res)
    }

    /// Calls the function value in rax (owned); it is released after the call.
    fn call_value(&mut self, t: Ty, args: &[Arg]) -> R<Ty> {
        let Ty::Func { params, ret, ext } = t else { return self.err(format!("cannot call a value of type {}", t.show())) };
        if params.len() != args.len() {
            return self.err(format!("expected {} argument(s), found {}", params.len(), args.len()));
        }
        let f = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", f));
        let mut temps = Vec::new();
        for (a, p) in args.iter().zip(&params) {
            let at = self.value(&a.value, Some(p))?;
            self.check(p, &at, "argument")?;
            self.norm(p);
            let t = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", t));
            temps.push(t);
        }
        self.emit_call(&temps, "", Some((f, ext)));
        if !ext {
            let keep = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", keep));
            self.release_slot(f);
            self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
            self.free(keep);
        }
        for t in temps {
            self.free(t);
        }
        self.free(f);
        self.norm(&ret);
        Ok(*ret)
    }

    fn call_sig(&mut self, sig: &Sig, receiver: Option<i32>, args: &[Arg]) -> R<Ty> {
        if sig.is_extern {
            self.require_unsafe(&format!("calling extern function '{}'", sig.sym))?;
        }
        // Bind arguments to parameters.
        let n = sig.params.len();
        let mut slots: Vec<Option<&Expr>> = vec![None; n];
        let mut rest: Vec<&Expr> = Vec::new();
        let fixed = if sig.variadic { n - 1 } else { n };
        let mut pos = 0;
        for a in args {
            if a.spread {
                return self.err("spread arguments ('...') are not supported by the native backend yet");
            }
            if let Some(name) = &a.name {
                let Some(i) = sig.params.iter().position(|p| &p.name == name) else {
                    return self.err(format!("no parameter named '{}'", name));
                };
                slots[i] = Some(&a.value);
            } else if pos < fixed {
                slots[pos] = Some(&a.value);
                pos += 1;
            } else if sig.variadic || sig.c_variadic {
                rest.push(&a.value);
            } else {
                return self.err(format!("too many arguments: expected {}", n));
            }
        }
        if self.opt && receiver.is_none() && rest.is_empty() && self.inline_depth < INLINE_DEPTH {
            if let Some((body, bm)) = self.inline_bodies.get(&sig.sym).cloned() {
                return self.inline_call(sig, &slots, &body, &bm);
            }
        }
        let mut temps = Vec::new();
        if let Some(r) = receiver {
            temps.push(r);
        }
        // (slot, type) of owned heap arguments that extern callees do not take over
        let mut extern_owned: Vec<i32> = Vec::new();
        let mut owned = Vec::new();
        // (argument position, bits) of float arguments of an extern call: Windows x64 also
        // wants them in XMM<position>
        let mut float_args: Vec<(usize, u8)> = Vec::new();
        for (i, p) in sig.params.iter().enumerate() {
            if sig.variadic && i == n - 1 {
                let Ty::List(el) = &p.ty else { unreachable!() };
                let lt = self.temp();
                let mut created = false;
                for x in &rest {
                    let vt = self.value(x, Some(el))?;
                    self.check(el, &vt, &format!("argument '{}'", p.name))?;
                    let item = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", item));
                    if !created {
                        self.new_list(&el.clone());
                        self.e(format!("mov qword ptr [rbp - {}], rax", lt));
                        created = true;
                    }
                    self.e(format!("mov rdx, qword ptr [rbp - {}]", item));
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", lt));
                    self.e("call rt_list_push");
                    self.free(item);
                }
                if !created {
                    self.new_list(&el.clone());
                    self.e(format!("mov qword ptr [rbp - {}], rax", lt));
                }
                temps.push(lt);
                owned.push(lt);
                continue;
            }
            let at = match (slots[i], &p.default) {
                (Some(x), _) => {
                    if p.by_ref {
                        self.expr(x, Some(&p.ty))?
                    } else {
                        self.value(x, Some(&p.ty))?
                    }
                }
                (None, Some(d)) => {
                    let saved = std::mem::replace(&mut self.cur_mod, sig.module.clone());
                    let r = self.value(d, Some(&p.ty));
                    self.cur_mod = saved;
                    r?
                }
                (None, None) => return self.err(format!("missing argument '{}'", p.name)),
            };
            self.check(&p.ty, &at, &format!("argument '{}'", p.name))?;
            self.norm(&p.ty);
            let t = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", t));
            if let Ty::Float { bits } = p.ty {
                float_args.push((temps.len(), bits));
            }
            temps.push(t);
            owned.push(t);
            if sig.is_extern && self.is_rc(&at) {
                extern_owned.push(t);
            }
        }
        for x in &rest {
            if sig.c_variadic {
                let mut t0 = self.value(x, None)?;
                if t0 == (Ty::Float { bits: 32 }) {
                    // C promotes float to double in variadic calls
                    self.e("movd xmm0, eax");
                    self.e("cvtss2sd xmm0, xmm0");
                    self.e("movq rax, xmm0");
                    t0 = Ty::Float { bits: 64 };
                }
                if t0.is_float() {
                    float_args.push((temps.len(), 64));
                }
                let t = self.temp();
                if t0 == Ty::Text {
                    let own = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", own));
                    extern_owned.push(own);
                    self.e("add rax, 8");
                }
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                temps.push(t);
                owned.push(t);
            }
        }
        if sig.is_extern {
            self.extern_syms.insert(sig.sym.clone());
        }
        for (idx, bits) in float_args.iter().filter(|(i, _)| *i < 4) {
            let reg = ARG_REGS[*idx];
            self.e(format!("mov {}, qword ptr [rbp - {}]", reg, temps[*idx]));
            if *bits == 32 {
                self.e(format!("movd xmm{}, {}", idx, ["ecx", "edx", "r8d", "r9d"][*idx]));
            } else {
                self.e(format!("movq xmm{}, {}", idx, reg));
            }
        }
        self.emit_call(&temps, &format!("call {}", sig.sym), None);
        if sig.is_extern {
            match sig.ret {
                Ty::Float { bits: 32 } => self.e("movd eax, xmm0"),
                Ty::Float { .. } => self.e("movq rax, xmm0"),
                _ => {}
            }
        }
        // C does not release what it is given
        for o in extern_owned {
            self.release_slot(o);
        }
        for t in owned {
            self.free(t);
        }
        self.norm(&sig.ret);
        Ok(sig.ret.clone())
    }

    /// A call of a small function compiled in place: the arguments are evaluated once, in order
    /// (literals and plain local variables are used as they are), then the function's expression
    /// is compiled in a scope holding only its parameters, in its own module.
    fn inline_call(&mut self, sig: &Sig, slots: &[Option<&Expr>], body: &Expr, module: &str) -> R<Ty> {
        let mut scope = Scope::default();
        let mut temps = Vec::new();
        for (i, p) in sig.params.iter().enumerate() {
            let Some(x) = slots[i] else { return self.err(format!("missing argument '{}'", p.name)) };
            let aliased = if let Some(v) = literal_bits(x, &p.ty) {
                Some(Local { off: 0, ty: p.ty.clone(), mutable: false, captured: false, reg: None, imm: Some(v) })
            } else if let ExprKind::Ident(n) = &x.kind {
                // a later argument that could change the variable forces a copy
                let later_ok = slots[i + 1..].iter().flatten().all(|y| reads_only(y));
                match self.lookup(n) {
                    Some(l) if later_ok && !l.captured && l.ty == p.ty => Some(Local { mutable: false, ..l }),
                    _ => None,
                }
            } else {
                None
            };
            let local = match aliased {
                Some(l) => l,
                None => {
                    let at = self.value(x, Some(&p.ty))?;
                    self.check(&p.ty, &at, &format!("argument '{}'", p.name))?;
                    self.norm(&p.ty);
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    temps.push(t);
                    Local { off: t, ty: p.ty.clone(), mutable: false, captured: false, reg: None, imm: None }
                }
            };
            scope.vars.insert(p.name.clone(), local);
        }
        let saved_scopes = std::mem::replace(&mut self.fs.scopes, vec![scope]);
        let saved_mod = std::mem::replace(&mut self.cur_mod, module.to_string());
        let saved_bindings = std::mem::take(&mut self.bindings);
        self.inline_depth += 1;
        let r = self.value(body, Some(&sig.ret)).and_then(|t| {
            self.check(&sig.ret, &t, "return value")?;
            self.norm(&sig.ret);
            Ok(())
        });
        self.inline_depth -= 1;
        self.bindings = saved_bindings;
        self.cur_mod = saved_mod;
        // heap values the body's calls left to be released belong to the caller's scope now
        let inner = std::mem::replace(&mut self.fs.scopes, saved_scopes);
        if let Some(sc) = self.fs.scopes.last_mut() {
            sc.owned.extend(inner.into_iter().flat_map(|s| s.owned));
        }
        for t in temps {
            self.free(t);
        }
        r?;
        Ok(sig.ret.clone())
    }

    /// Built-in `c` module: `c.cstr`, `c.from_cstr`, `c.alloc`, `c.free`.
    fn c_intrinsic(&mut self, name: &str, args: &[Arg]) -> R<Ty> {
        let one = |s: &Self| -> R<()> {
            if args.len() == 1 {
                Ok(())
            } else {
                s.err(format!("c.{}() takes one argument", name))
            }
        };
        let u8p = Ty::Ptr(Box::new(Ty::Int { bits: 8, signed: false }));
        match name {
            "cstr" => {
                one(self)?;
                let t = self.expr(&args[0].value, Some(&Ty::Text))?;
                self.check(&Ty::Text, &t, "c.cstr")?;
                // the text must outlive the call it is passed to: it is
                // released at the end of the statement
                let keep = self.slot();
                self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                self.fs.stmt_temps.push(keep);
                self.fs.may_own = true;
                self.e("add rax, 8");
                Ok(u8p)
            }
            "from_cstr" => {
                one(self)?;
                let (t, _) = self.operand(&args[0].value, None)?;
                self.check(&u8p, &t, "c.from_cstr")?;
                self.e("mov rcx, rax");
                self.e("call rt_text_from_cstr");
                Ok(Ty::Text)
            }
            "from_bytes" => {
                if args.len() != 2 {
                    return self.err("c.from_bytes(pointer, length) takes two arguments");
                }
                let (pt, _) = self.operand(&args[0].value, None)?;
                self.check(&u8p, &pt, "c.from_bytes")?;
                let p = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", p));
                let lt = self.expr(&args[1].value, Some(&Ty::i64()))?;
                if !lt.is_int() {
                    return self.err("c.from_bytes needs a number of bytes");
                }
                self.e("mov rdx, rax");
                self.e(format!("mov rcx, qword ptr [rbp - {}]", p));
                self.e("call rt_text_from_bytes");
                self.free(p);
                Ok(Ty::Text)
            }
            "alloc" => {
                one(self)?;
                self.expr(&args[0].value, Some(&Ty::Int { bits: 64, signed: false }))?;
                self.e("mov rcx, rax");
                self.e("xor edx, edx");
                self.e("call rt_alloc");
                Ok(u8p)
            }
            "free" => {
                one(self)?;
                self.expr(&args[0].value, None)?;
                self.e("mov rcx, rax");
                self.e("call rt_release");
                Ok(Ty::Void)
            }
            _ => self.err(format!("unknown function 'c.{}'", name)),
        }
    }

    // ---------------- defer ----------------

    /// Creates this function's list of deferred closures when its body has a `defer`.
    fn init_defers(&mut self, body: &[Stmt]) -> R<()> {
        if !block_has_defer(body) {
            return Ok(());
        }
        let list_ty = Ty::List(Box::new(Ty::Func { params: vec![], ret: Box::new(Ty::Void), ext: false }));
        let list = self.hidden_local(list_ty);
        self.e("mov ecx, 1");
        self.e("call rt_list_new");
        self.e(format!("mov qword ptr [rbp - {}], rax", list));
        let save = self.slot();
        self.fs.defer_list = Some(list);
        self.fs.may_own = true;
        self.fs.defer_save = Some(save);
        Ok(())
    }

    /// `defer call(...)`: builds a closure for the call and adds it to the list.
    fn defer_stmt(&mut self, e: &Expr, line: u32) -> R<()> {
        let Some(list) = self.fs.defer_list else {
            return self.err("`defer` is not allowed here");
        };
        if !matches!(e.kind, ExprKind::Call { .. }) {
            return self.err("`defer` needs a call, like `defer file.close()`");
        }
        let body = vec![Stmt { kind: StmtKind::Expr(e.clone()), line }];
        let want = Ty::Func { params: vec![], ret: Box::new(Ty::Void), ext: false };
        // records are shared (not copied) so the call sees them as they are at exit;
        // the result of a call that can fail is discarded
        self.next_lambda_share = true;
        self.next_lambda_ignore_result = true;
        self.lambda(&[], &body, Some(&want))?;
        self.e("mov rdx, rax");
        self.e(format!("mov rcx, qword ptr [rbp - {}]", list));
        self.e("call rt_list_push");
        Ok(())
    }

    /// Runs the deferred closures, last in first out, keeping the value in rax.
    fn emit_run_defers(&mut self) {
        let (Some(list), Some(save)) = (self.fs.defer_list, self.fs.defer_save) else { return };
        let (top, done) = (self.lbl("defer"), self.lbl("deferdone"));
        self.e(format!("mov qword ptr [rbp - {}], rax", save));
        self.label(&top);
        self.e(format!("mov rcx, qword ptr [rbp - {}]", list));
        self.e("cmp qword ptr [rcx], 0");
        self.e(format!("je {}", done));
        self.e("call rt_list_take");
        let c = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", c));
        self.emit_call(&[], "", Some((c, false)));
        self.release_slot(c);
        self.free(c);
        self.e(format!("jmp {}", top));
        self.label(&done);
        self.e(format!("mov rax, qword ptr [rbp - {}]", save));
    }

    // ---------------- futures ----------------

    fn future_fn(&self, name: &str) -> R<Sig> {
        match self.funcs.get(&("future".to_string(), name.to_string())) {
            Some(s) => Ok(s.clone()),
            None => self.err("async needs the future library"),
        }
    }

    /// Body of an `async func`: the original body becomes a task (a closure
    /// that returns the declared type) and the function returns the future
    /// that task completes.
    fn gen_async_body(&mut self, sig: &Sig, body: &[Stmt]) -> R<()> {
        let Ty::Future(inner) = sig.ret.clone() else { unreachable!() };
        self.emit_async_launch(&inner, body)
    }

    /// Runs `body` (which returns `inner`) as a task; leaves its future in rax.
    fn emit_async_launch(&mut self, inner: &Ty, body: &[Stmt]) -> R<()> {
        let inner = Box::new(inner.clone());
        let want = Ty::Func { params: vec![], ret: inner.clone(), ext: false };
        self.next_lambda_await = true;
        self.lambda(&[], body, Some(&want))?;
        let c = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", c));
        let new = self.future_fn("future_new")?;
        self.emit_call(&[], &format!("call {}", new.sym), None);
        let f = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", f));
        if self.is_rc(&inner) {
            // the result slot must own what it holds: use a list flagged as holding heap values
            let (_, off) = self.field_of("future.Future", "slot").expect("future library layout");
            self.e("mov ecx, 1");
            self.e("call rt_list_new");
            self.e(format!("mov rcx, qword ptr [rbp - {}]", f));
            self.e(format!("mov rdx, qword ptr [rcx + {}]", off));
            self.e(format!("mov qword ptr [rcx + {}], rax", off));
            self.e("mov rcx, rdx");
            self.e("call rt_release");
        }
        // `launch` takes the closure and one reference; the caller gets the other
        self.e(format!("mov rax, qword ptr [rbp - {}]", f));
        self.retain();
        let launch = self.future_fn("launch")?;
        self.emit_call(&[c, f], &format!("call {}", launch.sym), None);
        self.e(format!("mov rax, qword ptr [rbp - {}]", f));
        self.free(f);
        self.free(c);
        Ok(())
    }

    /// `timeout(ms, future)`: a future of `T or error` that fails when `ms` pass first.
    fn timeout_future(&mut self, ms_e: &Expr, f_e: &Expr) -> R<Ty> {
        let t = self.expr(ms_e, Some(&Ty::i64()))?;
        if !t.is_int() {
            return self.err(format!("timeout() needs a number of milliseconds, found {}", t.show()));
        }
        let tag = self.lbl("tm");
        let ms_name = format!("__{}_ms", tag);
        let f_name = format!("__{}_f", tag);
        let off = self.declare_local(&ms_name, Ty::i64(), false);
        self.e(format!("mov qword ptr [rbp - {}], rax", off));
        let ft = self.expr(f_e, None)?;
        let Ty::Future(inner) = ft.clone() else {
            return self.err(format!("timeout() needs a Future, found {}", ft.show()));
        };
        let off = self.declare_local(&f_name, ft, false);
        self.e(format!("mov qword ptr [rbp - {}], rax", off));
        let wait = if *inner == Ty::Void {
            format!("        await {}\n        return\n", f_name)
        } else {
            format!("        return await {}\n", f_name)
        };
        let src = format!(
            "package t\nfunc f()\n    let i = future.timeout_wait({}, {})\n    if i == 1\n{}    end\n    return error(\"timed out\")\nend\n",
            ms_name, f_name, wait
        );
        let file = parse_source(&src).map_err(|d| format!("internal: timeout template: {}", d.msg))?;
        let DeclKind::Func(fd) = &file.decls[0].kind else { unreachable!() };
        // a future that can already fail keeps a single `or error`: the failure of
        // either the operation or the timeout
        let result = match *inner {
            Ty::Result(u) => Ty::Result(u),
            t => Ty::Result(Box::new(t)),
        };
        let line = self.line;
        self.emit_async_launch(&result, &fd.body)?;
        self.line = line;
        Ok(Ty::Future(Box::new(result)))
    }

    /// `await all(xs)` / `await race(xs)` on a list of futures.
    fn await_combinator(&mut self, which: &str, arg: &Expr) -> R<Ty> {
        let lt = self.expr(arg, None)?;
        let Ty::List(el) = lt.clone() else {
            return self.err(format!("{}() needs a list of futures, found {}", which, lt.show()));
        };
        let Ty::Future(v) = *el.clone() else {
            return self.err(format!("{}() needs a list of futures, found {}", which, lt.show()));
        };
        let wait = self.future_fn("wait")?;
        self.push_scope();
        let l = self.hidden_local(lt.clone());
        self.e(format!("mov qword ptr [rbp - {}], rax", l));
        let res;
        if which == "all" {
            if *v == Ty::Void {
                return self.err("all() needs futures that return a value");
            }
            let out = self.slot();
            let idx = self.slot();
            self.new_list(&v.clone());
            self.e(format!("mov qword ptr [rbp - {}], rax", out));
            self.e(format!("mov qword ptr [rbp - {}], 0", idx));
            let (start, end) = (self.lbl("all"), self.lbl("allend"));
            self.label(&start);
            self.e(format!("mov rdx, qword ptr [rbp - {}]", idx));
            self.e(format!("mov rcx, qword ptr [rbp - {}]", l));
            self.e("cmp rdx, qword ptr [rcx]");
            self.e(format!("jge {}", end));
            self.e("call rt_list_get");
            self.retain();
            let tmp = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", tmp));
            self.emit_call(&[tmp], &format!("call {}", wait.sym), None);
            self.free(tmp);
            self.e("mov rdx, rax");
            self.e(format!("mov rcx, qword ptr [rbp - {}]", out));
            self.e("call rt_list_push");
            self.e(format!("inc qword ptr [rbp - {}]", idx));
            self.e(format!("jmp {}", start));
            self.label(&end);
            self.e(format!("mov rax, qword ptr [rbp - {}]", out));
            res = Ty::List(v);
        } else {
            let any = self.future_fn("wait_any")?;
            let arg_t = self.temp();
            self.e(format!("mov rax, qword ptr [rbp - {}]", l));
            self.retain();
            self.e(format!("mov qword ptr [rbp - {}], rax", arg_t));
            self.emit_call(&[arg_t], &format!("call {}", any.sym), None);
            self.free(arg_t);
            self.e("mov rdx, rax");
            self.e(format!("mov rcx, qword ptr [rbp - {}]", l));
            self.e("call rt_list_get");
            self.retain();
            let tmp = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", tmp));
            self.emit_call(&[tmp], &format!("call {}", wait.sym), None);
            self.free(tmp);
            self.norm(&v);
            res = *v;
        }
        self.pop_scope();
        Ok(res)
    }

    // ---------------- channels ----------------

    fn task_fn(&self, name: &str) -> R<Sig> {
        match self.funcs.get(&("task".to_string(), name.to_string())) {
            Some(s) => Ok(s.clone()),
            None => self.err("channels need the task library"),
        }
    }

    /// `channel(T)`
    fn channel_new(&mut self, args: &[Arg]) -> R<Ty> {
        if args.len() != 1 {
            return self.err("channel() takes the element type: `channel(int)`");
        }
        let ty = match &args[0].value.kind {
            ExprKind::Ident(n) => Type::Named { path: vec![n.clone()], args: vec![] },
            ExprKind::Field { expr, name, .. } => match &expr.kind {
                ExprKind::Ident(m) => Type::Named { path: vec![m.clone(), name.clone()], args: vec![] },
                _ => return self.err("channel() takes a type, like `channel(int)`"),
            },
            _ => return self.err("channel() takes a type, like `channel(int)`"),
        };
        let el = self.resolve(&ty, &self.cur_mod.clone())?;
        let sig = self.task_fn("chan_new")?;
        self.emit_call(&[], &format!("call {}", sig.sym), None);
        Ok(Ty::Chan(Box::new(el)))
    }

    /// `ch.send(v)`, `ch.receive()`, `ch.close()`; the channel is in rax (owned).
    fn chan_method(&mut self, el: &Ty, name: &str, args: &[Arg]) -> R<Ty> {
        let c = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", c));
        let res = match name {
            "send" => {
                if args.len() != 1 {
                    return self.err("'send' takes one value");
                }
                let vt = self.value(&args[0].value, Some(el))?;
                self.check(el, &vt, "value sent on the channel")?;
                self.norm(el);
                let v = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", v));
                let sig = self.task_fn("chan_send")?;
                self.emit_call(&[c, v], &format!("call {}", sig.sym), None);
                self.free(v);
                Ty::Void
            }
            "receive" => {
                let sig = self.task_fn("chan_recv")?;
                self.emit_call(&[c], &format!("call {}", sig.sym), None);
                self.norm(el);
                el.clone()
            }
            "close" => {
                let sig = self.task_fn("chan_close")?;
                self.emit_call(&[c], &format!("call {}", sig.sym), None);
                Ty::Void
            }
            _ => return self.err(format!("a channel has no method '{}'; use send, receive or close", name)),
        };
        self.free(c);
        Ok(res)
    }
}

// ---------------------------------------------------------------------------
// Generics and interfaces
//
// A generic function or record is a template. Using it with concrete types makes an *instance*:
// the template's declaration compiled with its type parameters bound to those types
// (`Compiler::bindings`), under a name that includes the type arguments. Each template is also
// compiled once with opaque `Ty::Param` types, so mistakes are reported at the definition.
// ---------------------------------------------------------------------------

/// Full spelling of a type, with module prefixes: the key of an instance.
fn ty_full(t: &Ty) -> String {
    match t {
        Ty::Rec(n) | Ty::Enum(n) => n.clone(),
        Ty::List(e) => format!("[]{}", ty_full(e)),
        Ty::Map(k, v) => format!("Map<{},{}>", ty_full(k), ty_full(v)),
        Ty::Ptr(e) => format!("*{}", ty_full(e)),
        Ty::Chan(e) => format!("Channel<{}>", ty_full(e)),
        Ty::Future(e) => format!("Future<{}>", ty_full(e)),
        Ty::Result(e) => format!("{}!", ty_full(e)),
        other => other.show(),
    }
}

/// Does the type mention one of the type parameters?
fn mentions_param(t: &Type, names: &[String]) -> bool {
    match t {
        Type::Named { path, args } => {
            (path.len() == 1 && args.is_empty() && names.contains(&path[0])) || args.iter().any(|a| mentions_param(a, names))
        }
        Type::Optional(i) | Type::Pointer(i) | Type::Slice(i) | Type::Array(_, i) => mentions_param(i, names),
        Type::Func { params, ret, .. } => {
            params.iter().any(|p| mentions_param(p, names))
                || ret.as_ref().map_or(false, |r| r.ty.as_ref().map_or(false, |t| mentions_param(t, names)))
        }
    }
}

const INSTANCE_LIMIT: usize = 2000;

impl Compiler {
    /// Qualified name of the generic record whose methods `fd` belongs to.
    fn generic_receiver(&self, m: &str, fd: &FuncDecl) -> Option<String> {
        let r = fd.receiver.as_ref()?;
        let Type::Named { path, args } = &r.ty else { return None };
        if args.is_empty() {
            return None;
        }
        let q = if path.len() == 1 { format!("{}.{}", m, path[0]) } else { path.join(".") };
        self.tmpl_recs.contains_key(&q).then_some(q)
    }

    fn snapshot(&mut self) -> Snapshot {
        Snapshot {
            fs: self.fs.clone(),
            procs: self.procs.len(),
            data: self.data.len(),
            strings: self.strings.clone(),
            records: self.records.clone(),
            rec_decls: self.rec_decls.clone(),
            shared_recs: self.shared_recs.clone(),
            methods: self.methods.clone(),
            funcs: self.funcs.clone(),
            inst_args: self.inst_args.clone(),
            inst_funcs: self.inst_funcs.clone(),
            pending: std::mem::take(&mut self.pending),
            fn_closures: self.fn_closures.clone(),
            bindings: self.bindings.clone(),
            cur_mod: self.cur_mod.clone(),
            line: self.line,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.fs = s.fs;
        self.procs.truncate(s.procs);
        self.data.truncate(s.data);
        self.strings = s.strings;
        self.records = s.records;
        self.rec_decls = s.rec_decls;
        self.shared_recs = s.shared_recs;
        self.methods = s.methods;
        self.funcs = s.funcs;
        self.inst_args = s.inst_args;
        self.inst_funcs = s.inst_funcs;
        self.pending = s.pending;
        self.fn_closures = s.fn_closures;
        self.bindings = s.bindings;
        self.cur_mod = s.cur_mod;
        self.line = s.line;
    }

    fn resolve_ret(&mut self, r: &Option<RetType>, m: &str) -> R<Ty> {
        match r {
            Some(r) => {
                let inner = match &r.ty {
                    Some(t) => self.resolve(t, m)?,
                    None => Ty::Void,
                };
                Ok(if r.fallible { Ty::Result(Box::new(inner)) } else { inner })
            }
            None => Ok(Ty::Void),
        }
    }

    // ---------------- interfaces ----------------

    /// The qualified interface a bound (`Printable`, `pkg.Printable`) written in module `m` names.
    fn bound_name(&self, bound: &[String], m: &str) -> R<String> {
        let q = if bound.len() == 1 { format!("{}.{}", m, bound[0]) } else { bound.join(".") };
        if self.interfaces.contains_key(&q) {
            Ok(q)
        } else if self.rec_decls.contains_key(&q) || self.enum_decls.contains_key(&q) || self.tmpl_recs.contains_key(&q) {
            self.err(format!("the bound '{}' must be an interface", bound.join(".")))
        } else {
            self.err(format!("unknown interface '{}'", bound.join(".")))
        }
    }

    /// Parameter and result types of the interface method `sg`.
    fn interface_method(&mut self, iface_mod: &str, sg: &MethodSig) -> R<(Vec<Ty>, Ty)> {
        let saved = std::mem::take(&mut self.bindings);
        let mut ps = Vec::new();
        let mut result = Ok(());
        for p in &sg.params {
            match &p.ty {
                Some(t) => match self.resolve(t, iface_mod) {
                    Ok(t) => ps.push(t),
                    Err(e) => result = Err(e),
                },
                None => result = self.err(format!("parameter '{}' of '{}' needs a type", p.name, sg.name)),
            }
        }
        let ret = self.resolve_ret(&sg.ret, iface_mod);
        self.bindings = saved;
        result?;
        Ok((ps, ret?))
    }

    /// Checks that `ty` has every method of the interface `iface`, with the same signature.
    fn satisfies(&mut self, ty: &Ty, iface: &str) -> R<()> {
        let (im, sigs) = self.interfaces[iface].clone();
        let iname = display_name(iface);
        for sg in &sigs {
            let (want_params, want_ret) = self.interface_method(&im, sg)?;
            let have: Option<(Vec<Ty>, Ty)> = match ty {
                Ty::Param { bound: Some(b), .. } => {
                    let (bm, bsigs) = self.interfaces[b].clone();
                    match bsigs.iter().find(|x| x.name == sg.name) {
                        Some(x) => Some(self.interface_method(&bm, x)?),
                        None => None,
                    }
                }
                Ty::Param { .. } => None,
                _ => self.methods.get(&(method_key(ty), sg.name.clone())).map(|m| (m.params.iter().map(|p| p.ty.clone()).collect(), m.ret.clone())),
            };
            let Some((have_params, have_ret)) = have else {
                return self.err(format!("{} does not satisfy {}: it has no method '{}'", ty.show(), iname, sg.name));
            };
            if have_params != want_params {
                let show = |v: &[Ty]| v.iter().map(|t| t.show()).collect::<Vec<_>>().join(", ");
                return self.err(format!(
                    "{} does not satisfy {}: method '{}' takes ({}) where {} needs ({})",
                    ty.show(),
                    iname,
                    sg.name,
                    show(&have_params),
                    iname,
                    show(&want_params)
                ));
            }
            if have_ret != want_ret {
                let r = |t: &Ty| if *t == Ty::Void { "nothing".to_string() } else { t.show() };
                return self.err(format!(
                    "{} does not satisfy {}: method '{}' returns {} where {} needs {}",
                    ty.show(),
                    iname,
                    sg.name,
                    r(&have_ret),
                    iname,
                    r(&want_ret)
                ));
            }
        }
        Ok(())
    }

    fn check_bound(&mut self, ty: &Ty, iface: &str) -> R<()> {
        if !self.declared {
            self.deferred_bounds.push((ty.clone(), iface.to_string(), self.line));
            return Ok(());
        }
        self.satisfies(ty, iface)
    }

    /// A method call on a value whose type is a type parameter: only the bound's methods exist.
    fn param_method_call(&mut self, name: &str, bound: &Option<String>, tname: &str, mname: &str, args: &[Arg]) -> R<Ty> {
        let Some(b) = bound else {
            return self.err(format!("{} has no method '{}': add a bound, for example <{}: Printable>", tname, mname, tname));
        };
        let (im, sigs) = self.interfaces[b].clone();
        let Some(sg) = sigs.iter().find(|s| s.name == mname) else {
            return self.err(format!("{} has no method '{}': its bound {} does not list it", tname, mname, display_name(b)));
        };
        let _ = name;
        let (ps, ret) = self.interface_method(&im, sg)?;
        if args.len() != ps.len() {
            return self.err(format!("'{}' takes {} argument{}", mname, ps.len(), if ps.len() == 1 { "" } else { "s" }));
        }
        for (a, pt) in args.iter().zip(&ps) {
            let at = self.expr(&a.value, Some(pt))?;
            self.check(pt, &at, &format!("argument of '{}'", mname))?;
        }
        self.e("xor eax, eax");
        Ok(ret)
    }

    // ---------------- generic records ----------------

    fn instantiate_record(&mut self, q: &str, args: &[Type], m: &str) -> R<Ty> {
        let (tm, td, shared) = self.tmpl_recs[q].clone();
        let TypeBody::Record(fields) = &td.body else { unreachable!() };
        let n = td.generics.len();
        if args.len() != n {
            return self.err(format!(
                "{} takes {} type argument{}, found {}",
                td.name,
                n,
                if n == 1 { "" } else { "s" },
                args.len()
            ));
        }
        let mut tys = Vec::new();
        for a in args {
            tys.push(self.resolve(a, m)?);
        }
        for (tp, ty) in td.generics.iter().zip(&tys) {
            if let Some(b) = &tp.bound {
                let bq = self.bound_name(b, &tm)?;
                self.check_bound(ty, &bq)?;
            }
        }
        let full: Vec<String> = tys.iter().map(ty_full).collect();
        let name = format!("{}<{}>", q, full.join(","));
        if self.rec_decls.contains_key(&name) {
            return Ok(Ty::Rec(name));
        }
        self.bump_instances()?;
        // registered before the layout, so a record may mention itself (`next: Node<T>?`)
        self.rec_decls.insert(name.clone(), (tm.clone(), Vec::new()));
        self.inst_args.insert(name.clone(), (q.to_string(), tys.clone()));
        if shared {
            self.shared_recs.insert(name.clone());
        }
        let binds: HashMap<String, Ty> = td.generics.iter().map(|g| g.name.clone()).zip(tys.iter().cloned()).collect();
        let saved = std::mem::replace(&mut self.bindings, binds);
        let info = self.layout(fields, &tm);
        self.bindings = saved;
        self.records.insert(name.clone(), info?);
        let methods = self.tmpl_methods.get(q).cloned().unwrap_or_default();
        for (mm, fd, line) in methods {
            self.instantiate_method(&name, &mm, &fd, line, &tys)?;
        }
        Ok(Ty::Rec(name))
    }

    fn instantiate_method(&mut self, rec_inst: &str, module: &str, fd: &FuncDecl, line: u32, tys: &[Ty]) -> R<()> {
        if self.methods.contains_key(&(rec_inst.to_string(), fd.name.clone())) {
            return Ok(());
        }
        let Some(Type::Named { args: rargs, .. }) = fd.receiver.as_ref().map(|r| r.ty.clone()) else { unreachable!() };
        let mut binds = HashMap::new();
        for (a, t) in rargs.iter().zip(tys) {
            match a {
                Type::Named { path, args } if path.len() == 1 && args.is_empty() => {
                    binds.insert(path[0].clone(), t.clone());
                }
                _ => return self.err("the receiver of a method of a generic record must list its type parameters, like Stack<T>"),
            }
        }
        if rargs.len() != tys.len() {
            return self.err(format!("the receiver of '{}' must have {} type parameter(s)", fd.name, tys.len()));
        }
        let saved = std::mem::replace(&mut self.bindings, binds.clone());
        let saved_mod = std::mem::replace(&mut self.cur_mod, module.to_string());
        let saved_line = std::mem::replace(&mut self.line, line);
        let sig = self.func_sig(module, fd);
        self.bindings = saved;
        self.cur_mod = saved_mod;
        self.line = saved_line;
        let sig = sig?;
        self.methods.insert((rec_inst.to_string(), fd.name.clone()), sig);
        self.pending.push(Pending { module: module.to_string(), decl: fd.clone(), bindings: binds, line });
        Ok(())
    }

    fn bump_instances(&mut self) -> R<()> {
        self.inst_count += 1;
        if self.inst_count > INSTANCE_LIMIT {
            return self.err("too many generic instances: does a generic function or record use itself with ever larger types?");
        }
        Ok(())
    }

    /// The generic record a literal `Name { ... }` builds: the expected type decides the arguments.
    fn generic_literal(&mut self, path: &[String], want: Option<&Ty>) -> R<Option<String>> {
        let m = self.cur_mod.clone();
        let q = if path.len() == 1 { format!("{}.{}", m, path[0]) } else { path.join(".") };
        if !self.tmpl_recs.contains_key(&q) {
            return Ok(None);
        }
        if let Some(Ty::Rec(inst)) = want {
            if self.inst_args.get(inst).map_or(false, |(tq, _)| *tq == q) {
                return Ok(Some(inst.clone()));
            }
        }
        let name = path.last().unwrap();
        let params: Vec<String> = self.tmpl_recs[&q].1.generics.iter().map(|g| g.name.clone()).collect();
        self.err(format!(
            "cannot infer the type arguments of {}: give the expected type, for example `let x: {}<{}> = ...`",
            name,
            name,
            params.iter().map(|_| "int").collect::<Vec<_>>().join(", ")
        ))
    }

    // ---------------- generic functions ----------------

    fn unify(&mut self, pat: &Type, actual: &Ty, params: &[String], b: &mut HashMap<String, Ty>, m: &str) -> R<()> {
        match pat {
            Type::Named { path, args } => {
                if path.len() == 1 && args.is_empty() && params.contains(&path[0]) {
                    match b.get(&path[0]) {
                        Some(prev) if prev != actual && !(prev.is_int() && actual.is_int()) => {
                            return self.err(format!(
                                "type parameter {} is both {} and {} in this call",
                                path[0],
                                prev.show(),
                                actual.show()
                            ));
                        }
                        Some(_) => {}
                        None => {
                            b.insert(path[0].clone(), actual.clone());
                        }
                    }
                    return Ok(());
                }
                if path.len() == 1 {
                    match (path[0].as_str(), actual) {
                        ("List", Ty::List(el)) if args.len() == 1 => return self.unify(&args[0], el, params, b, m),
                        ("Map", Ty::Map(k, v)) if args.len() == 2 => {
                            self.unify(&args[0], k, params, b, m)?;
                            return self.unify(&args[1], v, params, b, m);
                        }
                        ("Channel", Ty::Chan(el)) if args.len() == 1 => return self.unify(&args[0], el, params, b, m),
                        ("Future", Ty::Future(el)) if args.len() == 1 => return self.unify(&args[0], el, params, b, m),
                        _ => {}
                    }
                }
                let q = if path.len() == 1 { format!("{}.{}", m, path[0]) } else { path.join(".") };
                if self.tmpl_recs.contains_key(&q) {
                    if let Ty::Rec(inst) = actual {
                        if let Some((tq, targs)) = self.inst_args.get(inst).cloned() {
                            if tq == q && targs.len() == args.len() {
                                for (a, t) in args.iter().zip(&targs) {
                                    self.unify(a, t, params, b, m)?;
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            Type::Optional(i) => self.unify(i, actual, params, b, m),
            Type::Pointer(i) => match actual {
                Ty::Ptr(e) => self.unify(i, e, params, b, m),
                _ => Ok(()),
            },
            Type::Slice(i) => match actual {
                Ty::List(e) => self.unify(i, e, params, b, m),
                _ => Ok(()),
            },
            Type::Func { params: ps, ret, .. } => {
                if let Ty::Func { params: aps, ret: ar, .. } = actual {
                    for (p, a) in ps.iter().zip(aps) {
                        self.unify(p, a, params, b, m)?;
                    }
                    if let Some(t) = ret.as_ref().and_then(|r| r.ty.as_ref()) {
                        self.unify(t, ar, params, b, m)?;
                    }
                }
                Ok(())
            }
            Type::Array(..) => Ok(()),
        }
    }

    /// A call to a generic function: infer the type arguments, make (or reuse) the instance, call it.
    fn call_generic(&mut self, module: &str, fd: &FuncDecl, decl_line: u32, args: &[Arg], want: Option<&Ty>) -> R<Ty> {
        let names: Vec<String> = fd.generics.iter().map(|g| g.name.clone()).collect();
        let mut b: HashMap<String, Ty> = HashMap::new();
        let mut next = 0;
        let mut lambdas: Vec<(Type, &Expr)> = Vec::new();
        for a in args {
            if a.spread {
                return self.err("spread arguments ('...') are not supported by the native backend yet");
            }
            let pi = match &a.name {
                Some(n) => fd.params.iter().position(|p| &p.name == n),
                None => {
                    next += 1;
                    Some(next - 1)
                }
            };
            let Some(p) = pi.and_then(|i| fd.params.get(i)) else { continue };
            let Some(pt) = &p.ty else { continue };
            // a lambda with untyped parameters is typed from the parameter type once that is known
            if matches!(a.value.kind, ExprKind::Lambda { .. }) {
                if !p.variadic && mentions_param(pt, &names) {
                    lambdas.push((pt.clone(), &a.value));
                }
                continue;
            }
            if p.variadic || !mentions_param(pt, &names) {
                continue;
            }
            // type the argument by compiling it into a scratch buffer
            let snap = self.snapshot();
            let r = self.expr(&a.value, None);
            self.restore(snap);
            let aty = r?;
            self.unify(pt, &aty, &names, &mut b, module)?;
        }
        // lambdas: their parameter types come from the bindings so far; their body gives the rest
        for (pt, lam) in lambdas {
            let Type::Func { params: fps, ret: fret, .. } = &pt else { continue };
            let saved = std::mem::replace(&mut self.bindings, b.clone());
            let mut ptys = Vec::new();
            let mut open = None;
            for fp in fps {
                if mentions_param(fp, &names.iter().filter(|n| !b.contains_key(*n)).cloned().collect::<Vec<_>>()) {
                    open = Some(fp.clone());
                    break;
                }
                match self.resolve(fp, module) {
                    Ok(t) => ptys.push(t),
                    Err(e) => {
                        self.bindings = saved;
                        return Err(e);
                    }
                }
            }
            self.bindings = saved;
            if open.is_some() {
                continue;
            }
            let _ = fret;
            let want_fn = Ty::Func { params: ptys, ret: Box::new(Ty::Param { name: "?".into(), bound: None }), ext: false };
            let snap = self.snapshot();
            let r = self.expr(lam, Some(&want_fn));
            self.restore(snap);
            let lt = r?;
            self.unify(&pt, &lt, &names, &mut b, module)?;
        }
        if let (Some(w), Some(t)) = (want, fd.ret.as_ref().and_then(|r| r.ty.as_ref())) {
            let mut extra = HashMap::new();
            if self.unify(t, w, &names, &mut extra, module).is_ok() {
                for (k, v) in extra {
                    b.entry(k).or_insert(v);
                }
            }
        }
        for n in &names {
            if !b.contains_key(n) {
                return self.err(format!(
                    "cannot infer the type of {} in the call to '{}': no argument determines it; give the result an annotated type or pass a value of that type",
                    n, fd.name
                ));
            }
        }
        for n in &names {
            if b[n] == Ty::Void {
                return self.err(format!(
                    "the type {} in the call to '{}' would be 'nothing': a function argument must return a value here",
                    n, fd.name
                ));
            }
        }
        for g in &fd.generics {
            if let Some(bq) = &g.bound {
                let q = self.bound_name(bq, module)?;
                let ty = b[&g.name].clone();
                self.check_bound(&ty, &q)?;
            }
        }
        let argkey = names.iter().map(|n| ty_full(&b[n])).collect::<Vec<_>>().join(",");
        let key = (module.to_string(), fd.name.clone(), argkey.clone());
        let inst = match self.inst_funcs.get(&key) {
            Some(n) => n.clone(),
            None => {
                self.bump_instances()?;
                let inst = format!("{}__{}", fd.name, sym_of(&argkey));
                let mut decl = fd.clone();
                decl.generics.clear();
                decl.name = inst.clone();
                let saved = std::mem::replace(&mut self.bindings, b.clone());
                let saved_mod = std::mem::replace(&mut self.cur_mod, module.to_string());
                let sig = self.func_sig(module, &decl);
                self.bindings = saved;
                self.cur_mod = saved_mod;
                self.funcs.insert((module.to_string(), inst.clone()), sig?);
                self.inst_funcs.insert(key, inst.clone());
                self.pending.push(Pending { module: module.to_string(), decl, bindings: b, line: decl_line });
                inst
            }
        };
        let sig = self.funcs[&(module.to_string(), inst)].clone();
        self.call_sig(&sig, None, args)
    }

    // ---------------- driving ----------------

    /// Compiles every instance requested so far (and those they request).
    fn drain_pending(&mut self) -> R<()> {
        while let Some(p) = self.pending.pop() {
            self.cur_mod = p.module.clone();
            self.line = p.line;
            self.bindings = p.bindings.clone();
            let r = self.gen_func(&p.module, &p.decl, false);
            self.bindings.clear();
            r?;
        }
        Ok(())
    }

    /// Checks each template once, with its type parameters as opaque types, and runs the bound
    /// checks that had to wait until every method was known.
    fn check_templates(&mut self) -> R<()> {
        let deferred = std::mem::take(&mut self.deferred_bounds);
        for (ty, iface, line) in deferred {
            self.line = line;
            self.satisfies(&ty, &iface)?;
        }
        let mut fkeys: Vec<(String, String)> = self.tmpl_funcs.keys().cloned().collect();
        fkeys.sort();
        for key in fkeys {
            let (fd, line) = self.tmpl_funcs[&key].clone();
            let snap = self.snapshot();
            let r = self.check_function(&key.0, &fd, line);
            self.restore(snap);
            r?;
        }
        let mut rkeys: Vec<String> = self.tmpl_recs.keys().cloned().collect();
        rkeys.sort();
        for q in rkeys {
            let snap = self.snapshot();
            let r = self.check_record(&q);
            self.restore(snap);
            r?;
        }
        Ok(())
    }

    fn param_bindings(&self, generics: &[TypeParam], m: &str) -> R<HashMap<String, Ty>> {
        let mut b = HashMap::new();
        for g in generics {
            let bound = match &g.bound {
                Some(bq) => Some(self.bound_name(bq, m)?),
                None => None,
            };
            b.insert(g.name.clone(), Ty::Param { name: g.name.clone(), bound });
        }
        Ok(b)
    }

    fn check_function(&mut self, m: &str, fd: &FuncDecl, line: u32) -> R<()> {
        self.cur_mod = m.to_string();
        self.line = line;
        self.bindings = self.param_bindings(&fd.generics, m)?;
        let mut decl = fd.clone();
        decl.generics.clear();
        decl.name = format!("__check_{}", fd.name);
        let sig = self.func_sig(m, &decl)?;
        self.funcs.insert((m.to_string(), decl.name.clone()), sig);
        let r = self.gen_func(m, &decl, false);
        self.bindings.clear();
        r
    }

    fn check_record(&mut self, q: &str) -> R<()> {
        let (tm, td, _) = self.tmpl_recs[q].clone();
        self.cur_mod = tm.clone();
        self.line = 0;
        let binds = self.param_bindings(&td.generics, &tm)?;
        self.bindings = binds;
        let args: Vec<Type> = td.generics.iter().map(|g| Type::Named { path: vec![g.name.clone()], args: vec![] }).collect();
        self.instantiate_record(q, &args, &tm)?;
        // the methods of this instance are the template's own
        let todo = std::mem::take(&mut self.pending);
        for p in todo {
            self.cur_mod = p.module.clone();
            self.line = p.line;
            self.bindings = p.bindings.clone();
            self.gen_func(&p.module, &p.decl, false)?;
        }
        self.bindings.clear();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Constant folding
//
// An expression made only of literals is computed here instead of by the program. The rules are
// exactly the ones the generated code follows: integers wrap at the width of their type (each
// operation is normalized, as `binary` does), shifts use the count the processor masks, floats
// follow IEEE 754, and a division that could fail at run time is never folded.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Const {
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// `v` as the register value an integer of type `t` holds after normalization.
fn norm_val(v: i64, t: &Ty) -> i64 {
    match t {
        Ty::Int { bits: 8, signed: true } => v as i8 as i64,
        Ty::Int { bits: 8, signed: false } => v as u8 as i64,
        Ty::Int { bits: 16, signed: true } => v as i16 as i64,
        Ty::Int { bits: 16, signed: false } => v as u16 as i64,
        Ty::Int { bits: 32, signed: true } => v as i32 as i64,
        Ty::Int { bits: 32, signed: false } => v as u32 as i64,
        _ => v,
    }
}

/// An assembler spelling of a 64-bit constant that `mov` accepts for any value.
fn imm64(v: i64) -> String {
    if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
        v.to_string()
    } else {
        format!("0{:X}h", v as u64)
    }
}

impl Compiler {
    /// The value of `e` when it is made of literals only. `t` is the integer type its literals
    /// have, `fbits` the float width.
    fn fold(&self, e: &Expr, t: &Ty, fbits: u8) -> Option<Const> {
        match &e.kind {
            ExprKind::Int(n) => Some(Const::Int(*n as i64)),
            ExprKind::Float(f) => Some(Const::Float(if fbits == 32 { *f as f32 as f64 } else { *f })),
            ExprKind::Bool(b) => Some(Const::Bool(*b)),
            ExprKind::Unary { op, expr } => {
                let v = self.fold(expr, t, fbits)?;
                match (*op, v) {
                    ("-", Const::Int(n)) => Some(Const::Int(norm_val(n.wrapping_neg(), t))),
                    ("-", Const::Float(f)) => Some(Const::Float(-f)),
                    ("not", Const::Bool(b)) => Some(Const::Bool(!b)),
                    ("bitnot", Const::Int(n)) => Some(Const::Int(norm_val(!n, t))),
                    _ => None,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let is_cmp = matches!(*op, "==" | "!=" | "<" | "<=" | ">" | ">=");
                if is_cmp {
                    // comparison operands are typed on their own: literals are plain `int`s
                    let i64t = Ty::i64();
                    let (a, b) = (self.fold(lhs, &i64t, fbits)?, self.fold(rhs, &i64t, fbits)?);
                    let r = match (a, b) {
                        (Const::Int(a), Const::Int(b)) => cmp_op(*op, a.cmp(&b)),
                        (Const::Float(a), Const::Float(b)) => match *op {
                            "==" => a == b,
                            "!=" => a != b,
                            "<" => a < b,
                            "<=" => a <= b,
                            ">" => a > b,
                            _ => a >= b,
                        },
                        (Const::Bool(a), Const::Bool(b)) => match *op {
                            "==" => a == b,
                            "!=" => a != b,
                            _ => return None,
                        },
                        _ => return None,
                    };
                    return Some(Const::Bool(r));
                }
                let (a, b) = (self.fold(lhs, t, fbits)?, self.fold(rhs, t, fbits)?);
                match (a, b) {
                    (Const::Bool(a), Const::Bool(b)) => match *op {
                        "and" => Some(Const::Bool(a && b)),
                        "or" => Some(Const::Bool(a || b)),
                        _ => None,
                    },
                    (Const::Int(a), Const::Int(b)) => {
                        let unsigned = t.is_unsigned();
                        let v = match *op {
                            "+" => a.wrapping_add(b),
                            "-" => a.wrapping_sub(b),
                            "*" => a.wrapping_mul(b),
                            "/" | "%" => {
                                // a zero divisor (and the one overflowing signed division) must still
                                // fail at run time
                                if b == 0 || (!unsigned && a == i64::MIN && b == -1) {
                                    return None;
                                }
                                if unsigned {
                                    let (x, y) = (a as u64, b as u64);
                                    (if *op == "/" { x / y } else { x % y }) as i64
                                } else if *op == "/" {
                                    a / b
                                } else {
                                    a % b
                                }
                            }
                            "bitand" => a & b,
                            "bitor" => a | b,
                            "bitxor" => a ^ b,
                            "<<" => a.wrapping_shl((b & 63) as u32),
                            ">>" => {
                                let n = (b & 63) as u32;
                                if unsigned {
                                    ((a as u64) >> n) as i64
                                } else {
                                    a >> n
                                }
                            }
                            _ => return None,
                        };
                        Some(Const::Int(norm_val(v, t)))
                    }
                    (Const::Float(a), Const::Float(b)) => {
                        let v = match *op {
                            "+" => a + b,
                            "-" => a - b,
                            "*" => a * b,
                            "/" => a / b,
                            _ => return None,
                        };
                        // an f32 operation rounds to f32
                        Some(Const::Float(if fbits == 32 { v as f32 as f64 } else { v }))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Emits a folded constant and returns its type.
    fn emit_const(&mut self, c: Const, t: &Ty, fbits: u8) -> Ty {
        match c {
            Const::Int(v) => {
                self.e(format!("mov rax, {}", imm64(v)));
                t.clone()
            }
            Const::Bool(b) => {
                self.e(format!("mov eax, {}", b as i32));
                Ty::Bool
            }
            Const::Float(f) => {
                if fbits == 32 {
                    let b = (f as f32).to_bits();
                    if b == 0 {
                        self.e("xor eax, eax");
                    } else {
                        self.e(format!("mov eax, {}", imm64(b as i64)));
                    }
                } else {
                    let b = f.to_bits();
                    if b == 0 {
                        self.e("xor eax, eax");
                    } else {
                        self.e(format!("mov rax, {}", imm64(b as i64)));
                    }
                }
                Ty::Float { bits: fbits }
            }
        }
    }
}

fn cmp_op(op: &str, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        "==" => o == Equal,
        "!=" => o != Equal,
        "<" => o == Less,
        "<=" => o != Greater,
        ">" => o == Greater,
        _ => o != Less,
    }
}

// ---------------------------------------------------------------------------
// Conditions that jump
//
// `if` and `while` do not need the 0 or 1 a comparison produces: they jump on the processor
// flags the comparison sets. `and`, `or` and `not` become jumps as well.
// ---------------------------------------------------------------------------

fn inverse_cc(cc: &str) -> &'static str {
    match cc {
        "e" => "ne",
        "ne" => "e",
        "l" => "ge",
        "ge" => "l",
        "le" => "g",
        "g" => "le",
        "b" => "ae",
        "ae" => "b",
        "be" => "a",
        _ => "be",
    }
}

fn is_cmp_op(op: &str) -> bool {
    matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=")
}

impl Compiler {
    /// A right operand `simple_rhs` can load without evaluating anything.
    fn rhs_is_simple(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) => true,
            ExprKind::Ident(n) => self.lookup(n).map_or(false, |l| !l.captured),
            _ => false,
        }
    }

    /// Jumps to `label` when `cond` is `jump_if`, and falls through otherwise.
    fn cond_jump(&mut self, cond: &Expr, label: &str, jump_if: bool) -> R<()> {
        self.line = cond.line;
        let m = self.fs.stmt_temps.len();
        if self.opt {
            match &cond.kind {
                ExprKind::Unary { op: "not", expr } => return self.cond_jump(expr, label, !jump_if),
                ExprKind::Binary { op: "and", lhs, rhs } => {
                    if jump_if {
                        let skip = self.lbl("andskip");
                        self.cond_jump(lhs, &skip, false)?;
                        self.cond_jump(rhs, label, true)?;
                        self.label(&skip);
                    } else {
                        self.cond_jump(lhs, label, false)?;
                        self.cond_jump(rhs, label, false)?;
                    }
                    return Ok(());
                }
                ExprKind::Binary { op: "or", lhs, rhs } => {
                    if jump_if {
                        self.cond_jump(lhs, label, true)?;
                        self.cond_jump(rhs, label, true)?;
                    } else {
                        let ok = self.lbl("orok");
                        self.cond_jump(lhs, &ok, true)?;
                        self.cond_jump(rhs, label, false)?;
                        self.label(&ok);
                    }
                    return Ok(());
                }
                ExprKind::Binary { op, lhs, rhs } if is_cmp_op(op) && self.rhs_is_simple(rhs) => {
                    let (lt, lown) = self.operand(lhs, None)?;
                    if lown && self.is_rc(&lt) {
                        // an owned heap value on the left: the general path releases it
                        let t1 = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", t1));
                        let rt = self.simple_rhs(rhs, &lt).unwrap();
                        self.binary_core(op, &lt, &rt, true)?;
                        self.release_slot(t1);
                        self.free(t1);
                        self.flush_temps(m);
                        self.e("test rax, rax");
                        self.e(format!("{} {}", if jump_if { "jnz" } else { "jz" }, label));
                        return Ok(());
                    }
                    let rt = self.simple_rhs(rhs, &lt).unwrap();
                    if self.fs.stmt_temps.len() == m && self.fuse_compare(op, &lt, &rt, label, jump_if) {
                        return Ok(());
                    }
                    self.binary_core(op, &lt, &rt, true)?;
                    self.flush_temps(m);
                    self.e("test rax, rax");
                    self.e(format!("{} {}", if jump_if { "jnz" } else { "jz" }, label));
                    return Ok(());
                }
                _ => {}
            }
        }
        self.expr(cond, Some(&Ty::Bool))?;
        self.flush_temps(m);
        self.e("test rax, rax");
        self.e(format!("{} {}", if jump_if { "jnz" } else { "jz" }, label));
        Ok(())
    }

    /// With the operands in rax and rcx, compares them and jumps. Returns false, having emitted
    /// nothing, when the types need the general code (text, enums, results, type errors).
    fn fuse_compare(&mut self, op: &str, lt: &Ty, rt: &Ty, label: &str, jump_if: bool) -> bool {
        let intlike = |t: &Ty| t.is_int() || matches!(t, Ty::Bool | Ty::Ptr(_));
        if intlike(lt) && intlike(rt) && (compatible(lt, rt) || compatible(rt, lt)) {
            let unsigned = lt.is_unsigned() || rt.is_unsigned();
            let cc = match (op, unsigned) {
                ("==", _) => "e",
                ("!=", _) => "ne",
                ("<", false) => "l",
                ("<=", false) => "le",
                (">", false) => "g",
                (">=", false) => "ge",
                ("<", true) => "b",
                ("<=", true) => "be",
                (">", true) => "a",
                _ => "ae",
            };
            self.e("cmp rax, rcx");
            let cc = if jump_if { cc } else { inverse_cc(cc) };
            self.e(format!("j{} {}", cc, label));
            return true;
        }
        if let (Ty::Float { bits: a }, Ty::Float { bits: b }) = (lt, rt) {
            if a == b {
                let d = *a == 64;
                let (mv, uc) = if d { ("movq", "ucomisd") } else { ("movd", "ucomiss") };
                let (x, y) = if d { ("rax", "rcx") } else { ("eax", "ecx") };
                self.e(format!("{} xmm0, {}", mv, x));
                self.e(format!("{} xmm1, {}", mv, y));
                // unordered (a NaN) sets ZF, PF and CF: only `ne` is true then
                match op {
                    ">" | ">=" | "<" | "<=" => {
                        let swap = op.starts_with('<');
                        if swap {
                            self.e(format!("{} xmm1, xmm0", uc));
                        } else {
                            self.e(format!("{} xmm0, xmm1", uc));
                        }
                        let strict = op == ">" || op == "<";
                        let cc = match (strict, jump_if) {
                            (true, true) => "a",
                            (true, false) => "be",
                            (false, true) => "ae",
                            (false, false) => "b",
                        };
                        self.e(format!("j{} {}", cc, label));
                    }
                    _ => {
                        self.e(format!("{} xmm0, xmm1", uc));
                        // `==` is true when equal and ordered; `!=` is its opposite
                        let jump_when_equal = (op == "==") == jump_if;
                        if jump_when_equal {
                            // jump when equal and ordered
                            let skip = self.lbl("nan");
                            self.e(format!("jp {}", skip));
                            self.e(format!("je {}", label));
                            self.label(&skip);
                        } else {
                            // jump when not equal or unordered
                            self.e(format!("jp {}", label));
                            self.e(format!("jne {}", label));
                        }
                    }
                }
                return true;
            }
        }
        false
    }
}

/// Local cleanups of one function's code: a reload of a value just stored, a jump to the next
/// label, code that can never run, and a register moved to itself. Every rule looks at adjacent
/// lines only and never moves anything across a label.
fn peephole(code: &[String], ret_label: &str) -> Vec<String> {
    let mut cur: Vec<String> = code.to_vec();
    loop {
        let before = cur.len();
        let mut out: Vec<String> = Vec::with_capacity(cur.len());
        let mut dead = false; // after an unconditional jump or ret, up to the next label
        for line in &cur {
            let t = line.trim();
            let is_label = t.ends_with(':') && !line.starts_with(' ');
            if is_label {
                dead = false;
                // a jump to the label that follows it is no jump at all
                let mut i = out.len();
                while i > 0 && out[i - 1].ends_with(':') && !out[i - 1].starts_with(' ') {
                    i -= 1;
                }
                if i > 0 && out[i - 1].trim() == format!("jmp {}", &t[..t.len() - 1]) {
                    out.remove(i - 1);
                }
                out.push(line.clone());
                continue;
            }
            if dead {
                continue;
            }
            // `mov [X], r` then `mov r, [X]`: the register already holds the value
            if let Some(rest) = t.strip_prefix("mov ") {
                if let Some((dst, src)) = rest.split_once(", ") {
                    // only a 64-bit register: `mov eax, eax` clears the upper half
                    if dst == src && dst.starts_with('r') && !dst.contains('[') {
                        continue;
                    }
                    if let Some(prev) = out.last() {
                        if let Some(pr) = prev.trim().strip_prefix("mov ") {
                            if let Some((pdst, psrc)) = pr.split_once(", ") {
                                if pdst.starts_with("qword ptr [") && src == pdst && !psrc.contains('[') && psrc.starts_with('r') {
                                    if dst == psrc {
                                        continue;
                                    }
                                    // the value is still in the register that was stored
                                    if dst.starts_with('r') && !dst.contains('[') {
                                        out.push(format!("    mov {}, {}", dst, psrc));
                                        continue;
                                    }
                                }
                                // `mov r12, rax` then `mov rax, r12`: both already hold the value
                                let is_r = |x: &str| x.starts_with('r') && !x.contains(' ') && !x.contains('[');
                                if is_r(pdst) && is_r(psrc) && dst == psrc && src == pdst {
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
            let unconditional = t.starts_with("jmp ") || t == "ret";
            out.push(line.clone());
            if unconditional {
                dead = true;
            }
        }
        // a final jump to the epilogue that follows
        if out.last().map_or(false, |l| l.trim() == format!("jmp {}", ret_label)) {
            out.pop();
        }
        cur = out;
        if cur.len() == before {
            return cur;
        }
    }
}

fn norm_insn(t: &Ty) -> Option<&'static str> {
    Some(match t {
        Ty::Bool => "movzx eax, al",
        Ty::Int { bits: 8, signed: true } => "movsx rax, al",
        Ty::Int { bits: 8, signed: false } => "movzx eax, al",
        Ty::Int { bits: 16, signed: true } => "movsx rax, ax",
        Ty::Int { bits: 16, signed: false } => "movzx eax, ax",
        Ty::Int { bits: 32, signed: true } => "movsxd rax, eax",
        Ty::Int { bits: 32, signed: false } => "mov eax, eax",
        _ => return None,
    })
}

fn binop_static(op: &str) -> &'static str {
    match op {
        "+" => "+",
        "-" => "-",
        "*" => "*",
        "/" => "/",
        _ => "%",
    }
}

fn is_register(n: &str) -> bool {
    const R: [&str; 32] = [
        "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15", "eax", "ebx",
        "ecx", "edx", "esi", "edi", "r8d", "r9d", "r10d", "r11d", "ax", "bx", "cx", "dx", "al", "bl", "cl", "dl",
    ];
    R.contains(&n)
}

fn reg64(r: &str) -> String {
    match r {
        "eax" | "ax" | "al" => "rax".into(),
        "ebx" | "bx" | "bl" => "rbx".into(),
        "ecx" | "cx" | "cl" => "rcx".into(),
        "edx" | "dx" | "dl" => "rdx".into(),
        "esi" => "rsi".into(),
        "edi" => "rdi".into(),
        r if r.ends_with('d') && r.starts_with('r') => r[..r.len() - 1].to_string(),
        r => r.to_string(),
    }
}

// ---- defer: does a body contain a `defer` of its own (not inside a nested lambda)? ----

fn block_has_defer(b: &[Stmt]) -> bool {
    b.iter().any(stmt_has_defer)
}

fn stmt_has_defer(s: &Stmt) -> bool {
    match &s.kind {
        StmtKind::Defer(_) => true,
        StmtKind::Unsafe(b) => block_has_defer(b),
        StmtKind::For { body, .. } | StmtKind::While { body, .. } => block_has_defer(body),
        StmtKind::Expr(e) => expr_has_defer(e),
        StmtKind::Var(vd) => vd.value.as_ref().map_or(false, expr_has_defer),
        _ => false,
    }
}

fn expr_has_defer(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::If { branches, else_body } => {
            branches.iter().any(|(_, b)| block_has_defer(b)) || else_body.as_ref().map_or(false, |b| block_has_defer(b))
        }
        ExprKind::Match { arms, .. } => arms.iter().any(|a| block_has_defer(&a.body)),
        _ => false,
    }
}

// ---- free-variable collection for closures ----

/// True when the program mentions a float literal or a float type (the printing
/// library is only linked then). Searching the debug text over-approximates, which is harmless.
fn file_uses_floats(f: &File) -> bool {
    let d = format!("{:?}", f.decls);
    d.contains("Float(") || d.contains("\"float\"") || d.contains("\"f32\"") || d.contains("\"f64\"")
}

fn file_uses_concurrency(f: &File) -> bool {
    let mut names = Vec::new();
    for d in &f.decls {
        if let DeclKind::Func(fd) = &d.kind {
            if fd.is_async {
                names.push("$await".to_string());
            }
            collect_block(&fd.body, &mut names);
        }
    }
    names.iter().any(|n| n == "$spawn" || n == "$await" || n == "channel" || n == "sleep" || n == "timeout")
}

fn collect_block(b: &[Stmt], out: &mut Vec<String>) {
    for s in b {
        match &s.kind {
            StmtKind::Var(vd) => {
                if let Some(e) = &vd.value {
                    collect_expr(e, out);
                }
            }
            StmtKind::Assign { target, value, .. } => {
                collect_expr(target, out);
                collect_expr(value, out);
            }
            StmtKind::Spawn(e) => {
                out.push("$spawn".into());
                collect_expr(e, out);
            }
            StmtKind::Return(Some(e)) | StmtKind::Defer(e) | StmtKind::Expr(e) => collect_expr(e, out),
            StmtKind::Unsafe(b) => collect_block(b, out),
            StmtKind::For { iter, body, .. } => {
                collect_expr(iter, out);
                collect_block(body, out);
            }
            StmtKind::While { cond, body } => {
                collect_expr(cond, out);
                collect_block(body, out);
            }
            StmtKind::Asm(a) => {
                for o in &a.operands {
                    collect_expr(&o.value, out);
                }
            }
            _ => {}
        }
    }
}

fn collect_expr(e: &Expr, out: &mut Vec<String>) {
    match &e.kind {
        ExprKind::Ident(n) => out.push(n.clone()),
        ExprKind::Text(parts) => {
            for p in parts {
                if let TextPart::Expr(x) = p {
                    collect_expr(x, out);
                }
            }
        }
        ExprKind::List(xs) => xs.iter().for_each(|x| collect_expr(x, out)),
        ExprKind::Map(kv) => kv.iter().for_each(|(k, v)| {
            collect_expr(k, out);
            collect_expr(v, out);
        }),
        ExprKind::Record { fields, .. } => fields.iter().for_each(|(_, v)| collect_expr(v, out)),
        ExprKind::Lambda { body, .. } => collect_block(body, out),
        ExprKind::If { branches, else_body } => {
            for (c, b) in branches {
                collect_expr(c, out);
                collect_block(b, out);
            }
            if let Some(b) = else_body {
                collect_block(b, out);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_expr(scrutinee, out);
            for a in arms {
                collect_block(&a.body, out);
            }
        }
        ExprKind::Await(expr) => {
            out.push("$await".into());
            collect_expr(expr, out)
        }
        ExprKind::Unary { expr, .. } | ExprKind::Deref(expr) | ExprKind::Propagate(expr) => collect_expr(expr, out),
        ExprKind::Field { expr, .. } => collect_expr(expr, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_expr(lhs, out);
            collect_expr(rhs, out);
        }
        ExprKind::Index { expr, index } => {
            collect_expr(expr, out);
            collect_expr(index, out);
        }
        ExprKind::Call { callee, args } => {
            collect_expr(callee, out);
            args.iter().for_each(|a| collect_expr(&a.value, out));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Runtime (hand-written assembly, linked into every program)
// ---------------------------------------------------------------------------

const RUNTIME_COMMON: &str = r#"
; ---- Brasa runtime ----

; +1 reference on the object at rcx (null is fine). Preserves all registers.
rt_retain proc
    test rcx, rcx
    jz rt_retain_done
    inc qword ptr [rcx - 16]
rt_retain_done:
    ret
rt_retain endp

; -1 reference on the object at rcx; frees it (and its contents) at zero.
; Preserves rax. Clobbers rcx, rdx, r8-r11.
rt_release proc
    test rcx, rcx
    jz rt_rel_ret
    dec qword ptr [rcx - 16]
    jnz rt_rel_ret
    push rbp
    mov rbp, rsp
    push rax
    push rcx
    sub rsp, 32
    mov rdx, qword ptr [rcx - 8]
    test rdx, rdx
    jz rt_rel_free
    call rdx
rt_rel_free:
    mov rcx, qword ptr [rbp - 16]
    call rt_free_obj
    mov rax, qword ptr [rbp - 8]
    mov rsp, rbp
    pop rbp
rt_rel_ret:
    ret
rt_release endp

; drop procedures (called by rt_release with the object in rcx)
rt_drop_closure proc
    mov rcx, qword ptr [rcx + 8]
    jmp rt_release
rt_drop_closure endp

rt_drop_error proc
    mov rcx, qword ptr [rcx]
    jmp rt_release
rt_drop_error endp

rt_drop_result_plain proc
    mov rcx, qword ptr [rcx]
    jmp rt_release
rt_drop_result_plain endp

rt_drop_result_rc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rcx, qword ptr [rcx]
    call rt_release
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 8]
    call rt_release
    mov rsp, rbp
    pop rbp
    ret
rt_drop_result_rc endp

; copy r8 bytes from rdx to rcx
rt_memcpy proc
    push rsi
    push rdi
    mov rdi, rcx
    mov rsi, rdx
    mov rcx, r8
    rep movsb
    pop rdi
    pop rsi
    ret
rt_memcpy endp

; print "panic: <text rcx>" and exit with code 101
rt_panic proc
    sub rsp, 40
    mov qword ptr [rsp + 32], rcx
    lea rcx, rt_msg_panic
    call rt_write_out
    mov rcx, qword ptr [rsp + 32]
    call rt_write_out
    lea rcx, rt_msg_nl
    call rt_write_out
    mov ecx, 101
    call rt_exit
rt_panic endp

; rax = new text with room for rcx bytes (zeroed, length set)
rt_text_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    add rcx, 9
    xor edx, edx
    call rt_alloc
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rax], rcx
    mov rsp, rbp
    pop rbp
    ret
rt_text_alloc endp

; rax = new text: text rcx + text rdx (operands are not consumed)
rt_concat proc
    push rbp
    mov rbp, rsp
    push rsi
    push rdi
    sub rsp, 48
    mov qword ptr [rbp - 24], rcx
    mov qword ptr [rbp - 32], rdx
    mov rcx, qword ptr [rcx]
    add rcx, qword ptr [rdx]
    call rt_text_alloc
    lea rdi, [rax + 8]
    mov rsi, qword ptr [rbp - 24]
    mov rcx, qword ptr [rsi]
    add rsi, 8
    rep movsb
    mov rsi, qword ptr [rbp - 32]
    mov rcx, qword ptr [rsi]
    add rsi, 8
    rep movsb
    add rsp, 48
    pop rdi
    pop rsi
    pop rbp
    ret
rt_concat endp

; rax = 1 if texts rcx and rdx are equal
rt_text_eq proc
    push rsi
    push rdi
    cmp rcx, rdx
    je rt_teq_yes
    test rcx, rcx
    jz rt_teq_no
    test rdx, rdx
    jz rt_teq_no
    mov rax, qword ptr [rcx]
    cmp rax, qword ptr [rdx]
    jne rt_teq_no
    lea rsi, [rcx + 8]
    lea rdi, [rdx + 8]
    mov rcx, rax
    repe cmpsb
    jne rt_teq_no
rt_teq_yes:
    mov eax, 1
    jmp rt_teq_done
rt_teq_no:
    xor eax, eax
rt_teq_done:
    pop rdi
    pop rsi
    ret
rt_text_eq endp

; rax = byte rdx of text rcx
rt_text_byte proc
    cmp rdx, qword ptr [rcx]
    jae rt_index_oob
    movzx eax, byte ptr [rcx + rdx + 8]
    ret
rt_text_byte endp

; rax = new text: bytes [rdx, r8) of text rcx (indexes are clamped)
rt_text_slice proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov rax, qword ptr [rcx]
    cmp r8, rax
    jle rt_ts_e1
    mov r8, rax
rt_ts_e1:
    test r8, r8
    jns rt_ts_e2
    xor r8d, r8d
rt_ts_e2:
    test rdx, rdx
    jns rt_ts_s1
    xor edx, edx
rt_ts_s1:
    cmp rdx, r8
    jle rt_ts_s2
    mov rdx, r8
rt_ts_s2:
    mov qword ptr [rbp - 16], rdx
    sub r8, rdx
    mov qword ptr [rbp - 24], r8
    mov rcx, r8
    call rt_text_alloc
    mov qword ptr [rbp - 32], rax
    lea rcx, [rax + 8]
    mov rdx, qword ptr [rbp - 8]
    add rdx, 8
    add rdx, qword ptr [rbp - 16]
    mov r8, qword ptr [rbp - 24]
    call rt_memcpy
    mov rax, qword ptr [rbp - 32]
    mov rsp, rbp
    pop rbp
    ret
rt_text_slice endp

; rax = one-byte text for the byte in rcx
rt_text_chr proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov ecx, 1
    call rt_text_alloc
    mov rcx, qword ptr [rbp - 8]
    mov byte ptr [rax + 8], cl
    mov rsp, rbp
    pop rbp
    ret
rt_text_chr endp

; rax = new text holding the rdx bytes at rcx
rt_text_from_bytes proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov rcx, rdx
    call rt_text_alloc
    mov qword ptr [rbp - 24], rax
    lea rcx, [rax + 8]
    mov rdx, qword ptr [rbp - 8]
    mov r8, qword ptr [rbp - 16]
    call rt_memcpy
    mov rax, qword ptr [rbp - 24]
    mov rsp, rbp
    pop rbp
    ret
rt_text_from_bytes endp

; rax = decimal text of signed integer rcx
rt_text_from_int proc
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov rax, rcx
    lea r8, [rbp - 1]
    xor r9d, r9d
    test rax, rax
    jns rt_tfi_pos
    neg rax
    mov r9d, 1
rt_tfi_pos:
    mov r10, 10
rt_tfi_loop:
    xor edx, edx
    div r10
    add dl, 48
    dec r8
    mov byte ptr [r8], dl
    test rax, rax
    jnz rt_tfi_loop
    test r9d, r9d
    jz rt_tfi_copy
    dec r8
    mov byte ptr [r8], 45
rt_tfi_copy:
    lea rcx, [rbp - 1]
    sub rcx, r8
    mov qword ptr [rbp - 40], r8
    mov qword ptr [rbp - 48], rcx
    call rt_text_alloc
    mov rcx, qword ptr [rbp - 48]
    mov r8, qword ptr [rbp - 40]
    lea r9, [rax + 8]
rt_tfi_cp:
    test rcx, rcx
    jz rt_tfi_done
    mov dl, byte ptr [r8]
    mov byte ptr [r9], dl
    inc r8
    inc r9
    dec rcx
    jmp rt_tfi_cp
rt_tfi_done:
    mov rsp, rbp
    pop rbp
    ret
rt_text_from_int endp

; rax = text copied from NUL-terminated bytes at rcx
rt_text_from_cstr proc
    push rbp
    mov rbp, rsp
    push rsi
    push rdi
    sub rsp, 32
    mov rsi, rcx
    xor ecx, ecx
    test rsi, rsi
    jz rt_tfc_alloc
rt_tfc_len:
    cmp byte ptr [rsi + rcx], 0
    je rt_tfc_alloc
    inc rcx
    jmp rt_tfc_len
rt_tfc_alloc:
    mov rdi, rcx
    call rt_text_alloc
    mov rcx, rdi
    lea rdi, [rax + 8]
    rep movsb
    add rsp, 32
    pop rdi
    pop rsi
    pop rbp
    ret
rt_text_from_cstr endp

; rax = new empty list; rcx = 1 when its elements are heap objects
rt_list_new proc
    sub rsp, 40
    mov qword ptr [rsp + 32], rcx
    mov ecx, 32
    lea rdx, rt_drop_list_plain
    cmp qword ptr [rsp + 32], 0
    je rt_ln_alloc
    lea rdx, rt_drop_list_rc
rt_ln_alloc:
    call rt_alloc
    mov rcx, qword ptr [rsp + 32]
    mov qword ptr [rax + 24], rcx
    add rsp, 40
    ret
rt_list_new endp

rt_drop_list_plain proc
    mov rcx, qword ptr [rcx + 16]
    jmp rt_release
rt_drop_list_plain endp

rt_drop_list_rc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], 0
rt_dlr_loop:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 16]
    cmp rax, qword ptr [rcx]
    jae rt_dlr_done
    mov rdx, qword ptr [rcx + 16]
    mov rcx, qword ptr [rdx + rax * 8]
    call rt_release
    inc qword ptr [rbp - 16]
    jmp rt_dlr_loop
rt_dlr_done:
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    call rt_release
    mov rsp, rbp
    pop rbp
    ret
rt_drop_list_rc endp

; append rdx (an owned value) to list rcx
rt_list_push proc
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov rax, qword ptr [rcx]
    cmp rax, qword ptr [rcx + 8]
    jb rt_lp_store
    mov rax, qword ptr [rcx + 8]
    add rax, rax
    cmp rax, 4
    jae rt_lp_cap
    mov eax, 4
rt_lp_cap:
    mov qword ptr [rbp - 24], rax
    lea rcx, [rax * 8]
    xor edx, edx
    call rt_alloc
    mov qword ptr [rbp - 32], rax
    mov rcx, qword ptr [rbp - 8]
    mov r8, qword ptr [rcx + 16]
    mov qword ptr [rbp - 40], r8
    mov r9, qword ptr [rcx]
    xor r10d, r10d
rt_lp_cp:
    cmp r10, r9
    jae rt_lp_cpdone
    mov r11, qword ptr [r8 + r10 * 8]
    mov qword ptr [rax + r10 * 8], r11
    inc r10
    jmp rt_lp_cp
rt_lp_cpdone:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 32]
    mov qword ptr [rcx + 16], rax
    mov rdx, qword ptr [rbp - 24]
    mov qword ptr [rcx + 8], rdx
    mov rcx, qword ptr [rbp - 40]
    call rt_release
rt_lp_store:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rcx]
    mov r8, qword ptr [rcx + 16]
    mov rdx, qword ptr [rbp - 16]
    mov qword ptr [r8 + rax * 8], rdx
    inc qword ptr [rcx]
    mov rsp, rbp
    pop rbp
    ret
rt_list_push endp

; rax = element rdx of list rcx (borrowed)
rt_list_get proc
    cmp rdx, qword ptr [rcx]
    jae rt_index_oob
    mov rax, qword ptr [rcx + 16]
    mov rax, qword ptr [rax + rdx * 8]
    ret
rt_list_get endp

; list rcx [rdx] = r8 (owned); releases the previous element
rt_list_set proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    cmp rdx, qword ptr [rcx]
    jae rt_index_oob
    mov rax, qword ptr [rcx + 16]
    lea rax, [rax + rdx * 8]
    mov qword ptr [rbp - 8], rax
    mov qword ptr [rbp - 16], r8
    cmp qword ptr [rcx + 24], 0
    je rt_ls_store
    mov rcx, qword ptr [rax]
    call rt_release
rt_ls_store:
    mov rax, qword ptr [rbp - 8]
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rax], rcx
    mov rsp, rbp
    pop rbp
    ret
rt_list_set endp

; rax = the last element of list rcx, removed WITHOUT releasing it (ownership moves to the caller)
rt_list_take proc
    mov rax, qword ptr [rcx]
    test rax, rax
    jz rt_take_empty
    dec rax
    mov qword ptr [rcx], rax
    mov rdx, qword ptr [rcx + 16]
    mov rax, qword ptr [rdx + rax * 8]
    ret
rt_take_empty:
    lea rcx, rt_msg_take
    jmp rt_panic
rt_list_take endp

; empties list rcx (releasing its elements)
rt_list_clear proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], 0
    cmp qword ptr [rcx + 24], 0
    je rt_lc_done
rt_lc_loop:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 16]
    cmp rax, qword ptr [rcx]
    jae rt_lc_done
    mov rdx, qword ptr [rcx + 16]
    mov rcx, qword ptr [rdx + rax * 8]
    call rt_release
    inc qword ptr [rbp - 16]
    jmp rt_lc_loop
rt_lc_done:
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rcx], 0
    mov rsp, rbp
    pop rbp
    ret
rt_list_clear endp

rt_index_oob proc
    lea rcx, rt_msg_oob
    jmp rt_panic
rt_index_oob endp
"#;

const RUNTIME_MAP: &str = include_str!("runtime_map.asm");

const RUNTIME_WIN: &str = r#"
; rax = zeroed heap object of rcx bytes; rdx = drop procedure (or 0).
; Objects have a 16-byte header: [refcount][drop].
rt_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov rax, qword ptr rt_heap
    test rax, rax
    jnz rt_alloc_have
    call GetProcessHeap
    mov qword ptr rt_heap, rax
rt_alloc_have:
    mov rcx, rax
    mov edx, 8
    mov r8, qword ptr [rbp - 8]
    add r8, 16
    call HeapAlloc
    test rax, rax
    jz rt_alloc_fail
    mov qword ptr [rax], 1
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rax + 8], rcx
    add rax, 16
    inc qword ptr rt_live
    mov rsp, rbp
    pop rbp
    ret
rt_alloc_fail:
    lea rcx, rt_msg_oom
    call rt_panic
rt_alloc endp

; write rdx bytes at rcx to stdout
rt_write proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov ecx, -11
    call GetStdHandle
    mov rcx, rax
    mov rdx, qword ptr [rbp - 8]
    mov r8, qword ptr [rbp - 16]
    lea r9, [rbp - 24]
    mov qword ptr [rsp + 32], 0
    call WriteFile
    mov rsp, rbp
    pop rbp
    ret
rt_write endp

; write text rcx to stdout; the text is released afterwards when rdx = 1
rt_write_out proc
    sub rsp, 40
    mov rdx, qword ptr [rcx]
    add rcx, 8
    call rt_write
    add rsp, 40
    ret
rt_write_out endp


; frees the object whose payload is at rcx
rt_free_obj proc
    sub rsp, 40
    mov r8, rcx
    sub r8, 16
    mov rcx, qword ptr rt_heap
    xor edx, edx
    call HeapFree
    dec qword ptr rt_live
    add rsp, 40
    ret
rt_free_obj endp

; exit the process with code ecx
rt_exit proc
    sub rsp, 40
    call ExitProcess
rt_exit endp
"#;

const RUNTIME_BARE: &str = r#"
; ---- bare-metal runtime: a size-class heap and a console hook ----

; rax = zeroed heap object of rcx bytes; rdx = drop procedure (or 0).
; Block layout: [total size][refcount][drop][payload...]; freed blocks of
; less than 4096 bytes go on a free list per 16-byte size class.
rt_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    lea rax, [rcx + 39]
    and rax, -16
    mov qword ptr [rbp - 8], rax
    mov qword ptr [rbp - 16], rdx
    cmp rax, 4096
    jae rt_alloc_bump
    mov rcx, rax
    shr rcx, 4
    lea r8, rt_freelists
    mov r9, qword ptr [r8 + rcx * 8]
    test r9, r9
    jz rt_alloc_bump
    mov r10, qword ptr [r9 + 8]
    mov qword ptr [r8 + rcx * 8], r10
    mov rax, r9
    jmp rt_alloc_init
rt_alloc_bump:
    mov rax, qword ptr rt_brk
    mov rcx, qword ptr [rbp - 8]
    lea rdx, [rax + rcx]
    cmp rdx, 67108864
    ja rt_alloc_fail
    mov qword ptr rt_brk, rdx
rt_alloc_init:
    mov rcx, qword ptr [rbp - 8]
    lea r9, [rax + 8]
    lea r10, [rax + rcx]
rt_alloc_zero:
    mov qword ptr [r9], 0
    add r9, 8
    cmp r9, r10
    jb rt_alloc_zero
    mov qword ptr [rax], rcx
    mov qword ptr [rax + 8], 1
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rax + 16], rcx
    add rax, 24
    inc qword ptr rt_live
    mov rsp, rbp
    pop rbp
    ret
rt_alloc_fail:
    lea rcx, rt_msg_oom
    call rt_panic
rt_alloc endp

; frees the object whose payload is at rcx
rt_free_obj proc
    sub rcx, 24
    dec qword ptr rt_live
    mov rax, qword ptr [rcx]
    cmp rax, 4096
    jae rt_free_done
    shr rax, 4
    lea r8, rt_freelists
    mov r9, qword ptr [r8 + rax * 8]
    mov qword ptr [rcx + 8], r9
    mov qword ptr [r8 + rax * 8], rcx
rt_free_done:
    ret
rt_free_obj endp

; text rcx goes to the kernel's `console_write`, which owns its argument
rt_write_out proc
    sub rsp, 40
    mov qword ptr [rsp + 32], rcx
    call rt_retain
    mov rcx, qword ptr [rsp + 32]
    call B_CONSOLE_WRITE
    add rsp, 40
    ret
rt_write_out endp

; stop the machine
rt_exit proc
    cli
rt_exit_loop:
    hlt
    jmp rt_exit_loop
rt_exit endp
"#;

const RUNTIME_DATA_WIN: &str = "rt_heap dq 0
";

const RUNTIME_DATA_BARE: &str = "rt_brk dq 2097152
rt_freelists:
    times 256 dq 0
";

const RUNTIME_DATA_COMMON: &str = r#"rt_live dq 0
dq 4611686018427387904, 0
rt_msg_oom dq 13
    db "out of memory", 0
dq 4611686018427387904, 0
rt_msg_oob dq 19
    db "index out of bounds", 0
dq 4611686018427387904, 0
rt_msg_take dq 23
    db "take from an empty list", 0
dq 4611686018427387904, 0
rt_msg_nokey dq 17
    db "map key not found", 0
dq 4611686018427387904, 0
rt_msg_floatrange dq 18
    db "float out of range", 0
dq 4611686018427387904, 0
rt_msg_panic dq 7
    db "panic: ", 0
dq 4611686018427387904, 0
rt_msg_error dq 7
    db "error: ", 0
dq 4611686018427387904, 0
rt_msg_nl dq 1
    db 10, 0
"#;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Where the kernel is loaded (and linked to run) by the boot sector.
pub const KERNEL_BASE: u64 = 0x10000;
/// The boot sector lives where the BIOS puts it.
pub const BOOT_BASE: u64 = 0x7C00;
const MAX_KERNEL_SECTORS: usize = 1000;

fn collect_asm(b: &[Stmt], out: &mut Vec<String>) {
    for s in b {
        match &s.kind {
            StmtKind::Asm(a) => out.extend(a.lines.iter().cloned()),
            StmtKind::Unsafe(inner) => collect_asm(inner, out),
            _ => {}
        }
    }
}

/// The assembly written in the `#[naked]` functions of a boot file.
fn boot_source(file: &File) -> Result<String, String> {
    let mut lines = Vec::new();
    for d in &file.decls {
        if let DeclKind::Func(fd) = &d.kind {
            if d.attrs.iter().any(|a| a.name == "naked") {
                collect_asm(&fd.body, &mut lines);
            }
        }
    }
    if lines.is_empty() {
        return Err(format!("{}.br: the boot file needs a #[naked] function containing `asm` blocks", file.package));
    }
    Ok(lines.join("
"))
}

/// Builds a bootable disk image: boot sector + kernel.
///
/// The boot file is Brasa whose `#[naked]` functions hold the real-mode and
/// mode-switching code; the kernel is ordinary Brasa compiled for long mode
/// and linked at `KERNEL_BASE`. It must define `func console_write(s: text)`,
/// which receives everything `print` writes.
pub fn build_os(boot: &File, kernel: &File) -> Result<Vec<u8>, String> {
    let mut c = Compiler::new(Target::Bare);
    c.load(kernel)?;
    c.declare()?;
    let ok = matches!(
        c.funcs.get(&(kernel.package.clone(), "console_write".to_string())),
        Some(s) if s.params.len() == 1 && s.params[0].ty == Ty::Text && s.ret == Ty::Void
    );
    if !ok {
        return Err(format!(
            "{}.br: the kernel must define `func console_write(s: text)`; `print` sends its output there",
            kernel.package
        ));
    }
    let asm = c.generate()?;
    let k = crate::asm_x86::assemble(&asm, KERNEL_BASE, 64).map_err(|e| format!("kernel: {}", e))?;
    let mut kernel_bytes = k.bytes;
    while kernel_bytes.len() % 512 != 0 {
        kernel_bytes.push(0);
    }
    let sectors = kernel_bytes.len() / 512;
    if sectors > MAX_KERNEL_SECTORS {
        return Err(format!("the kernel is {} sectors; the boot sector loads at most {}", sectors, MAX_KERNEL_SECTORS));
    }

    let b = crate::asm_x86::assemble(&boot_source(boot)?, BOOT_BASE, 16).map_err(|e| format!("boot: {}", e))?;
    let mut boot_bytes = b.bytes;
    if boot_bytes.len() != 512 || boot_bytes[510..] != [0x55, 0xAA] {
        return Err(format!(
            "boot: the boot sector must be exactly 512 bytes ending in 0x55 0xAA (it is {} bytes)",
            boot_bytes.len()
        ));
    }
    let Some(&at) = b.symbols.get("kernel_sectors") else {
        return Err("boot: define `kernel_sectors dw 0`; the builder stores the kernel size there".into());
    };
    let off = (at - BOOT_BASE) as usize;
    boot_bytes[off..off + 2].copy_from_slice(&(sectors as u16).to_le_bytes());

    let mut image = boot_bytes;
    image.extend_from_slice(&kernel_bytes);
    Ok(image)
}

/// Compiles a parsed program (plus the std modules it uses) to MASM assembly.
pub fn generate_asm(file: &File) -> Result<String, String> {
    generate_asm_with(file, &CompileOptions::default())
}

/// `generate_asm` with explicit options.
pub fn generate_asm_with(file: &File, options: &CompileOptions) -> Result<String, String> {
    let mut c = Compiler::new(Target::Windows);
    c.opt = options.optimize;
    c.load(file)?;
    c.declare()?;
    c.generate()
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
</assembly>
"#;

/// Visual Studio installations that can run `vcvars64.bat`, newest first.
fn vcvars_candidates() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let vswhere = Path::new(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
    if vswhere.exists() {
        if let Ok(out) = Command::new(vswhere).args(["-all", "-products", "*", "-property", "installationPath"]).output() {
            for l in String::from_utf8_lossy(&out.stdout).lines() {
                if !l.trim().is_empty() {
                    roots.push(PathBuf::from(l.trim()));
                }
            }
        }
    }
    for base in [r"C:\Program Files\Microsoft Visual Studio", r"C:\Program Files (x86)\Microsoft Visual Studio"] {
        for ver in ["18", "2022", "2019"] {
            for ed in ["Community", "Professional", "Enterprise", "BuildTools"] {
                roots.push(Path::new(base).join(ver).join(ed));
            }
        }
    }
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .map(|r| r.join(r"VC\Auxiliary\Build\vcvars64.bat"))
        .filter(|p| p.exists() && seen.insert(p.clone()))
        .collect()
}

/// Compiles to a native Windows x86-64 executable using `ml64` and `link`.
pub fn compile_to_native(file: &File, output_exe: &str) -> Result<(), String> {
    compile_to_native_with(file, output_exe, &CompileOptions::default())
}

/// `compile_to_native` with explicit options.
pub fn compile_to_native_with(file: &File, output_exe: &str, options: &CompileOptions) -> Result<(), String> {
    let mut c = Compiler::new(Target::Windows);
    c.opt = options.optimize;
    c.load(file)?;
    c.declare()?;
    let asm = c.generate()?;
    let gui = c.uses_ui;
    let mut libs: Vec<String> = vec!["kernel32".into()];
    libs.extend(c.libs.iter().filter(|l| *l != "kernel32").cloned());

    let out_abs = std::env::current_dir().map_err(|e| e.to_string())?.join(output_exe);
    // unique per build: several builds may run at once in one process
    static BUILD_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let work = std::env::temp_dir().join(format!(
        "brasa-build-{}-{}",
        std::process::id(),
        BUILD_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&work).map_err(|e| format!("cannot create {}: {}", work.display(), e))?;
    std::fs::write(work.join("prog.asm"), &asm).map_err(|e| e.to_string())?;
    std::fs::write(work.join("prog.manifest"), MANIFEST).map_err(|e| e.to_string())?;

    let candidates = vcvars_candidates();
    if candidates.is_empty() {
        return Err("could not find Visual Studio C++ build tools (vcvars64.bat); install the 'Desktop development with C++' workload".into());
    }
    let lib_args: Vec<String> = libs.iter().map(|l| format!("{}.lib", l)).collect();
    let mut last_err = String::new();
    for vcvars in candidates {
        let bat = format!(
            "@echo off\r\ncall \"{}\" >nul 2>&1\r\nml64 /nologo /c /Fo prog.obj prog.asm || exit /b 1\r\nlink /nologo /subsystem:{} /entry:brasa_start /manifest:embed /manifestinput:prog.manifest prog.obj {} /out:\"{}\" || exit /b 1\r\n",
            vcvars.display(),
            if gui { "windows" } else { "console" },
            lib_args.join(" "),
            out_abs.display()
        );
        let bat_path = work.join("build.bat");
        std::fs::write(&bat_path, bat).map_err(|e| e.to_string())?;
        let out = Command::new("cmd")
            .arg("/c")
            .arg(&bat_path)
            .current_dir(&work)
            .output()
            .map_err(|e| format!("failed to run the assembler/linker: {}", e))?;
        if out.status.success() {
            let _ = std::fs::remove_dir_all(&work);
            return Ok(());
        }
        last_err = format!(
            "assembler/linker failed (using {}):\n{}{}",
            vcvars.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Err(format!("{}\n(intermediate files kept in {})", last_err, work.display()))
}
