//! Native x86-64 back end for Brasa (Windows, Win64 ABI).
//!
//! Brasa source -> x86-64 assembly (MASM-style syntax) -> built-in assembler -> PE .exe (`pe.rs`) or ELF.
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
//!   semantics: a record read from a variable/field/list is copied (`B_copy_*`,
//!   a shallow copy: nested plain records are copied, lists, maps and text are
//!   retained) when it is stored somewhere else. `ref` parameters and receivers share it.
//! - `[]T`: `[len][cap][data*][elements_are_heap]`. A list (and a map) is shared by
//!   reference counting but has value semantics through copy on write: every write
//!   goes through the writer's own place (`write_place`), which runs `rt_list_unique`
//!   / `rt_map_unique` first: when the count is above 1 the list is copied one level
//!   into the place (plain-record elements with `B_copy_*`, others retained). A `ref`
//!   list or map parameter is passed as the address of the caller's slot.
//! - enums: a number when no variant has data, otherwise `[tag][fields...]`.
//! - `T or error`: a box `[error][value]`; `error` is `[message][kind][cause]` (the cause is an
//!   `error` or 0). An `ErrorKind` is an interned text `package.Name`: equal names, same object.
//! - `func(...)` values: `[code][env]` (a closure; captures by value).
//!   `extern "C" func(...)` values are raw code addresses (C callbacks).
//!
//! Every Brasa function uses the Win64 calling convention and only volatile
//! registers, so any top-level function can be handed to C as a callback.

use crate::ast::*;
use crate::parser::parse_source;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

type R<T> = Result<T, String>;

#[path = "derive_ops.rs"]
mod derive_ops;
use derive_ops::Cap;

/// The Linux layer of a library: `std/<name>.linux.br`, used instead of `std/<name>.br` when the
/// target is Linux. Libraries that touch the operating system have one.
pub fn std_linux_source(name: &str) -> Option<&'static str> {
    match name {
        "sys" => Some(include_str!("../std/sys.linux.br")),
        "task" => Some(include_str!("../std/task.linux.br")),
        "fs" => Some(include_str!("../std/fs.linux.br")),
        "net" => Some(include_str!("../std/net.linux.br")),
        "tls" => Some(include_str!("../std/tls.linux.br")),
        "ui" => Some(include_str!("../std/ui.linux.br")),
        _ => None,
    }
}

/// Libraries that are written on the Windows API and need a Linux layer to be used on Linux.
/// `ws` never gets one: it is internal to `net` on Windows.
const OS_LIBRARIES: &[&str] = &["task", "fs", "net", "sys", "tls", "ui", "thread", "sync"];

/// The source of library `name` for a target: the Linux layer on Linux when there is one.
pub fn std_source_for(name: &str, target: Target) -> Option<&'static str> {
    if target == Target::Linux {
        if let Some(src) = std_linux_source(name) {
            return Some(src);
        }
    }
    std_source(name)
}

/// Standard library modules written in Brasa and embedded in the compiler.
/// The standard library packages the compiler and the runtime rest on, or that most programs need:
/// their names are reserved and a package of a project cannot take them. Every other standard
/// library (`json`, `crypto`, `tls`, `ui`, `web`, `ai`, `mcp`, `rag`, `db`, `postgres`, `sqlserver`)
/// is on its way to being a package of its own and may be shadowed by one.
pub const CORE_STD: [&str; 19] = [
    "bufio", "derive", "errors", "float", "format", "fs", "future", "list", "math", "net", "reflect", "set", "sorted", "sync", "sys", "task", "text", "thread", "ws",
];

pub fn is_core_std(name: &str) -> bool {
    CORE_STD.contains(&name)
}

pub fn std_source(name: &str) -> Option<&'static str> {
    match name {
        "ui" => Some(include_str!("../std/ui.br")),
        "task" => Some(include_str!("../std/task.br")),
        "future" => Some(include_str!("../std/future.br")),
        "thread" => Some(include_str!("../std/thread.br")),
        "sync" => Some(include_str!("../std/sync.br")),
        "fs" => Some(include_str!("../std/fs.br")),
        "net" => Some(include_str!("../std/net.br")),
        "sys" => Some(include_str!("../std/sys.br")),
        "ws" => Some(include_str!("../std/ws.br")),
        "tls" => Some(include_str!("../std/tls.br")),
        "float" => Some(include_str!("../std/float.br")),
        "math" => Some(include_str!("../std/math.br")),
        "text" => Some(include_str!("../std/text.br")),
        "errors" => Some(include_str!("../std/errors.br")),
        "list" => Some(include_str!("../std/list.br")),
        "format" => Some(include_str!("../std/format.br")),
        "set" => Some(include_str!("../std/set.br")),
        "sorted" => Some(include_str!("../std/sorted.br")),
        "bufio" => Some(include_str!("../std/bufio.br")),
        "derive" => Some(include_str!("../std/derive.br")),
        "reflect" => Some(include_str!("../std/reflect.br")),
        "json" => Some(include_str!("../std/json.br")),
        "web" => Some(include_str!("../std/web.br")),
        "crypto" => Some(include_str!("../std/crypto.br")),
        "db" => Some(include_str!("../std/db.br")),
        "postgres" => Some(include_str!("../std/postgres.br")),
        "sqlserver" => Some(include_str!("../std/sqlserver.br")),
        "ai" => Some(include_str!("../std/ai.br")),
        "mcp" => Some(include_str!("../std/mcp.br")),
        "rag" => Some(include_str!("../std/rag.br")),
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
    /// `char`: a Unicode scalar value, held zero-extended in 32 bits.
    Char,
    Rec(String),
    Enum(String),
    List(Box<Ty>),
    /// `Map<K, V>`: `K` is `text` or an integer type.
    Map(Box<Ty>, Box<Ty>),
    Ptr(Box<Ty>),
    Func { params: Vec<Ty>, ret: Box<Ty>, ext: bool },
    Err,
    /// `ErrorKind`: an interned text `package.Name` (equal kinds are the same object).
    Kind,
    /// `T or error` (`T` is `Void` for `-> error`).
    Result(Box<Ty>),
    Chan(Box<Ty>),
    /// `Future<T>`: a handle to the result of an `async func` (`T` may be `Void`).
    Future(Box<Ty>),
    /// `Rc<T>`: a shared, reference-counted record created by `new T{ ... }` (`T` is a record).
    Rc(Box<Ty>),
    /// `Weak<T>`: a counted handle to the weak cell of an `Rc<T>`; it does not keep the object alive.
    Weak(Box<Ty>),
    /// A type parameter while a generic body is checked once: nothing is known about it but its bound
    /// (the qualified name of an interface).
    Param { name: String, bound: Option<String> },
}

impl Ty {
    fn i64() -> Ty {
        Ty::Int { bits: 64, signed: true }
    }
    /// `int`: the everyday name of `i32`; the default type of an integer literal and of lengths and indices.
    fn int() -> Ty {
        Ty::Int { bits: 32, signed: true }
    }
    fn is_int(&self) -> bool {
        matches!(self, Ty::Int { .. })
    }
    fn is_float(&self) -> bool {
        matches!(self, Ty::Float { .. })
    }
    fn is_unsigned(&self) -> bool {
        matches!(self, Ty::Int { signed: false, .. } | Ty::Char | Ty::Ptr(_))
    }
    /// Size in memory (record fields, pointer targets).
    fn size(&self) -> u32 {
        match self {
            Ty::Bool => 1,
            Ty::Char => 4,
            Ty::Int { bits, .. } => *bits as u32 / 8,
            Ty::Float { bits } => *bits as u32 / 8,
            _ => 8,
        }
    }
    fn nullable(&self) -> bool {
        matches!(self, Ty::Nil | Ty::Text | Ty::Err | Ty::Rec(_) | Ty::List(_) | Ty::Map(..) | Ty::Ptr(_) | Ty::Func { .. } | Ty::Chan(_) | Ty::Param { .. } | Ty::Rc(_) | Ty::Weak(_))
    }
    fn show(&self) -> String {
        match self {
            Ty::Void => "nothing".into(),
            Ty::Nil => "nil".into(),
            Ty::Bool => "bool".into(),
            Ty::Int { bits: 32, signed: true } => "int".into(),
            Ty::Int { bits: 64, signed: true } => "long".into(),
            Ty::Int { bits, signed } => format!("{}{}", if *signed { "i" } else { "u" }, bits),
            Ty::Float { bits: 32 } => "f32".into(),
            Ty::Float { .. } => "float".into(),
            Ty::Text => "text".into(),
            Ty::Char => "char".into(),
            Ty::Rec(n) | Ty::Enum(n) => display_name(n),
            Ty::Param { name, .. } => name.clone(),
            Ty::List(t) => format!("[]{}", t.show()),
            Ty::Map(k, v) => format!("Map<{}, {}>", k.show(), v.show()),
            Ty::Ptr(t) => format!("*{}", t.show()),
            Ty::Err => "error".into(),
            Ty::Kind => "ErrorKind".into(),
            Ty::Result(t) => {
                if **t == Ty::Void {
                    "error".into()
                } else {
                    format!("{} or error", t.show())
                }
            }
            Ty::Chan(t) => format!("Channel<{}>", t.show()),
            Ty::Rc(t) => format!("Rc<{}>", t.show()),
            Ty::Weak(t) => format!("Weak<{}>", t.show()),
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
/// Does every value of integer type `from` fit `to`? (the implicit, lossless widening)
fn widens(from: &Ty, to: &Ty) -> bool {
    match (from, to) {
        (Ty::Int { bits: fb, signed: fs }, Ty::Int { bits: tb, signed: ts }) => {
            if fs == ts {
                fb <= tb
            } else {
                // unsigned into a strictly wider signed type
                !*fs && *ts && fb < tb
            }
        }
        _ => false,
    }
}

/// The wider of two integer operand types, when one converts to the other.
fn wider(a: &Ty, b: &Ty) -> Option<Ty> {
    if widens(a, b) {
        Some(b.clone())
    } else if widens(b, a) {
        Some(a.clone())
    } else {
        None
    }
}

/// Is a comparison of `a` with `b` unsigned? Two integers compare as their wider type.
fn cmp_unsigned(a: &Ty, b: &Ty) -> bool {
    match wider(a, b) {
        Some(w) if a.is_int() && b.is_int() => w.is_unsigned(),
        _ => a.is_unsigned() || b.is_unsigned(),
    }
}

fn compatible(want: &Ty, got: &Ty) -> bool {
    match (want, got) {
        (a, b) if a == b => true,
        (Ty::Int { .. }, Ty::Int { .. }) => widens(got, want),
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
            // function values need the exact integer types (no widening of a signature)
            let exact = |a: &Ty, b: &Ty| !(a.is_int() && b.is_int()) || a == b;
            e1 == e2 && p1.len() == p2.len() && compatible(r1, r2) && exact(r1, r2) && p1.iter().zip(p2).all(|(a, b)| exact(a, b))
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
    /// written with `pub`: other packages may use it
    public: bool,
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
    public: bool,
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
    /// A `ref` parameter, receiver or lambda parameter: a borrow that must not escape.
    is_ref: bool,
    /// A `ref` parameter of list or map type: the slot holds the address of the caller's slot, so a
    /// write (after `unique`) updates the caller's own variable.
    ind: bool,
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
    /// Names of locals that hold a `shared` value: their content is immutable.
    shared_locals: HashSet<String>,
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
            defer_list: None,
            defer_save: None,
            allow_ignored_result: false,
            has_asm: false,
            may_own: false,
            regs: HashMap::new(),
            used_regs: Vec::new(),
            shared_locals: HashSet::new(),
        }
    }
}

const ARG_REGS: [&str; 4] = ["rcx", "rdx", "r8", "r9"];

/// Largest body (in expression nodes) and deepest nesting of calls compiled in place.
const INLINE_NODES: usize = 12;
const INLINE_DEPTH: usize = 4;
const INLINE_PARAMS: usize = 4;

const TEXT_INDEX_MSG: &str = "text cannot be indexed: use s.bytes() for the bytes, s.slice(a, b) for a piece (byte positions), or for c in s for the characters";

fn plain_scalar(t: &Ty) -> bool {
    matches!(t, Ty::Int { .. } | Ty::Char | Ty::Bool | Ty::Float { .. } | Ty::Ptr(_))
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
            ExprKind::Call { callee, args, .. } => {
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

fn is_int_literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) => true,
        ExprKind::Unary { op: "-", expr } => matches!(expr.kind, ExprKind::Int(_)),
        _ => false,
    }
}

/// A number literal, or arithmetic on number literals only (it takes the type its context wants).
fn is_const_int_expr(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) => true,
        ExprKind::Unary { op: "-", expr } => is_const_int_expr(expr),
        ExprKind::Binary { op, lhs, rhs } if matches!(*op, "+" | "-" | "*" | "/" | "%" | "bitand" | "bitor" | "bitxor" | "<<" | ">>") => is_const_int_expr(lhs) && is_const_int_expr(rhs),
        _ => false,
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
        "int" => (32, true),
        "i64" | "isize" | "long" => (64, true),
        "u8" | "byte" => (8, false),
        "u16" => (16, false),
        "u32" => (32, false),
        "u64" | "usize" => (64, false),
        _ => return None,
    };
    Some(Ty::Int { bits, signed })
}

/// Does the literal `n` (negated when `neg`) fit integer type `t`?
fn lit_fits(n: u64, neg: bool, t: &Ty) -> bool {
    let Ty::Int { bits, signed } = t else { return true };
    let v = n as i128;
    let v = if neg { -v } else { v };
    let (lo, hi) = if *signed { (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1) } else { (0, (1i128 << bits) - 1) };
    lo <= v && v <= hi
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

/// The record behind an `Rc<T>` (a pointer to the same layout), or the type itself.
fn rc_view(t: &Ty) -> Ty {
    match t {
        Ty::Rc(inner) if matches!(**inner, Ty::Rec(_)) => (**inner).clone(),
        other => other.clone(),
    }
}

/// Key under which the methods of a receiver type are stored.
fn method_key(t: &Ty) -> String {
    match t {
        Ty::Rec(r) => r.clone(),
        other => ty_full(other),
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
    /// A static Linux x86-64 executable (system calls, no C library).
    Linux,
}

/// The parts of the program that survive pruning, as assembly text.
struct Pruned {
    procs: Vec<String>,
    thunks: Vec<String>,
    globals: Vec<String>,
    closures: Vec<String>,
    statics: Vec<String>,
    data: Vec<String>,
}

/// Options of a compilation.
#[derive(Clone, Copy, Debug)]
pub struct CompileOptions {
    /// Constant folding, direct operands, fused compare-and-branch, peephole cleanups and the
    /// cheaper function entry. `--no-opt` turns it off.
    pub optimize: bool,
    /// Set by `--test` when it builds the test entry point: a failing `assert` in a function
    /// that returns `error` then returns the error (one test fails, the run goes on). It is not
    /// a command-line flag; a normal build aborts instead.
    pub test_mode: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions { optimize: true, test_mode: false }
    }
}

pub struct Compiler {
    target: Target,
    /// All optimizations are on (see `CompileOptions`).
    opt: bool,
    test_mode: bool,
    /// Compiling the program that runs comptime generators: `Type` is `reflect.Type`.
    driver: bool,
    modules: Vec<Module>,
    imports: HashMap<String, HashSet<String>>,
    /// `(importing module, qualifier)` to the module key when a package is imported under a name
    /// that is not its key (two packages with the same name, or one package under two names).
    mod_aliases: HashMap<(String, String), String>,
    rec_decls: HashMap<String, (String, Vec<Field>)>,
    /// `#[shared]` records have reference semantics: never copied on assignment, capture or iteration.
    shared_recs: HashSet<String>,
    /// records that `new` allocates (they get a `B_rcdrop_*` wrapper); never rolled back
    rc_recs: HashSet<String>,
    enum_decls: HashMap<String, (String, Vec<Variant>)>,
    /// qualified names of the types written with `pub`
    pub_types: HashSet<String>,
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
    /// the library of each extern symbol, from the `#[link]` before it (Windows imports)
    extern_lib: BTreeMap<String, String>,
    /// Windows imports as `(dll, symbols)`, `kernel32` first; filled by `generate`
    win_imports: Vec<(String, Vec<String>)>,
    /// functions handed to C as callbacks (Linux: each gets a System V entry thunk)
    callbacks: BTreeMap<String, (Vec<u8>, u8)>,
    /// the divisor being compiled is a literal that is not zero
    div_nonzero: bool,
    data: Vec<String>,
    procs: Vec<String>,
    strings: HashMap<String, String>,
    fn_closures: BTreeSet<String>,
    labels: usize,
    /// the next `lambda()` is the body of an async function: `await` is allowed in it
    /// symbols of functions whose body contains a direct `await`
    await_syms: std::collections::HashSet<String>,
    /// functions marked `#[export]`
    exported: std::collections::HashSet<String>,
    /// the next `lambda()` shares the records it captures instead of copying them
    next_lambda_share: bool,
    /// the next `lambda()` may ignore the result of a call that can fail
    next_lambda_ignore_result: bool,
    cur_mod: String,
    line: u32,
    fs: FnState,
    uses_ui: bool,
    /// `use thread` was written; counts become atomic only if a function of it is reachable.
    uses_thread_lib: bool,
    /// The program uses `shared` or `spawn_thread`: counts and `rt_live` use `lock` instructions.
    threads: bool,
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
    custom_allocator: Option<crate::noalloc::CustomAllocatorInfo>,
    /// Methods the compiler wrote itself (derived `eq`, `compare`, `hash`): (type key, name).
    derived: HashSet<(String, String)>,
    /// Hash and equality thunks of general map keys, by key type: (hash symbol, equality symbol).
    map_thunks: BTreeMap<String, (String, String)>,
    /// Map key types met before every method was known: (key type, line, module).
    deferred_keys: Vec<(Ty, u32, String)>,
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
    derived: HashSet<(String, String)>,
    map_thunks: BTreeMap<String, (String, String)>,
}

impl Compiler {
    fn new(target: Target) -> Self {
        Compiler {
            target,
            opt: true,
            test_mode: false,
            driver: crate::comptime::driver_mode(),
            modules: Vec::new(),
            imports: HashMap::new(),
            mod_aliases: HashMap::new(),
            rec_decls: HashMap::new(),
            shared_recs: HashSet::new(),
            rc_recs: HashSet::new(),
            enum_decls: HashMap::new(),
            pub_types: HashSet::new(),
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
            extern_lib: BTreeMap::new(),
            win_imports: Vec::new(),
            callbacks: BTreeMap::new(),
            div_nonzero: false,
            data: Vec::new(),
            procs: Vec::new(),
            strings: HashMap::new(),
            fn_closures: BTreeSet::new(),
            labels: 0,
            await_syms: std::collections::HashSet::new(),
            exported: std::collections::HashSet::new(),
            next_lambda_share: false,
            next_lambda_ignore_result: false,
            cur_mod: String::new(),
            line: 0,
            fs: FnState::new(Ty::Void, String::new()),
            uses_ui: false,
            uses_thread_lib: false,
            threads: false,
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
            custom_allocator: None,
            derived: HashSet::new(),
            map_thunks: BTreeMap::new(),
            deferred_keys: Vec::new(),
        }
    }

    /// Rejects uses of the ref bindings `refs` that could outlive the call (spec `ref-parameters`).
    fn check_refs(&mut self, refs: &[(String, bool)], body: &[Stmt]) -> R<()> {
        match crate::refcheck::check_ref_escapes(refs, body) {
            Ok(()) => Ok(()),
            Err((name, line)) => {
                self.line = line;
                self.ref_escape(&name)
            }
        }
    }

    fn ref_escape<T>(&self, name: &str) -> R<T> {
        self.err(format!(
            "`ref` parameter '{}' cannot escape: it is only a temporary borrow and cannot be stored, returned, captured or sent to a task; use `new T` for a value that must live longer",
            name
        ))
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

    /// Loads the text `file.br:line` of the statement being compiled into `reg`, for a failing path.
    fn at(&mut self, reg: &str) {
        let l = self.string_lit(&format!("{}.br:{}", self.cur_mod, self.line));
        self.e(format!("lea {}, {}", reg, l));
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
            Ty::Text | Ty::Kind | Ty::Rec(_) | Ty::List(_) | Ty::Map(..) | Ty::Err | Ty::Result(_) | Ty::Chan(_) | Ty::Future(_) | Ty::Rc(_) | Ty::Weak(_) => true,
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
    fn map_type(&mut self, k: Ty, v: Ty) -> R<Ty> {
        if self.target == Target::Bare {
            return self.err("maps need the Windows runtime and cannot be used in a bare-metal kernel");
        }
        if !matches!(k, Ty::Param { .. }) {
            if self.declared {
                self.check_key(&k)?;
            } else {
                self.deferred_keys.push((k.clone(), self.line, self.cur_mod.clone()));
            }
        }
        Ok(Ty::Map(Box::new(k), Box::new(v)))
    }

    /// A key type must be `Hash`: the error names the type and the reason.
    fn check_key(&self, k: &Ty) -> R<()> {
        match self.lacks(k, Cap::Hash) {
            None => Ok(()),
            Some((leaf, why)) => {
                if leaf {
                    self.err(format!("{} cannot be a map key because it is not Hash ({})", k.show(), why))
                } else {
                    self.err(format!("{} cannot be a map key because {}", k.show(), why))
                }
            }
        }
    }

    /// Keys kept as plain numbers: integers, `bool` and enums without data.
    fn plain_key(&self, k: &Ty) -> bool {
        k.is_int() || matches!(k, Ty::Char | Ty::Bool | Ty::Param { .. }) || matches!(k, Ty::Enum(q) if !self.enum_payload.contains(q))
    }

    /// rax = new empty map of type `t`.
    fn new_map(&mut self, t: &Ty) -> R<()> {
        let Ty::Map(k, v) = t else { unreachable!() };
        let mut flags = (self.is_rc(v) as i32) << 1;
        let mut thunks = None;
        if **k == Ty::Text {
            flags |= 1;
        } else if !self.plain_key(k) {
            flags |= 12;
            thunks = Some(self.map_thunks_for(k)?);
        }
        self.e(format!("mov ecx, {}", flags));
        if let Some((h, e)) = thunks {
            self.e(format!("lea rdx, {}", h));
            self.e(format!("lea r8, {}", e));
        }
        self.e("call rt_map_new");
        Ok(())
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
        if !self.opt || !(ty.is_int() || matches!(ty, Ty::Char | Ty::Bool | Ty::Ptr(_))) {
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
            (None, _) if l.ind => {
                self.e(format!("mov {}, qword ptr [rbp - {}]", dst, l.off));
                self.e(format!("mov {d}, qword ptr [{d}]", d = dst));
            }
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
        sc.vars.insert(name.to_string(), Local { off, ty: ty.clone(), mutable, captured: false, reg, imm: None, is_ref: false, ind: false });
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

    /// `assert(cond)`, `assert(cond, "message")` and `assert_eq(a, b)`: under `--test`, in a
    /// function that returns an error, a failed check returns the error `line N: ...` (one test
    /// fails and the run goes on); anywhere else it aborts with the same text and the location
    /// of the assertion. Written as the statements a program could have written itself.
    fn assert_stmt(&mut self, name: &str, args: &[Arg], line: u32) -> R<()> {
        let returns = self.test_mode && matches!(self.fs.ret, Ty::Result(_));
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
                callee: Box::new(mk(ExprKind::Ident(if returns { "error" } else { "__abort" }.into()))),
                args: vec![Arg { name: None, by_ref: false, spread: false, value: mk(ExprKind::Text(parts)) }],
                targs: Vec::new(),
            });
            if returns {
                vec![Stmt { kind: StmtKind::Return(Some(call)), line }]
            } else {
                vec![Stmt { kind: StmtKind::Expr(call), line }]
            }
        };
        // the abort message carries the location already
        let prefix = TextPart::Lit(if returns { format!("line {}: ", line) } else { String::new() });
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
        // generators run, derives, generated functions and blocks are expanded (comptime-reflection)
        let expansion = if self.driver { None } else { Some(crate::comptime::expand(main, self.target)?) };
        let main = match &expansion {
            Some(x) => &x.main,
            None => main,
        };
        let mut queue = vec![(main.package.clone(), main.clone())];
        let mut seen = HashSet::new();
        seen.insert(main.package.clone());
        while let Some((name, file)) = queue.pop() {
            let mut imps = HashSet::new();
            let mut wanted: Vec<Vec<String>> = file.uses.clone();
            if file_uses_concurrency(&file) {
                if self.target == Target::Bare {
                    return Err("'spawn', 'channel', 'async' and 'await' need the task library, which needs Windows".into());
                }
                wanted.push(vec!["task".into()]);
                wanted.push(vec!["future".into()]);
            }
            let (uses_shared, uses_spawn) = file_uses_threads(&file);
            if uses_spawn {
                if self.target == Target::Bare {
                    return Err("'spawn_thread' needs operating-system threads: it is not available in a bare-metal kernel".into());
                }
                wanted.push(vec!["thread".into()]);
            }
            if (uses_shared || uses_spawn) && self.target != Target::Bare {
                self.threads = true;
            }
            if name != "float" && file_uses_floats(&file) {
                if self.target == Target::Bare {
                    return Err("floating point is not available in a bare-metal kernel".into());
                }
                wanted.push(vec!["float".into()]);
            }
            for segs in wanted {
                let mut std_name = crate::codegen_x64::std_source(&segs[0]).is_some();
                // a package of the project may take the name of a standard library that is not core
                // (spec `package-manager`); the standard libraries themselves keep their own
                let importer_is_std = name != main.package && crate::codegen_x64::std_source(&name).is_some();
                let mut shadowing: Option<(String, &'static str)> = None;
                if std_name && !is_core_std(&segs[0]) && !importer_is_std {
                    if let Some(r) = crate::package::resolve_use(&name, &segs)? {
                        if !self.driver {
                            crate::warn::push(format!("the package '{}' shadows the standard library's '{}'", segs.join("."), segs[0]));
                        }
                        shadowing = Some((r.key, r.source));
                        std_name = false;
                    } else if !self.driver {
                        crate::warn::push(format!("'{}' is moving out of the standard library into a package of its own (its source is packages/{} in the Brasa repository); it still works as `use {}`", segs[0], segs[0], segs[0]));
                    }
                }
                let m = if std_name { segs[0].clone() } else { segs.last().unwrap().clone() };
                if m == "ws" && name == main.package {
                    return Err("'ws' is internal to 'net' and cannot be used by programs; use 'net' instead".into());
                }
                if std_name && self.target == Target::Bare && (matches!(m.as_str(), "ui" | "task" | "future" | "fs" | "net" | "sys" | "ws" | "tls" | "thread" | "sync" | "web" | "crypto" | "db" | "postgres" | "sqlserver" | "ai" | "mcp" | "rag")) {
                    return Err(format!("'{}' needs Windows and cannot be used in a bare-metal kernel", m));
                }
                if std_name
                    && self.target == Target::Linux
                    && (m == "ws" || (OS_LIBRARIES.contains(&m.as_str()) && std_linux_source(&m).is_none()))
                {
                    return Err(format!("'{}' is not available on Linux yet (it uses the Windows API)", m));
                }
                // where the package is: the standard library, the project (brasa.toml and its
                // directories), then the files registered next to the source
                let found: Option<(String, &'static str)> = if shadowing.is_some() {
                    shadowing
                } else if std_name {
                    std_source_for(&m, self.target).map(|s| (m.clone(), s))
                } else {
                    match crate::package::resolve_use(&name, &segs)? {
                        Some(r) => Some((r.key, r.source)),
                        None => match crate::comptime::registered_source(&segs[0]) {
                            Some(s) if segs.len() == 1 => Some((segs[0].clone(), s)),
                            _ if segs[0] == "c" => None,
                            _ => {
                                return Err(format!(
                                    "unknown package '{}' (not in the standard library, brasa.toml or the project directory); to add one: brasa-parse --add <git-url>",
                                    segs.join(".")
                                ))
                            }
                        },
                    }
                };
                imps.insert(m.clone());
                if let Some((k, _)) = &found {
                    if *k != m {
                        self.mod_aliases.insert((name.clone(), m.clone()), k.clone());
                    }
                }
                if m == "thread" {
                    self.uses_thread_lib = true;
                }
                if m == "ui" {
                    self.uses_ui = true;
                }
                if let Some((key, src)) = found {
                    if !seen.contains(&key) {
                        seen.insert(key.clone());
                        let mut f = parse_source(src).map_err(|d| format!("{}{}.br:{}:{}: {}", if std_name { "std/" } else { "package " }, key, d.line, d.col, d.msg))?;
                        // generators run at build time only; generated templates have no body here
                        let driver = self.driver;
                        f.decls.retain(|d| match &d.kind {
                            DeclKind::Func(fd) => !(d.attrs.iter().any(|a| a.name == "generated") || (fd.comptime && !driver)),
                            _ => true,
                        });
                        if let Some(extra) = expansion.as_ref().and_then(|x| x.extra.get(&key)) {
                            f.decls.extend(extra.iter().cloned());
                        }
                        queue.push((key.clone(), f));
                    }
                }
            }
            self.imports.insert(name.clone(), imps);
            self.modules.push(Module { name, file });
        }
        Ok(())
    }

    /// The standard interfaces `Eq`, `Ord` and `Hash`, visible in every package without `use`.
    fn install_prelude(&mut self) -> R<()> {
        let src = "package std
type Eq = interface
    func eq(self, other: Self) -> bool
end
type Ord = interface
    func compare(self, other: Self) -> int
end
type Hash = interface
    func hash(self) -> long
end
";
        let f = parse_source(src).map_err(|d| format!("prelude: {}", d.msg))?;
        for d in f.decls {
            if let DeclKind::Type(td) = d.kind {
                if let TypeBody::Interface(sigs) = td.body {
                    self.interfaces.insert(format!("std.{}", td.name), ("std".to_string(), sigs));
                }
            }
        }
        Ok(())
    }

    fn declare(&mut self) -> R<()> {
        self.install_prelude()?;
        // Type names first, so records can refer to each other.
        let mods: Vec<(String, File)> = self.modules.iter().map(|m| (m.name.clone(), m.file.clone())).collect();
        for (mname, file) in &mods {
            self.cur_mod = mname.clone();
            for d in &file.decls {
                self.line = d.line;
                if let DeclKind::Type(td) = &d.kind {
                    let q = format!("{}.{}", mname, td.name);
                    if d.public {
                        self.pub_types.insert(q.clone());
                    }
                    if td.name == "Rc" || td.name == "Weak" {
                        return self.err(format!("type '{}' is defined twice: '{}' is a built-in type (shared references)", td.name, td.name));
                    }
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
            // a `#[link]` names the library of the externs that follow it, up to the next `#[link]`
            let mut cur_lib: Option<String> = None;
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
                            if a.name == "link_src" && self.target == Target::Windows {
                                self.line = d.line;
                                return self.err("#[link_src] is not supported on Windows: the compiler no longer builds C source files there (build the C code into a DLL and use #[link])");
                            }
                            if a.name == "link" {
                                if let Some(Expr { kind: ExprKind::Text(parts), .. }) = a.args.first() {
                                    if let Some(TextPart::Lit(l)) = parts.first() {
                                        self.libs.insert(l.clone());
                                        cur_lib = Some(l.clone());
                                    }
                                }
                            }
                        }
                        if let Some(l) = &cur_lib {
                            self.extern_lib.entry(ed.name.clone()).or_insert_with(|| l.clone());
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
                                public: d.public,
                            },
                        );
                    }
                    DeclKind::Var(vd) => {
                        let konst = if vd.mutable { None } else { vd.value.as_ref().and_then(|e| self.const_eval(e, m)) };
                        let ty = match (&vd.ty, &konst) {
                            (Some(t), _) => self.resolve(t, m)?,
                            (None, Some(k)) => {
                                let t = Ty::int();
                                if !lit_fits(k.unsigned_abs(), *k < 0, &t) {
                                    return self.err(format!("constant {} is out of range for int; write `let {}: long = ...`", k, vd.name));
                                }
                                t
                            }
                            (None, None) => self.infer_global(vd.value.as_ref(), m)?,
                        };
                        self.globals.insert(
                            (m.clone(), vd.name.clone()),
                            Global { sym: format!("G_{}_{}", m, vd.name), ty, konst, mutable: vd.mutable, public: d.public },
                        );
                    }
                    _ => {}
                }
            }
        }
        self.check_public_signatures()?;
        self.declared = true;
        let keys = std::mem::take(&mut self.deferred_keys);
        for (k, line, m) in keys {
            self.cur_mod = m;
            self.line = line;
            self.check_key(&k)?;
        }
        let mods: Vec<(String, File)> = self.modules.iter().map(|m| (m.name.clone(), m.file.clone())).collect();
        let mut ifaces = HashSet::new();
        for k in self.interfaces.keys() {
            ifaces.insert(k.clone());
        }
        self.custom_allocator = crate::noalloc::check_program(&mods, &ifaces)?;
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
            // spec `ref-parameters`: a task may outlive the caller, so it cannot borrow from it
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
            Some(t) => format!("B_T{}_{}", mangle(&ty_full(t)), fd.name),
            _ => format!("B_{}_{}", m, fd.name),
        };
        if fd_has_await(fd) {
            self.await_syms.insert(sym.clone());
        }
        Ok(Sig { sym, module: m.to_string(), params, ret, variadic, c_variadic: false, is_extern: false, receiver, public: fd.public })
    }

    fn infer_global(&mut self, e: Option<&Expr>, m: &str) -> R<Ty> {
        let Some(e) = e else { return self.err("global variable needs a type or a value") };
        Ok(match &e.kind {
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Text(_) => Ty::Text,
            ExprKind::Int(_) => Ty::int(),
            ExprKind::Char(_) => Ty::Char,
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
        let q = self.qname(m, path);
        if path.len() > 1 && !self.type_visible(&q, m) && self.type_exists(&q) {
            return self.private_name(path.last().unwrap(), type_owner(&q));
        }
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

    /// A `pub` declaration may not name a private type of its own package.
    fn check_public_signatures(&mut self) -> R<()> {
        let mods: Vec<(String, Vec<Decl>)> = self.modules.iter().map(|m| (m.name.clone(), m.file.decls.clone())).collect();
        for (m, decls) in &mods {
            self.cur_mod = m.clone();
            for d in decls {
                if !d.public {
                    continue;
                }
                self.line = d.line;
                let mut tys: Vec<&Type> = Vec::new();
                let mut generics: Vec<&str> = Vec::new();
                let mut bounds: Vec<&Vec<String>> = Vec::new();
                let (kind, name) = match &d.kind {
                    DeclKind::Func(fd) => {
                        for p in &fd.params {
                            tys.extend(p.ty.iter());
                        }
                        if let Some(r) = &fd.ret {
                            tys.extend(r.ty.iter());
                        }
                        generics.extend(fd.generics.iter().map(|g| g.name.as_str()));
                        bounds.extend(fd.generics.iter().filter_map(|g| g.bound.as_ref()));
                        (if fd.receiver.is_some() { "method" } else { "func" }, fd.name.as_str())
                    }
                    DeclKind::Extern(ed) => {
                        for p in &ed.params {
                            tys.extend(p.ty.iter());
                        }
                        tys.extend(ed.ret.iter());
                        ("extern func", ed.name.as_str())
                    }
                    DeclKind::Var(vd) => {
                        tys.extend(vd.ty.iter());
                        (if vd.mutable { "var" } else { "let" }, vd.name.as_str())
                    }
                    DeclKind::Type(td) => {
                        generics.extend(td.generics.iter().map(|g| g.name.as_str()));
                        bounds.extend(td.generics.iter().filter_map(|g| g.bound.as_ref()));
                        match &td.body {
                            TypeBody::Record(fs) => tys.extend(fs.iter().filter(|f| f.public).map(|f| &f.ty)),
                            TypeBody::Enum(vs) => tys.extend(vs.iter().flat_map(|v| v.fields.iter())),
                            TypeBody::Alias(t) => tys.push(t),
                            TypeBody::Interface(sigs) => {
                                for sg in sigs {
                                    for p in &sg.params {
                                        tys.extend(p.ty.iter());
                                    }
                                    if let Some(r) = &sg.ret {
                                        tys.extend(r.ty.iter());
                                    }
                                }
                            }
                        }
                        ("type", td.name.as_str())
                    }
                    DeclKind::Embed(_) => continue,
                };
                let mut names: Vec<String> = Vec::new();
                for t in tys {
                    named_types(t, &generics, &mut names);
                }
                for b in bounds {
                    if b.len() == 1 {
                        names.push(b[0].clone());
                    }
                }
                for n in names {
                    let q = format!("{}.{}", m, n);
                    if self.type_exists(&q) && !self.pub_types.contains(&q) {
                        return self.err(format!(
                            "pub {} '{}' exposes private type '{}'; mark '{}' pub or make '{}' private",
                            kind, name, n, n, name
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn type_exists(&self, q: &str) -> bool {
        self.tmpl_recs.contains_key(q)
            || self.rec_decls.contains_key(q)
            || self.interfaces.contains_key(q)
            || self.enum_decls.contains_key(q)
            || self.aliases.contains_key(q)
    }

    fn resolve(&mut self, t: &Type, m: &str) -> R<Ty> {
        match t {
            Type::Shared(inner) => return self.resolve(inner, m),
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
                        "char" => return Ok(Ty::Char),
                        "error" => return Ok(Ty::Err),
                        "ErrorKind" => return Ok(Ty::Kind),
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
                        "Rc" | "Weak" if args.len() == 1 => {
                            let inner = self.resolve(&args[0], m)?;
                            if !matches!(inner, Ty::Rec(_) | Ty::Param { .. }) {
                                return self.err(format!("{}<T> needs a record type, found {}", n, inner.show()));
                            }
                            return Ok(if n == "Rc" { Ty::Rc(Box::new(inner)) } else { Ty::Weak(Box::new(inner)) });
                        }
                        _ => {}
                    }
                }
                if path.len() == 2 && path[0] == "c" {
                    if let Some(t) = c_type(&path[1]) {
                        return Ok(t);
                    }
                }
                let q = self.qname(m, path);
                if path.len() > 1 && !self.type_visible(&q, m) && self.type_exists(&q) {
                    return self.private_name(path.last().unwrap(), type_owner(&q));
                }
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
                if path.len() == 1 && path[0] == "Self" {
                    return self.err("`Self` is only valid inside an interface method signature");
                }
                if path.len() == 1 && path[0] == "Type" {
                    if self.driver && self.rec_decls.contains_key("reflect.Type") {
                        return Ok(Ty::Rec("reflect.Type".to_string()));
                    }
                    return self.err("`Type` exists only at compile time: use it in a `comptime func` or as a `comptime` parameter");
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
                        if d.attrs.iter().any(|a| a.name == "export") {
                            if let Some(sg) = self.funcs.get(&(m.to_string(), fd.name.clone())) {
                                self.exported.insert(sg.sym.clone());
                            }
                        }
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
        let mut rcs: Vec<String> = self.rc_recs.iter().filter(|q| self.records.contains_key(*q)).cloned().collect();
        rcs.sort();
        for q in rcs {
            self.gen_rc_drop(&q);
        }
        let pruned = self.prune_units(&main_mod, &mut inits);

        let bare = self.target == Target::Bare;
        let linux = self.target == Target::Linux;
        // a Linux program that calls C is dynamically linked: it needs `exit` (to flush what C
        // buffered) and the libraries named by `#[link]` (the C library when none is)
        let dynamic = linux && !self.extern_syms.is_empty();
        if dynamic {
            self.extern_syms.insert("exit".to_string());
            self.extern_syms.insert("fflush".to_string());
        }
        let mut out = String::new();
        out.push_str("; Generated by the Brasa compiler. Do not edit.
option casemap:none
");
        if dynamic {
            let mut needed: Vec<String> = self.libs.iter().cloned().collect();
            if needed.is_empty() {
                needed.push("c".into());
            }
            for l in needed {
                out.push_str(&format!("; link {}\n", l));
            }
        }
        out.push('\n');
        if !bare && !linux {
            let heap_syms: &[&str] = if self.custom_allocator.is_some() { &[] } else { &["GetProcessHeap", "HeapAlloc", "HeapFree"] };
            for s in ["ExitProcess", "GetStdHandle", "WriteFile", "SetConsoleOutputCP"].iter().chain(heap_syms.iter()).copied() {
                self.extern_syms.insert(s.to_string());
                self.extern_lib.entry(s.to_string()).or_insert_with(|| "kernel32".to_string());
            }
            let mut by_lib: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for s in &self.extern_syms {
                let Some(lib) = self.extern_lib.get(s) else {
                    return Err(format!("extern function '{}' has no #[link(\"library\")] naming the library that provides it", s));
                };
                by_lib.entry(lib.clone()).or_default().push(s.clone());
            }
            // kernel32 first (the runtime), the others by name, so the image is reproducible
            let mut imports: Vec<(String, Vec<String>)> = Vec::new();
            if let Some(k) = by_lib.remove("kernel32") {
                imports.push(("kernel32".to_string(), k));
            }
            imports.extend(by_lib);
            self.win_imports = imports;
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
        } else if linux {
            // The kernel enters with the stack pointing at argc. The heap is the zeroed memory
            // after the image, from the next page.
            start.push_str("brasa_start proc
    and rsp, -16
    sub rsp, 48
");
            if self.custom_allocator.is_none() {
                start.push_str("    lea rax, rt_image_end
    add rax, 4095
    and rax, -4096
    mov qword ptr rt_brk, rax
    add rax, 268435456
    mov qword ptr rt_heap_end, rax
");
            }
        } else {
            start.push_str("brasa_start proc
    sub rsp, 40
    mov ecx, 65001
    call qword ptr GOT_SetConsoleOutputCP
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
        let custom = self.custom_allocator.clone();
        let common = match &custom {
            Some(c) if c.realloc_sym.is_some() => RUNTIME_COMMON.replace(
                "rt_lp_cap:\n    mov qword ptr [rbp - 24], rax\n",
                &format!("rt_lp_cap:\n    mov qword ptr [rbp - 24], rax\n{}", RUNTIME_LIST_REALLOC),
            ),
            _ => RUNTIME_COMMON.to_string(),
        };
        let heap_runtime = |rt: String| -> String {
            match &custom {
                Some(c) => format!("{}{}", strip_heap(&rt), custom_runtime(c)),
                None => rt,
            }
        };
        if bare {
            // the entry point is the first byte of the image
            out.push_str(&start);
            out.push_str(&common);
            out.push_str(&heap_runtime(RUNTIME_BARE.replace("B_CONSOLE_WRITE", &format!("B_{}_console_write", main_mod))));
        } else if linux {
            out.push_str(&start);
            out.push_str(&common);
            if dynamic {
                // leave through the C library, so that it can flush what it buffered
                // and flush what C buffered before Brasa writes, so the output keeps its order
                out.push_str(&heap_runtime(
                    RUNTIME_LINUX
                        .replace(
                            "rt_exit proc\n    mov edi, ecx\n    mov eax, 231\n    syscall\nrt_exit endp",
                            "rt_exit proc\n    sub rsp, 40\n    mov edi, ecx\n    call qword ptr GOT_exit\nrt_exit endp",
                        )
                        .replace(
                            "rt_write proc\n    push rdi\n    push rsi\n    mov rsi, rcx\n",
                            "rt_write proc\n    push rdi\n    push rsi\n    push rcx\n    push rdx\n    sub rsp, 8\n    xor edi, edi\n    call qword ptr GOT_fflush\n    add rsp, 8\n    pop rdx\n    pop rcx\n    mov rsi, rcx\n",
                        ),
                ));
            } else {
                out.push_str(&heap_runtime(RUNTIME_LINUX.to_string()));
            }
            out.push('\n');
            if pruned.procs.iter().any(|p| p.contains("rt_map_")) {
                out.push_str(RUNTIME_MAP);
                out.push('\n');
            }
        } else {
            out.push_str(&common);
            out.push_str(&heap_runtime(RUNTIME_WIN.to_string()));
            out.push('\n');
            if pruned.procs.iter().any(|p| p.contains("rt_map_")) {
                out.push_str(RUNTIME_MAP);
                out.push('\n');
            }
            out.push_str(&start);
        }
        if pruned.procs.iter().any(|p| p.contains("rt_dv_")) {
            out.push_str(RUNTIME_DERIVE);
            out.push('\n');
        }
        for p in &pruned.procs {
            out.push_str(p);
            out.push('\n');
        }
        // C reaches a callback with the System V convention (arguments in rdi, rsi, rdx, rcx):
        // the thunk moves up to four of them where Brasa functions expect them
        for t in &pruned.thunks {
            out.push_str(t);
        }
        out.push_str(".data
");
        if linux {
            for s in &self.extern_syms {
                out.push_str(&format!("GOT_{} dq 0\n", s));
            }
        }
        if !bare && !linux {
            // the import address table: one run of slots per library, each run ended by a zero
            // (the loader fills the slots; the PE writer builds the import directory around them)
            for (dll, syms) in &self.win_imports {
                out.push_str(&format!("; imports from {}
", dll));
                for s in syms {
                    out.push_str(&format!("GOT_{} dq 0
", s));
                }
                out.push_str("dq 0
");
            }
        }
        out.push_str(RUNTIME_DATA_COMMON);
        let data = if bare {
            RUNTIME_DATA_BARE
        } else if linux {
            RUNTIME_DATA_LINUX
        } else {
            RUNTIME_DATA_WIN
        };
        if custom.is_some() {
            out.push_str(&strip_heap_data(data));
        } else {
            out.push_str(data);
        }
        for g in &pruned.globals {
            out.push_str(g);
        }
        for f in &pruned.closures {
            out.push_str(f);
        }
        for e in &pruned.statics {
            out.push_str(e);
        }
        for d in &pruned.data {
            out.push_str(d);
        }
        if linux {
            // the heap starts on the page after this
            out.push_str("rt_image_end dq 0\n");
        }
        if self.threads {
            out = lock_counts(&out);
        }
        out.push_str("
end
");
        Ok(out)
    }

    /// Keeps what can be reached (module `prune`) and returns the text of each part of the program.
    /// With pruning off everything is kept. Also drops the C imports nobody calls and clears
    /// `uses_ui` when no `ui` function is reachable.
    fn prune_units(&mut self, main_mod: &str, inits: &mut Vec<String>) -> Pruned {
        use crate::prune::{data_units, idents, reachable, Kind, Unit};
        let mut units: Vec<Unit> = Vec::new();
        let mut cuts = [0usize; 5];
        for p in &self.procs {
            let defs: Vec<String> = p.lines().filter_map(|l| l.strip_suffix(" proc")).map(|n| n.to_string()).collect();
            let kind = if defs.iter().any(|d| d.starts_with("B_init_")) { Kind::Init } else { Kind::Plain };
            units.push(Unit { text: p.clone(), defs, kind });
        }
        cuts[0] = units.len();
        for (cb, (kinds, ret)) in &self.callbacks {
            let text = callback_thunk(cb, kinds, *ret);
            units.push(Unit { text, defs: vec![format!("CB_{}", cb)], kind: Kind::Plain });
        }
        cuts[1] = units.len();
        let mut globals: Vec<&Global> = self.globals.values().filter(|g| g.konst.is_none()).collect();
        globals.sort_by(|a, b| a.sym.cmp(&b.sym));
        for g in globals {
            units.push(Unit { text: format!("{} dq 0\n", g.sym), defs: vec![g.sym.clone()], kind: Kind::Plain });
        }
        cuts[2] = units.len();
        for f in &self.fn_closures {
            units.push(Unit { text: format!("dq {}, 0\nC_{} dq {}, 0\n", IMMORTAL, f, f), defs: vec![format!("C_{}", f)], kind: Kind::Plain });
        }
        cuts[3] = units.len();
        for (q, i) in &self.enum_statics {
            let def = format!("E_{}_{}", sym_of(q), i);
            units.push(Unit { text: format!("dq {}, 0\n{} dq {}\n", IMMORTAL, def, i), defs: vec![def], kind: Kind::Plain });
        }
        cuts[4] = units.len();
        units.extend(data_units(&self.data));

        let keep: Vec<bool> = if crate::prune::enabled() {
            let mut roots = format!("B_{}_main B_{}_console_write\n", main_mod, main_mod);
            for e in &self.exported {
                roots.push_str(e);
                roots.push('\n');
            }
            if let Some(c) = &self.custom_allocator {
                roots.push_str(&custom_runtime(c));
            }
            reachable(&units, &[&roots])
        } else {
            vec![true; units.len()]
        };
        let pick = |from: usize, to: usize| -> Vec<String> { (from..to).filter(|i| keep[*i]).map(|i| units[i].text.clone()).collect() };
        let out = Pruned {
            procs: pick(0, cuts[0]),
            thunks: pick(cuts[0], cuts[1]),
            globals: pick(cuts[1], cuts[2]),
            closures: pick(cuts[2], cuts[3]),
            statics: pick(cuts[3], cuts[4]),
            data: pick(cuts[4], units.len()),
        };
        if crate::prune::enabled() {
            let alive: HashSet<String> = (0..units.len()).filter(|i| keep[*i]).flat_map(|i| units[i].defs.clone()).collect();
            inits.retain(|i| alive.contains(i));
            let mut mentioned: HashSet<&str> = HashSet::new();
            for i in 0..units.len() {
                if keep[i] {
                    mentioned.extend(idents(&units[i].text));
                }
            }
            // runtime imports are named by the runtime itself and are always kept
            const RUNTIME_IMPORTS: [&str; 6] = ["ExitProcess", "GetStdHandle", "WriteFile", "SetConsoleOutputCP", "GetProcessHeap", "HeapAlloc"];
            self.extern_syms.retain(|s| mentioned.contains(format!("GOT_{}", s).as_str()) || RUNTIME_IMPORTS.contains(&s.as_str()));
            self.uses_ui = self.uses_ui && mentioned.iter().any(|t| t.starts_with("B_ui_"));
            self.threads = self.threads || (self.uses_thread_lib && mentioned.iter().any(|t| t.starts_with("B_thread_")));
        } else {
            self.threads = self.threads || self.uses_thread_lib;
        }
        out
    }

    /// The profile of the dialect `name` as the current module sees it: declared by a package the
    /// module imports with `use`.
    fn sql_profile(&mut self, name: &str) -> R<crate::sqlblock::Profile> {
        let imps: Vec<String> = self.imports.get(&self.cur_mod).map(|s| s.iter().cloned().collect()).unwrap_or_default();
        let mut found: Vec<(String, crate::sqlblock::Profile)> = Vec::new();
        for m in &imps {
            let key = self.mod_aliases.get(&(self.cur_mod.clone(), m.clone())).cloned().unwrap_or_else(|| m.clone());
            let Some(module) = self.modules.iter().find(|x| x.name == key) else { continue };
            for d in &module.file.decls {
                if let Some(p) = crate::sqlblock::profile_from_attrs(&d.attrs) {
                    match p {
                        Ok(p) if p.name == name => found.push((key.clone(), p)),
                        Ok(_) => {}
                        Err(e) => return self.err(format!("package '{}': {}", key, e)),
                    }
                }
            }
        }
        match found.len() {
            1 => Ok(found.remove(0).1),
            0 => {
                // a dialect of the standard library names its package
                let provider = [("postgres", "postgres"), ("tsql", "sqlserver")].iter().find(|(d, _)| *d == name).map(|(_, p)| *p);
                match provider {
                    Some(p) => self.err(format!("the sql dialect '{}' is provided by the package '{}', which must be imported with `use {}`", name, p, p)),
                    None => self.err(format!("no imported package declares the sql dialect '{}'", name)),
                }
            }
            _ => self.err(format!("the sql dialect '{}' is declared by more than one imported package ({})", name, found.iter().map(|f| f.0.clone()).collect::<Vec<_>>().join(", "))),
        }
    }

    /// A `sql <dialect>` block: a `db.Sql` made from the text with placeholders and the values of
    /// the holes, each converted to a `db.Arg` by its type.
    fn sql_block(&mut self, dialect: &str, raw: &str, line: u32, want: Option<&Ty>) -> R<Ty> {
        use crate::sqlblock::scan;
        self.line = line;
        let block_line = line;
        let profile = self.sql_profile(dialect)?;
        if !self.imports.get(&self.cur_mod).map_or(false, |s| s.contains("db")) {
            return self.err("a `sql` block needs `use db`");
        }
        let scanned = match scan(&profile, raw, block_line + 1) {
            Ok(s) => s,
            Err(e) => {
                self.line = e.line;
                return self.err(e.msg);
            }
        };
        let mk = |kind: ExprKind, line: u32| Expr { kind, line };
        let db = |name: &str, line: u32| mk(ExprKind::Field { expr: Box::new(mk(ExprKind::Ident("db".to_string()), line)), name: name.to_string(), safe: false }, line);
        let call = |callee: Expr, args: Vec<Expr>, line: u32| mk(ExprKind::Call { callee: Box::new(callee), args: args.into_iter().map(|value| Arg { name: None, by_ref: false, spread: false, value }).collect(), targs: Vec::new() }, line);
        let text = |s: &str, line: u32| mk(ExprKind::Text(vec![TextPart::Lit(s.to_string())]), line);
        let mut args = Vec::new();
        for h in &scanned.holes {
            let e = match crate::parser::parse_expr_at(&h.src, h.line) {
                Ok(e) => e,
                Err(d) => {
                    self.line = h.line;
                    return self.err(format!("in the hole {{{}}}: {}", h.src, d.msg));
                }
            };
            // the type of the value, found by compiling it once and undoing that
            self.line = h.line;
            let snap = self.snapshot();
            let t = self.value(&e, None);
            self.restore(snap);
            let t = t?;
            let arg = match &t {
                Ty::Int { .. } => call(db("int", h.line), vec![e], h.line),
                Ty::Float { bits: 64 } => call(db("float", h.line), vec![e], h.line),
                Ty::Float { .. } => {
                    let wide = call(mk(ExprKind::Ident("f64".to_string()), h.line), vec![e], h.line);
                    call(db("float", h.line), vec![wide], h.line)
                }
                Ty::Bool => call(db("bool", h.line), vec![e], h.line),
                // a `text?` is a `text` that may be nil: the helper sends NULL for nil
                Ty::Text => call(db("opt_str", h.line), vec![e], h.line),
                Ty::Nil => call(db("null", h.line), Vec::new(), h.line),
                Ty::List(el) if **el == (Ty::Float { bits: 64 }) => call(db("vector", h.line), vec![e], h.line),
                Ty::Enum(q) if q == "db.Arg" => e,
                other => return self.err(format!("a value of type {} cannot be a parameter of a sql block (use int, float, bool, text, []float or db.Arg)", other.show())),
            };
            args.push(arg);
        }
        self.line = block_line;
        let call_expr = if args.is_empty() {
            call(db("sql_const", block_line), vec![text(dialect, block_line), text(&scanned.text, block_line)], block_line)
        } else {
            call(db("sql", block_line), vec![text(dialect, block_line), text(&scanned.text, block_line), mk(ExprKind::List(args), block_line)], block_line)
        };
        self.expr(&call_expr, want)
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
        for p in &fd.params {
            if matches!(p.ty, Some(Type::Shared(_))) {
                self.fs.shared_locals.insert(p.name.clone());
            }
        }
        let has_await = fd_has_await(fd);
        if has_await && fd.name == "drop" && fd.receiver.is_some() {
            return self.err("`await` cannot be used in a `drop` method");
        }
        if has_await && self.exported.contains(&sig.sym) {
            return self.err("`await` cannot be used in a function called from C");
        }
        if self.opt && !naked && self.target != Target::Bare {
            self.fs.regs = crate::regalloc::plan(fd);
        }
        let mut incoming: Vec<(String, Ty, bool)> = Vec::new();
        if let (Some(r), Some(rt)) = (&fd.receiver, &sig.receiver) {
            incoming.push((r.name.clone(), rt.clone(), r.by_ref && !matches!(rt, Ty::List(_) | Ty::Map(..))));
        }
        for p in &sig.params {
            incoming.push((p.name.clone(), p.ty.clone(), p.by_ref));
        }
        if !naked {
            let mut refs: Vec<(String, bool)> = Vec::new();
            if let (Some(r), Some(rt)) = (&fd.receiver, &sig.receiver) {
                if r.by_ref {
                    refs.push((r.name.clone(), plain_scalar(rt)));
                }
            }
            for p in &sig.params {
                if p.by_ref {
                    refs.push((p.name.clone(), plain_scalar(&p.ty)));
                }
            }
            self.check_refs(&refs, &fd.body)?;
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
        for (i, (name, ty, by_ref)) in params.iter().enumerate() {
            // a `ref` list or map is passed as the address of the caller's slot (copy on write)
            let ind = *by_ref && matches!(ty, Ty::List(_) | Ty::Map(..));
            let off = self.slot();
            if i < 4 {
                pro.push(format!("    mov qword ptr [rbp - {}], {}", off, ARG_REGS[i]));
            } else {
                pro.push(format!("    mov rax, qword ptr [rbp + {}]", 16 + 8 * i));
                pro.push(format!("    mov qword ptr [rbp - {}], rax", off));
            }
            let reg = if ind { None } else { self.reg_for(name, ty) };
            if let Some(n) = norm_insn(ty) {
                pro.push(format!("    mov rax, qword ptr [rbp - {}]", off));
                pro.push(format!("    {}", n));
                pro.push(format!("    mov qword ptr [rbp - {}], rax", off));
            }
            if let Some(r) = reg {
                pro.push(format!("    mov {}, qword ptr [rbp - {}]", r, off));
            }
            let rc = self.is_rc(ty) && !ind;
            let sc = &mut self.fs.scopes[0];
            sc.vars.insert(name.clone(), Local { off, ty: ty.clone(), mutable: true, captured: false, reg, imm: None, is_ref: *by_ref, ind });
            if rc {
                sc.owned.push((off, ty.clone()));
                self.fs.may_own = true;
            }
        }
        pro
    }

    /// Per-record `B_drop_*` (releases heap fields) and `B_copy_*` (shallow copy: nested plain records are copied, lists, maps and text are retained, so they are copied by their first write).
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

    /// `B_rcdrop_*`: the drop of an object made by `new`. The weak cell is cleared first (so `upgrade()`
    /// is `nil` from here on, also inside `drop`), then the record's own drop runs (user `drop`, heap fields).
    fn gen_rc_drop(&mut self, q: &str) {
        let s = sym_of(q);
        let cell = self.cell_off(q);
        let inner = self.rec_drop(q);
        let mut d = format!(
            "B_rcdrop_{s} proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    mov qword ptr [rbp - 8], rcx\n    mov rcx, qword ptr [rcx + {cell}]\n    test rcx, rcx\n    jz Lrcd_{s}\n    mov qword ptr [rcx], 0\n    call rt_release\nLrcd_{s}:\n    mov rcx, qword ptr [rbp - 8]\n"
        );
        if inner != "0" {
            d.push_str(&format!("    call {}\n", inner));
        }
        d.push_str(&format!("    mov rsp, rbp\n    pop rbp\n    ret\nB_rcdrop_{s} endp\n"));
        self.procs.push(d);
    }

    /// `weak(x)`: rax = the weak cell of the `Rc<T>` `x` (made on first use, one reference for the caller).
    fn weak_of(&mut self, args: &[Arg]) -> R<Ty> {
        if args.len() != 1 {
            return self.err("weak() takes one argument, an Rc<T>");
        }
        let (t, own) = self.operand(&args[0].value, None)?;
        let Ty::Rc(inner) = &t else {
            return self.err(format!("weak() takes an Rc<T> (made with `new`), found {}", t.show()));
        };
        let Ty::Rec(r) = &**inner else {
            return self.err(format!("weak() takes an Rc<T> of a record, found {}", t.show()));
        };
        let cell = self.cell_off(r);
        let (have, done) = (self.lbl("wkhave"), self.lbl("wkdone"));
        let slot = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", slot));
        self.e("test rax, rax");
        self.e(format!("jz {}", done));
        self.e(format!("mov rdx, qword ptr [rax + {}]", cell));
        self.e("test rdx, rdx");
        self.e(format!("jnz {}", have));
        self.e("mov ecx, 8");
        self.e("xor edx, edx");
        self.e("call rt_alloc");
        self.e(format!("mov rcx, qword ptr [rbp - {}]", slot));
        self.e("mov qword ptr [rax], rcx");
        self.e(format!("mov qword ptr [rcx + {}], rax", cell));
        self.e("mov rdx, rax");
        self.label(&have);
        self.e("mov rax, rdx");
        self.retain();
        self.label(&done);
        if own {
            let keep = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", keep));
            self.release_slot(slot);
            self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
            self.free(keep);
        }
        self.free(slot);
        Ok(Ty::Weak(inner.clone()))
    }

    /// `w.upgrade()` with the weak cell in rax (owned): rax = a new strong reference or nil.
    fn weak_upgrade(&mut self, inner: &Ty, args: &[Arg]) -> R<Ty> {
        if !args.is_empty() {
            return self.err("upgrade() takes no arguments");
        }
        let nil = self.lbl("upnil");
        let slot = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", slot));
        self.e("test rax, rax");
        self.e(format!("jz {}", nil));
        self.e("mov rax, qword ptr [rax]");
        self.e("test rax, rax");
        self.e(format!("jz {}", nil));
        self.retain();
        self.label(&nil);
        let keep = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", keep));
        self.release_slot(slot);
        self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
        self.free(keep);
        self.free(slot);
        Ok(Ty::Rc(Box::new(inner.clone())))
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

    /// An integer literal must fit the type it takes; a too-big `int` suggests `long`.
    fn lit_check(&self, n: u64, neg: bool, t: &Ty) -> R<()> {
        if lit_fits(n, neg, t) {
            return Ok(());
        }
        let shown = if neg { format!("-{}", n) } else { n.to_string() };
        let hint = if *t == Ty::int() && lit_fits(n, neg, &Ty::i64()) { "; write it as a long (`let x: long = ...`)" } else { "" };
        self.err(format!("integer literal {} is out of range for {}{}", shown, t.show(), hint))
    }

    fn check(&self, want: &Ty, got: &Ty, what: &str) -> R<()> {
        if compatible(want, got) {
            Ok(())
        } else {
            let hint = if want.is_float() && got.is_int() {
                " (write a float literal such as 3.0, or use float(x))".to_string()
            } else if want.is_int() && (got.is_int() || got.is_float()) {
                format!(" (use {}(x) to convert)", want.show())
            } else {
                String::new()
            };
            self.err(format!("{}: expected {}, found {}{}", what, want.show(), got.show(), hint))
        }
    }

    fn rec_size(&self, r: &str) -> u32 {
        self.records[r].size
    }

    /// The error for a name that exists but is not `pub`, used from another package.
    fn private_name<T>(&self, name: &str, pkg: &str) -> R<T> {
        self.err(format!("'{}' is private to package '{}'; mark it `pub` to use it from another package", name, pkg))
    }

    /// A function, global or variant of package `m` may be named from the current package.
    fn check_member_vis(&self, m: &str, name: &str, public: bool) -> R<()> {
        if public || m == self.cur_mod || self.cur_mod == "comptime_run" {
            Ok(())
        } else {
            self.private_name(name, m)
        }
    }

    /// The type `q` (qualified) is `pub` or declared in package `from`.
    fn type_visible(&self, q: &str, from: &str) -> bool {
        self.pub_types.contains(q) || type_owner(q) == from
    }

    /// The declared fields of record `r` (an instance of a generic record uses its template's).
    fn rec_field_decls(&self, r: &str) -> Option<(&str, &[Field])> {
        let (m, f) = self.rec_decls.get(r)?;
        if f.is_empty() {
            if let Some((tq, _)) = self.inst_args.get(r) {
                if let Some((tm, td, _)) = self.tmpl_recs.get(tq) {
                    if let TypeBody::Record(fs) = &td.body {
                        return Some((tm.as_str(), fs.as_slice()));
                    }
                }
            }
        }
        Some((m.as_str(), f.as_slice()))
    }

    /// `field_of` for source the program wrote: a private field of another package's record is an error.
    fn fld(&self, r: &str, name: &str) -> R<Option<(Ty, u32)>> {
        let res = self.field_of(r, name);
        if res.is_some() {
            if let Some((owner, fields)) = self.rec_field_decls(r) {
                if owner != self.cur_mod && fields.iter().any(|f| f.name == name && !f.public) {
                    return self.err(format!("field '{}' of '{}' is private to package '{}'", name, r, owner));
                }
            }
        }
        Ok(res)
    }

    /// A record literal or `new` of another package's record needs every field to be `pub`.
    fn check_constructible(&self, r: &str) -> R<()> {
        if let Some((owner, fields)) = self.rec_field_decls(r) {
            if owner != self.cur_mod && fields.iter().any(|f| !f.public) {
                return self.err(format!(
                    "cannot create '{}' here: it has private fields; use a function from package '{}'",
                    r,
                    owner
                ));
            }
        }
        Ok(())
    }

    /// A method of another package must be `pub`.
    fn check_method_vis(&self, sig: &Sig, name: &str) -> R<()> {
        if sig.public || sig.module == self.cur_mod || name == "drop" || self.cur_mod == "comptime_run" {
            Ok(())
        } else {
            self.private_name(name, &sig.module)
        }
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

    /// The module key a qualifier written in module `from` stands for.
    fn modkey(&self, from: &str, q: &str) -> String {
        self.mod_aliases.get(&(from.to_string(), q.to_string())).cloned().unwrap_or_else(|| q.to_string())
    }

    /// The qualified name of the type a path names: one segment is a type of `m`, two are
    /// `<imported package>.<type>`.
    fn qname(&self, m: &str, path: &[String]) -> String {
        if path.len() == 1 {
            format!("{}.{}", m, path[0])
        } else {
            format!("{}.{}", self.modkey(m, &path[0]), path[1..].join("."))
        }
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
            Ty::Map(..) => self.new_map(&t.clone())?,
            Ty::Rec(r) => self.record_literal(&r.clone(), &[])?,
            Ty::Text => {
                let l = self.string_lit("");
                self.e(format!("lea rax, {}", l));
            }
            Ty::Chan(_) | Ty::Err | Ty::Kind | Ty::Result(_) => return self.err(format!("a {} needs an explicit value", t.show())),
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
            Ty::Text | Ty::Kind => {}
            Ty::Int { .. } => {
                self.e("mov rcx, rax");
                self.e("call rt_text_from_int");
            }
            Ty::Char => {
                self.e("mov rcx, rax");
                self.e("call rt_text_from_char");
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
                let is_shared = matches!(vd.ty, Some(Type::Shared(_)))
                    || matches!(&vd.value, Some(Expr { kind: ExprKind::Shared(_), .. }))
                    || matches!(&vd.value, Some(Expr { kind: ExprKind::Ident(n), .. }) if self.fs.shared_locals.contains(n));
                if is_shared {
                    self.fs.shared_locals.insert(vd.name.clone());
                } else {
                    self.fs.shared_locals.remove(&vd.name);
                }
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
            StmtKind::Block { name, .. } => return self.err(format!("block '{}' was not expanded", name)),
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
                ExprKind::Call { callee, args, .. }
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

    // ---------------- copy on write ----------------
    //
    // A list or map is shared by reference counting, so a write first makes sure the writer's own
    // place holds the only reference (`rt_list_unique`, `rt_map_unique`): when someone else holds it
    // too, it is copied one level and the place gets the copy. Writes therefore work on the place
    // (the address of the slot that owns the list), never on a retained temporary.

    /// The static type of a variable, field or element path, or `None` for anything else.
    fn path_ty(&self, e: &Expr) -> Option<Ty> {
        match &e.kind {
            ExprKind::Ident(n) => {
                if let Some(l) = self.lookup(n) {
                    return Some(l.ty);
                }
                let g = self.globals.get(&(self.cur_mod.clone(), n.clone()))?;
                if g.konst.is_some() {
                    None
                } else {
                    Some(g.ty.clone())
                }
            }
            ExprKind::Field { expr, name, safe: false } => {
                let Ty::Rec(r) = rc_view(&self.path_ty(expr)?) else { return None };
                self.field_of(&r, name).map(|f| f.0)
            }
            ExprKind::Index { expr, .. } => match self.path_ty(expr)? {
                Ty::List(el) => Some(*el),
                Ty::Map(_, v) => Some(*v),
                _ => None,
            },
            _ => None,
        }
    }

    /// `lea rdx, <copy function>` for the elements of a list or the values of a map, or `xor edx, edx`
    /// when they are retained as they are.
    fn clone_fn_for(&self, el: &Ty) -> String {
        match el {
            Ty::Rec(r) if !self.shared_recs.contains(r) => format!("lea rdx, B_copy_{}", sym_of(r)),
            _ => "xor edx, edx".to_string(),
        }
    }

    /// rax = the address of a slot holding a list or map of type `ty`  ->  rax = that list or map,
    /// copied into the slot first when it is shared.
    fn unique_rax(&mut self, ty: &Ty) {
        self.e("mov rcx, rax");
        match ty {
            Ty::List(el) => {
                let c = self.clone_fn_for(el);
                self.e(c);
                self.e("call rt_list_unique");
            }
            Ty::Map(_, v) => {
                let c = self.clone_fn_for(v);
                self.e(c);
                self.e("call rt_map_unique");
            }
            _ => {}
        }
    }

    /// Keeps the owned value in rax alive until the end of the statement and leaves the address of
    /// the slot that holds it in rax.
    fn temp_place(&mut self) {
        let keep = self.slot();
        self.e(format!("mov qword ptr [rbp - {}], rax", keep));
        self.fs.stmt_temps.push(keep);
        self.fs.may_own = true;
        self.e(format!("lea rax, [rbp - {}]", keep));
    }

    /// rax = the address of the element `obj[index]` (a list or map element slot), after the
    /// container was made unique. A bad index stops the program before anything is copied.
    fn index_place(&mut self, obj: &Expr, index: &Expr) -> R<Ty> {
        let Some(ct) = self.path_ty(obj) else { return self.err("invalid assignment target") };
        match ct {
            Ty::List(el) => {
                let ity = self.expr(index, Some(&Ty::i64()))?;
                if !ity.is_int() {
                    return self.err(format!("list index must be a number, found {}", ity.show()));
                }
                let (it, st) = (self.temp(), self.temp());
                self.e(format!("mov qword ptr [rbp - {}], rax", it));
                let lt = self.write_place(obj)?;
                self.e(format!("mov qword ptr [rbp - {}], rax", st));
                let ok = self.lbl("cowok");
                self.e("mov rcx, qword ptr [rax]");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                self.e("cmp rdx, qword ptr [rcx]");
                self.e(format!("jb {}", ok));
                self.at("r9");
                self.e("jmp rt_index_oob");
                self.label(&ok);
                self.e(format!("mov rax, qword ptr [rbp - {}]", st));
                self.unique_rax(&lt);
                self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                self.e("mov rax, qword ptr [rax + 16]");
                self.e("lea rax, [rax + rdx * 8]");
                self.free(st);
                self.free(it);
                Ok(*el)
            }
            Ty::Map(k, v) => {
                let kt = self.value(index, Some(&k))?;
                self.check(&k, &kt, "map key")?;
                let (ks, st) = (self.temp(), self.temp());
                self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                let mt = self.write_place(obj)?;
                self.e(format!("mov qword ptr [rbp - {}], rax", st));
                // a missing key stops the program before anything is copied
                self.e("mov rcx, qword ptr [rax]");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.at("r8");
                self.e("call rt_map_get");
                self.e(format!("mov rax, qword ptr [rbp - {}]", st));
                self.unique_rax(&mt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.e("call rt_map_find");
                self.e("add rax, 24");
                if self.is_rc(&k) {
                    self.release_slot(ks);
                }
                self.free(st);
                self.free(ks);
                Ok(*v)
            }
            Ty::Text => self.err(TEXT_INDEX_MSG),
            _ => self.err("invalid assignment target"),
        }
    }

    /// rax = the address of the slot that owns the list or map `e`; every list and map on the way
    /// there has been made unique, the one in the slot itself has not.
    fn write_place(&mut self, e: &Expr) -> R<Ty> {
        match &e.kind {
            ExprKind::Ident(n) => {
                if let Some(l) = self.lookup(n) {
                    if l.ind {
                        self.e(format!("mov rax, qword ptr [rbp - {}]", l.off));
                    } else {
                        self.e(format!("lea rax, [rbp - {}]", l.off));
                    }
                    return Ok(l.ty);
                }
                let key = (self.cur_mod.clone(), n.clone());
                if let Some(g) = self.globals.get(&key) {
                    let (sym, ty) = (g.sym.clone(), g.ty.clone());
                    self.e(format!("lea rax, {}", sym));
                    return Ok(ty);
                }
            }
            ExprKind::Field { expr: obj, name, safe: false } if self.path_ty(e).is_some() => {
                let ot = self.rec_ptr(obj)?;
                if let Ty::Rec(r) = &rc_view(&ot) {
                    if let Some((fty, off)) = self.fld(r, name)? {
                        self.e(format!("add rax, {}", off));
                        return Ok(fty);
                    }
                }
            }
            ExprKind::Index { expr: obj, index } if self.path_ty(e).is_some() => {
                return self.index_place(obj, index);
            }
            _ => {}
        }
        // anything else is a value of its own: a temporary holds it
        let t = self.expr(e, None)?;
        if !matches!(t, Ty::List(_) | Ty::Map(..)) {
            return self.err(format!("cannot write to a {}", t.show()));
        }
        self.temp_place();
        Ok(t)
    }

    /// rax = the record that the path `e` names (a pointer), reached for a write: lists and maps on
    /// the way are made unique first.
    fn rec_ptr(&mut self, e: &Expr) -> R<Ty> {
        match &e.kind {
            ExprKind::Ident(n) => {
                if let Some(l) = self.lookup(n) {
                    self.load_local("rax", &l);
                    return Ok(l.ty);
                }
                let key = (self.cur_mod.clone(), n.clone());
                if let Some(g) = self.globals.get(&key) {
                    let (sym, ty) = (g.sym.clone(), g.ty.clone());
                    self.e(format!("mov rax, qword ptr {}", sym));
                    return Ok(ty);
                }
            }
            ExprKind::Field { expr: obj, name, safe: false } if self.path_ty(e).is_some() => {
                let ot = self.rec_ptr(obj)?;
                if let Ty::Rec(r) = &rc_view(&ot) {
                    if let Some((fty, off)) = self.fld(r, name)? {
                        self.e(format!("mov rax, qword ptr [rax + {}]", off));
                        return Ok(fty);
                    }
                }
            }
            ExprKind::Index { expr: obj, index } if self.path_ty(e).is_some() => {
                let el = self.index_place(obj, index)?;
                self.e("mov rax, qword ptr [rax]");
                return Ok(el);
            }
            _ => {}
        }
        let (t, own) = self.operand(e, None)?;
        if own {
            let keep = self.slot();
            self.e(format!("mov qword ptr [rbp - {}], rax", keep));
            self.fs.stmt_temps.push(keep);
            self.fs.may_own = true;
        }
        Ok(t)
    }

    /// `xs.push(v)`, `xs.clear()`, `xs.take()` on a list and `m.remove(k)`, `m.clear()` on a map,
    /// where the receiver is a variable, field or element path: the write goes to the receiver's own
    /// place. `Ok(None)` when the call is not one of these.
    fn cow_method(&mut self, obj: &Expr, pt: &Ty, name: &str, args: &[Arg]) -> R<Option<Ty>> {
        if matches!(name, "push" | "add" | "clear" | "take" | "remove" | "insert" | "pop") && !self.methods.contains_key(&(method_key(pt), name.to_string())) {
            self.shared_write_check(obj, true)?;
        }
        if self.methods.contains_key(&(method_key(pt), name.to_string())) {
            return Ok(None);
        }
        match (pt, name) {
            (Ty::List(el), "push" | "add") => {
                if args.len() != 1 {
                    return self.err(format!("'{}' takes one argument", name));
                }
                let vt = self.value(&args[0].value, Some(el))?;
                self.check(el, &vt, "list element")?;
                let v = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", v));
                let lt = self.write_place(obj)?;
                self.unique_rax(&lt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", v));
                self.e("call rt_list_push");
                self.free(v);
                Ok(Some(Ty::Void))
            }
            (Ty::List(_), "clear") | (Ty::Map(..), "clear") => {
                if !args.is_empty() {
                    return self.err("'clear' takes no arguments");
                }
                let lt = self.write_place(obj)?;
                self.unique_rax(&lt);
                self.e("mov rcx, rax");
                self.e(if matches!(pt, Ty::List(_)) { "call rt_list_clear" } else { "call rt_map_clear" });
                Ok(Some(Ty::Void))
            }
            (Ty::List(el), "take") => {
                let lt = self.write_place(obj)?;
                self.unique_rax(&lt);
                self.e("mov rcx, rax");
                self.at("rdx");
                self.e("call rt_list_take");
                Ok(Some((**el).clone()))
            }
            (Ty::Map(k, _), "remove") => {
                if args.len() != 1 {
                    return self.err("'remove' takes 1 argument(s)");
                }
                let kt = self.value(&args[0].value, Some(k))?;
                self.check(k, &kt, "map key")?;
                let ks = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                let mt = self.write_place(obj)?;
                self.unique_rax(&mt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.e("call rt_map_remove");
                if self.is_rc(k) {
                    self.release_slot(ks);
                }
                self.free(ks);
                Ok(Some(Ty::Void))
            }
            _ => Ok(None),
        }
    }

    /// `obj[index] = rhs` where `obj` is a list or map path.
    fn assign_index_cow(&mut self, obj: &Expr, index: &Expr, rhs: &Expr, ct: &Ty) -> R<()> {
        match ct {
            Ty::List(el) => {
                let ity = self.expr(index, Some(&Ty::i64()))?;
                if !ity.is_int() {
                    return self.err(format!("list index must be a number, found {}", ity.show()));
                }
                let (it, v, st) = (self.temp(), self.temp(), self.temp());
                self.e(format!("mov qword ptr [rbp - {}], rax", it));
                let vt = self.value(rhs, Some(el))?;
                self.check(el, &vt, "list element")?;
                self.e(format!("mov qword ptr [rbp - {}], rax", v));
                let lt = self.write_place(obj)?;
                self.e(format!("mov qword ptr [rbp - {}], rax", st));
                let ok = self.lbl("cowok");
                self.e("mov rcx, qword ptr [rax]");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                self.e("cmp rdx, qword ptr [rcx]");
                self.e(format!("jb {}", ok));
                self.at("r9");
                self.e("jmp rt_index_oob");
                self.label(&ok);
                self.e(format!("mov rax, qword ptr [rbp - {}]", st));
                self.unique_rax(&lt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                self.e(format!("mov r8, qword ptr [rbp - {}]", v));
                self.at("r9");
                self.e("call rt_list_set");
                self.free(st);
                self.free(v);
                self.free(it);
            }
            Ty::Map(k, v) => {
                let kt = self.value(index, Some(k))?;
                self.check(k, &kt, "map key")?;
                let (ks, vs) = (self.temp(), self.temp());
                self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                let vt = self.value(rhs, Some(v))?;
                self.check(v, &vt, "map value")?;
                self.e(format!("mov qword ptr [rbp - {}], rax", vs));
                let mt = self.write_place(obj)?;
                self.unique_rax(&mt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.e(format!("mov r8, qword ptr [rbp - {}]", vs));
                self.e("call rt_map_set");
                if self.is_rc(k) {
                    self.release_slot(ks);
                }
                self.free(vs);
                self.free(ks);
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn assign(&mut self, target: &Expr, op: &str, value: &Expr) -> R<()> {
        self.shared_write_check(target, false)?;
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
                    if l.ind {
                        // a `ref` list or map: the caller's own variable is replaced
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", l.off));
                        self.e("mov rcx, qword ptr [rdx]");
                        self.e("mov qword ptr [rdx], rax");
                        self.e("call rt_release");
                    } else if self.is_rc(&l.ty) {
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
                // a path into lists and maps makes each of them unique before the store
                let (ot, own) = if matches!(self.path_ty(obj).map(|t| rc_view(&t)), Some(Ty::Rec(_))) { (self.rec_ptr(obj)?, false) } else { self.operand(obj, None)? };
                let r = match &rc_view(&ot) {
                    Ty::Rec(r) => r.clone(),
                    Ty::Ptr(p) => match &**p {
                        Ty::Rec(r) => r.clone(),
                        _ => return self.err(format!("cannot assign field '{}' on {}", name, ot.show())),
                    },
                    _ => return self.err(format!("cannot assign field '{}' on {}", name, ot.show())),
                };
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                if let Some((fty, off)) = self.fld(&r, name)? {
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
                    self.check_method_vis(&sig, &format!("set_{}", name))?;
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
                if let Some(ct @ (Ty::List(_) | Ty::Map(..))) = self.path_ty(obj) {
                    return self.assign_index_cow(obj, index, rhs, &ct);
                }
                let (ot, own) = self.operand(obj, None)?;
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                let key_want = if let Ty::Map(k, _) = &ot { Some((**k).clone()) } else { None };
                let ity = if key_want.is_some() { self.value(index, key_want.as_ref())? } else { self.expr(index, key_want.as_ref())? };
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
                        if self.is_rc(k) {
                            self.release_slot(it);
                        }
                    }
                    Ty::List(el) => {
                        let vt = self.value(rhs, Some(el))?;
                        self.check(el, &vt, "list element")?;
                        self.e("mov r8, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                        self.at("r9");
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
            let rt = self.expr(rhs, Some(&ty))?;
            if rt != ty {
                // `0..n` with a `long` n counts in `long`; the bounds must otherwise agree
                match wider(&ty, &rt) {
                    Some(w) if rt.is_int() => {
                        for sc in self.fs.scopes.iter_mut().rev() {
                            if let Some(l) = sc.vars.get_mut(&vars[0]) {
                                l.ty = w.clone();
                                break;
                            }
                        }
                    }
                    _ => return self.err(format!("range bounds must have the same integer type, found {} and {}", ty.show(), rt.show())),
                }
            }
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
            if lt == Ty::Text {
                if vars.len() != 1 {
                    return self.err("a loop over a text has one variable: the character");
                }
                // the text is held for the whole loop, so the body may reassign the variable that named it
                let txt = self.hidden_local(lt);
                self.e(format!("mov qword ptr [rbp - {}], rax", txt));
                let off = self.slot();
                self.e(format!("mov qword ptr [rbp - {}], 0", off));
                let cv = self.declare_local(&vars[0], Ty::Char, false);
                self.e(format!("mov qword ptr [rbp - {}], 0", cv));
                self.label(&start);
                self.e(format!("mov rcx, qword ptr [rbp - {}]", txt));
                self.e(format!("mov rdx, qword ptr [rbp - {}]", off));
                self.e("cmp rdx, qword ptr [rcx]");
                self.e(format!("jae {}", end));
                self.e("call rt_text_next_char");
                self.e(format!("mov qword ptr [rbp - {}], rdx", off));
                self.e(format!("mov qword ptr [rbp - {}], rax", cv));
                let depth = self.fs.scopes.len();
                self.fs.loops.push(LoopCtx { cont: cont.clone(), brk: end.clone(), depth });
                self.block(body)?;
                self.fs.loops.pop();
                self.label(&cont);
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
                (Some(self.declare_local(&vars[0], Ty::int(), false)), self.declare_local(&vars[1], (*el).clone(), false))
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
        // lets an assembler error be reported at the Brasa line of the block
        self.e(format!("; asm {}.br:{}", self.cur_mod, self.line));
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
                _ => Ty::int(),
            };
            let fbits = if matches!(want, Some(Ty::Float { bits: 32 })) { 32 } else { 64 };
            if let Some(c) = self.fold(e, &t, fbits) {
                return Ok(self.emit_const(c, &t, fbits));
            }
        }
        match &e.kind {
            ExprKind::Int(n) => {
                let t = match want {
                    Some(t) if t.is_int() => t.clone(),
                    _ => Ty::int(),
                };
                self.lit_check(*n, false, &t)?;
                self.e(format!("mov rax, {}", imm64(*n as i64)));
                Ok(t)
            }
            ExprKind::Char(c) => {
                self.e(format!("mov eax, {}", *c as u32));
                Ok(Ty::Char)
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
                    self.new_map(&t)?;
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
                        self.new_map(&t)?;
                        self.e(format!("mov qword ptr [rbp - {}], rax", mt));
                        self.e(format!("mov rax, qword ptr [rbp - {}]", vs));
                        self.free(vs);
                        created = true;
                    }
                    self.e("mov r8, rax");
                    self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", mt));
                    self.e("call rt_map_set");
                    if kt.as_ref().map_or(false, |k| self.is_rc(k)) {
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
                self.check_constructible(&r)?;
                self.record_literal(&r, fields)?;
                Ok(Ty::Rec(r))
            }
            ExprKind::Lambda { params, body } => self.lambda(params, body, want),
            ExprKind::If { branches, else_body } => self.if_chain(branches, else_body.as_ref(), want, true),
            ExprKind::Match { scrutinee, arms } => self.match_expr(scrutinee, arms, want, true),
            ExprKind::Shared(inner) => self.shared_expr(inner, want),
            ExprKind::SpawnThread(call) => self.spawn_thread(call, e.line),
            ExprKind::New(inner) => {
                let ExprKind::Record { path, fields } = &inner.kind else {
                    return self.err("'new' needs a record literal such as `new Node{ ... }`");
                };
                let want_rec = want.map(rc_view);
                let r = match self.generic_literal(path, want_rec.as_ref())? {
                    Some(r) => r,
                    None => self.record_name(path, &self.cur_mod.clone())?,
                };
                self.check_constructible(&r)?;
                self.rc_recs.insert(r.clone());
                self.record_alloc(&r, fields, true)?;
                Ok(Ty::Rc(Box::new(Ty::Rec(r))))
            }
            ExprKind::Block { name, .. } => self.err(format!("block '{}' was not expanded", name)),
            ExprKind::Sql { dialect, raw } => self.sql_block(dialect, raw, e.line, want),
            ExprKind::Propagate(inner) => self.try_expr(inner),
            ExprKind::Await(inner) => {
                if let ExprKind::Call { callee, args, .. } = &inner.kind {
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
                    if let ExprKind::Int(n) = &inner.kind {
                        let t = match want {
                            Some(t) if t.is_int() => t.clone(),
                            _ => Ty::int(),
                        };
                        self.lit_check(*n, true, &t)?;
                        self.e(format!("mov rax, {}", imm64((*n as i64).wrapping_neg())));
                        return Ok(t);
                    }
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
                        self.at("r8");
                        self.e("call rt_map_get");
                        self.retain_if(v);
                        if self.is_rc(k) {
                            self.release_slot(ks);
                        }
                        self.free(ks);
                        (**v).clone()
                    }
                    Ty::List(el) => {
                        self.e("mov rdx, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.at("r9");
                        self.e("call rt_list_get");
                        self.retain_if(el);
                        (**el).clone()
                    }
                    Ty::Text => return self.err(TEXT_INDEX_MSG),
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
            ExprKind::Call { callee, args, targs } => self.call(callee, args, targs, want),
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
            self.check_member_vis(m, name, g.public)?;
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
            self.check_member_vis(m, name, sig.public)?;
            if sig.is_extern {
                return self.err(format!("extern function '{}' cannot be used as a value", name));
            }
            let ext = matches!(want, Some(Ty::Func { ext: true, .. }));
            if ext && self.await_syms.contains(&sig.sym) {
                return self.err("`await` cannot be used in a function called from C");
            }
            if ext && self.target == Target::Linux {
                // C calls with the System V convention: the thunk moves the arguments to ours
                let kind = |t: &Ty| match t {
                    Ty::Float { bits: 32 } => 1u8,
                    Ty::Float { .. } => 2,
                    _ => 0,
                };
                let kinds: Vec<u8> = sig.params.iter().map(|p| kind(&p.ty)).collect();
                self.callbacks.insert(sig.sym.clone(), (kinds, kind(&sig.ret)));
                self.e(format!("lea rax, CB_{}", sig.sym));
            } else if ext {
                self.e(format!("lea rax, {}", sig.sym));
            } else {
                self.fn_closures.insert(sig.sym.clone());
                self.e(format!("lea rax, C_{}", sig.sym));
            }
            return Ok(sig.func_ty(ext));
        }
        if let Some((q, idx)) = self.variants.get(&key).cloned() {
            self.check_member_vis(m, name, self.pub_types.contains(&q))?;
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
                    if l.is_ref && self.fs.unsafe_depth == 0 {
                        return self.ref_escape(name);
                    }
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
                let Some((fty, off)) = self.fld(&r, name)? else { return self.err(format!("{} has no field '{}'", r, name)) };
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

    /// A number literal (or arithmetic on literals) on the left adapts to an integer type on the
    /// right (`0 - x` with `x: u32`): that type, when it applies.
    fn literal_side_type(&mut self, op: &str, lhs: &Expr, rhs: &Expr, want: Option<&Ty>) -> Option<Ty> {
        let is_cmp = matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=");
        if is_const_int_expr(lhs) && !is_const_int_expr(rhs) && op != "<<" && op != ">>" && (is_cmp || !want.is_some_and(|w| w.is_int())) {
            let snap = self.snapshot();
            let r = self.expr(rhs, None);
            self.restore(snap);
            if let Ok(t) = r {
                if t.is_int() {
                    return Some(t);
                }
            }
        }
        None
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
                        let res = self.core_for(rhs, op, &lt, &rt, is_cmp)?;
                        if let Some(t) = owned {
                            self.release_slot(t);
                            self.free(t);
                        }
                        return Ok(res);
                    }
                }
            }
        }
        let lit_ty = self.literal_side_type(op, lhs, rhs, want);
        let (lt, lown) = self.operand(lhs, if lit_ty.is_some() { lit_ty.as_ref() } else if is_cmp { None } else { want })?;
        // a literal or a plain local on the right needs no temporary: load it straight into rcx
        if self.opt && !(lown && self.is_rc(&lt)) {
            if let Some(rt) = self.simple_rhs(rhs, &lt) {
                return self.core_for(rhs, op, &lt, &rt, is_cmp);
            }
        }
        let t1 = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t1));
        let (rt, rown) = self.operand(rhs, Some(&lt))?;
        let t2 = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t2));
        self.e("mov rcx, rax");
        self.e(format!("mov rax, qword ptr [rbp - {}]", t1));
        let res = self.core_for(rhs, op, &lt, &rt, is_cmp)?;
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
                let t = if lt.is_int() { lt.clone() } else { Ty::int() };
                if !lit_fits(*n, false, &t) {
                    return None; // the general path reports the range error
                }
                self.e(format!("mov rcx, {}", imm64(*n as i64)));
                Some(t)
            }
            ExprKind::Char(c) => {
                self.e(format!("mov ecx, {}", *c as u32));
                Some(Ty::Char)
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
    /// `binary_core` for the operands of `lhs op rhs`: a literal divisor that is not zero needs
    /// no zero check.
    fn core_for(&mut self, rhs: &Expr, op: &str, lt: &Ty, rt: &Ty, is_cmp: bool) -> R<Ty> {
        self.div_nonzero = self.opt && matches!(&rhs.kind, ExprKind::Int(n) if *n != 0);
        let r = self.binary_core(op, lt, rt, is_cmp);
        self.div_nonzero = false;
        r
    }

    fn binary_core(&mut self, op: &str, lt: &Ty, rt: &Ty, is_cmp: bool) -> R<Ty> {
        if op == "+" && ((*lt == Ty::Text && *rt == Ty::Char) || (*lt == Ty::Char && *rt == Ty::Text)) {
            // a char joins a text as its UTF-8 bytes
            let (a, b, c) = (self.temp(), self.temp(), self.temp());
            self.e(format!("mov qword ptr [rbp - {}], rax", a));
            self.e(format!("mov qword ptr [rbp - {}], rcx", b));
            let (cs, other) = if *lt == Ty::Char { (a, b) } else { (b, a) };
            self.e(format!("mov rcx, qword ptr [rbp - {}]", cs));
            self.e("call rt_text_from_char");
            self.e(format!("mov qword ptr [rbp - {}], rax", cs));
            self.e(format!("mov rcx, qword ptr [rbp - {}]", a));
            self.e(format!("mov rdx, qword ptr [rbp - {}]", b));
            self.e("call rt_concat");
            self.e(format!("mov qword ptr [rbp - {}], rax", c));
            let _ = other;
            self.e(format!("mov rcx, qword ptr [rbp - {}]", cs));
            self.e("call rt_release");
            self.e(format!("mov rax, qword ptr [rbp - {}]", c));
            self.free(c);
            self.free(b);
            self.free(a);
            return Ok(Ty::Text);
        }
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
                "<" | "<=" | ">" | ">=" => {
                    self.emit_std(Cap::Ord, &Ty::Text)?;
                    self.e("test rax, rax");
                    let cc = match op {
                        "<" => "l",
                        "<=" => "le",
                        ">" => "g",
                        _ => "ge",
                    };
                    self.e(format!("set{} al", cc));
                    self.e("movzx eax, al");
                    return Ok(Ty::Bool);
                }
                _ => return self.err(format!("operator '{}' is not defined for text", op)),
            }
        }
        if is_cmp {
            if let Some(t) = self.std_operator(op, lt, rt)? {
                return Ok(t);
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
            if (matches!(lt, Ty::Rc(_) | Ty::Weak(_)) || matches!(rt, Ty::Rc(_) | Ty::Weak(_))) && !matches!(op, "==" | "!=") {
                return self.err(format!("operator '{}' is not defined for {}: references only compare with == and !=", op, if matches!(lt, Ty::Rc(_) | Ty::Weak(_)) { lt.show() } else { rt.show() }));
            }
            if !(compatible(lt, rt) || compatible(rt, lt)) {
                return self.err(format!("cannot compare {} with {}", lt.show(), rt.show()));
            }
            if let (Ty::Enum(a), Ty::Enum(_)) = (lt, rt) {
                if self.enum_payload.contains(a) {
                    return self.err("enums with data cannot be compared with '=='; use `match`");
                }
            }
            if (matches!(lt, Ty::Result(_) | Ty::Err) || matches!(rt, Ty::Result(_) | Ty::Err)) && *lt != Ty::Nil && *rt != Ty::Nil {
                return self.err("a result cannot be compared; use `match`, `try` or `??`");
            }
            let unsigned = cmp_unsigned(lt, rt);
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
            // a shift keeps the type of the value shifted; the count may be any integer
            (Ty::Int { .. }, Ty::Int { .. }) if op == "<<" || op == ">>" => lt.clone(),
            (Ty::Int { .. }, Ty::Int { .. }) => match wider(lt, rt) {
                Some(t) => t,
                None => {
                    return self.err(format!(
                        "operator '{}' is not defined for {} and {}; convert one side first (for example {}(x))",
                        op,
                        lt.show(),
                        rt.show(),
                        lt.show()
                    ))
                }
            },
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
                if self.target != Target::Bare && !self.div_nonzero {
                    self.at("rdx");
                    self.e("test rcx, rcx");
                    self.e("jz rt_div_zero");
                }
                if unsigned {
                    self.e("xor edx, edx");
                    self.e("div rcx");
                } else if self.div_nonzero {
                    self.e("cqo");
                    self.e("idiv rcx");
                } else {
                    // MIN / -1 wraps to MIN (and MIN % -1 is 0) instead of faulting
                    let (norm, done) = (self.lbl("divnorm"), self.lbl("divdone"));
                    self.e("cmp rcx, -1");
                    self.e(format!("jne {}", norm));
                    if op == "/" {
                        self.e("neg rax");
                    } else {
                        self.e("xor eax, eax");
                    }
                    self.e(format!("jmp {}", done));
                    self.label(&norm);
                    self.e("cqo");
                    self.e("idiv rcx");
                    if op == "%" {
                        self.e("mov rax, rdx");
                    }
                    self.label(&done);
                    self.norm(&ty);
                    return Ok(ty);
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
                let mk = self.modkey(&self.cur_mod, m);
                return self.member(&mk, name, want);
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
        let ot_rc = ot.clone();
        let ot = rc_view(&ot);
        let res = match &ot {
            Ty::Rec(r) => self.rec_field(&r.clone(), name, own)?,
            Ty::Ptr(p) if matches!(&**p, Ty::Rec(_)) => {
                let Ty::Rec(r) = &**p else { unreachable!() };
                let f = self.fld(r, name)?;
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
                Ty::int()
            }
            Ty::Map(..) if name == "len" => {
                self.e("mov rax, qword ptr [rax + 48]");
                Ty::int()
            }
            Ty::Char if name == "code" => Ty::int(),
            Ty::Err if name == "message" => {
                self.e("mov rax, qword ptr [rax]");
                self.retain();
                Ty::Text
            }
            Ty::Err if name == "kind" => {
                self.e("mov rax, qword ptr [rax + 8]");
                self.retain();
                Ty::Kind
            }
            Ty::Err if name == "cause" => {
                self.e("mov rax, qword ptr [rax + 16]");
                self.retain();
                Ty::Err
            }
            Ty::Err if name == "full_message" => {
                self.e("mov rcx, rax");
                self.e("call rt_error_full");
                Ty::Text
            }
            Ty::Kind if name == "name" => {
                self.retain();
                Ty::Text
            }
            _ => return self.err(format!("{} has no field '{}'", ot.show(), name)),
        };
        // property getters consume their receiver themselves
        let consumed = matches!(&ot, Ty::Rec(r) if self.field_of(r, name).is_none());
        let ot = ot_rc;
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
        if let Some((fty, off)) = self.fld(r, name)? {
            self.load_mem(&fty, &format!("rax + {}", off));
            self.retain_if(&fty);
            return Ok(fty);
        }
        if let Some(sig) = self.methods.get(&(r.to_string(), name.to_string())).cloned() {
            if sig.params.is_empty() && sig.ret != Ty::Void {
                self.check_method_vis(&sig, name)?;
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
        self.record_alloc(r, fields, false)
    }

    /// Offset of the weak-cell word of a shared object (after the fields).
    fn cell_off(&self, r: &str) -> u32 {
        (self.rec_size(r) + 7) & !7
    }

    /// `rc`: the object of a `new` (an extra word for the weak cell, dropped by `B_rcdrop_*`).
    fn record_alloc(&mut self, r: &str, fields: &[(String, Expr)], rc: bool) -> R<()> {
        let (size, drop) = if rc { (self.cell_off(r) + 8, format!("B_rcdrop_{}", sym_of(r))) } else { (self.rec_size(r), self.rec_drop(r)) };
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
                let is_char = matches!(&lit.kind, ExprKind::Char(_));
                if is_char != (*st == Ty::Char) || !(st.is_int() || *st == Ty::Bool || *st == Ty::Char) {
                    return self.err(format!("a {} pattern cannot match {}", if is_char { "char" } else { "number" }, st.show()));
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
            let by_ref = p.by_ref && !matches!(t, Ty::List(_) | Ty::Map(..));
            ptys.push((p.name.clone(), t, by_ref));
        }
        if let Some(wp) = &wparams {
            if wp.len() != params.len() {
                return self.err(format!("expected a function with {} parameter(s), found {}", wp.len(), params.len()));
            }
        }
        let own_refs: Vec<(String, bool)> =
            ptys.iter().filter(|p| p.2).map(|p| (p.0.clone(), plain_scalar(&p.1))).collect();
        self.check_refs(&own_refs, body)?;
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
            sc.vars.insert(n.clone(), Local { off, ty: l.ty.clone(), mutable: false, captured: true, reg: None, imm: None, is_ref: false, ind: false });
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

    /// A call of a C function on Linux: the System V convention. Integers and pointers go to
    /// rdi, rsi, rdx, rcx, r8, r9, floats to xmm0-7 (counted separately), the rest to the stack
    /// from [rsp]; `al` holds the number of vector registers used (for variadic functions). The
    /// function is reached through its slot in the import table filled in by the dynamic loader.
    fn sysv_call(&mut self, temps: &[i32], floats: &[(usize, u8)], sym: &str) {
        const IREGS: [&str; 6] = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        self.fs.max_args = self.fs.max_args.max(temps.len());
        let (mut ni, mut nf, mut ns) = (0, 0, 0);
        let mut moves: Vec<String> = Vec::new();
        for (i, t) in temps.iter().enumerate() {
            let float = floats.iter().find(|(p, _)| *p == i).map(|(_, b)| *b);
            match float {
                Some(bits) if nf < 8 => {
                    moves.push(format!("mov rax, qword ptr [rbp - {}]", t));
                    moves.push(if bits == 32 { format!("movd xmm{}, eax", nf) } else { format!("movq xmm{}, rax", nf) });
                    nf += 1;
                }
                None if ni < 6 => {
                    moves.push(format!("mov {}, qword ptr [rbp - {}]", IREGS[ni], t));
                    ni += 1;
                }
                _ => {
                    moves.push(format!("mov rax, qword ptr [rbp - {}]", t));
                    moves.push(format!("mov qword ptr [rsp + {}], rax", 8 * ns));
                    ns += 1;
                }
            }
        }
        for m in moves {
            self.e(m);
        }
        self.e(format!("mov eax, {}", nf));
        self.e(format!("call qword ptr GOT_{}", sym));
    }

    fn call(&mut self, callee: &Expr, args: &[Arg], targs: &[Type], want: Option<&Ty>) -> R<Ty> {
        match &callee.kind {
            // test hook: lowers the element ceiling of lists, maps and texts so a test can reach it
            ExprKind::Ident(name) if self.lookup(name).is_none() && name == "__set_max_len" && args.len() == 1 => {
                let at = self.expr(&args[0].value, Some(&Ty::i64()))?;
                if !at.is_int() {
                    return self.err("__set_max_len() takes a number");
                }
                self.e("mov qword ptr rt_max_len, rax");
                Ok(Ty::Void)
            }
            ExprKind::Ident(name) if self.lookup(name).is_none() && matches!(name.as_str(), "__eq" | "__cmp" | "__hash" | "__tag" | "__payload" | "__mix") => {
                self.std_intrinsic(name, args)
            }
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
                        // truncate toward zero; NaN, infinities and out-of-range values abort
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
                        self.at("rdx");
                        self.e("call rt_abort");
                        self.label(&ok);
                        if t == Ty::Bool {
                            return self.err("cannot convert float to bool");
                        }
                        if let (Ty::Int { bits, .. }, Some(_)) = (&t, norm_insn(&t)) {
                            // a narrower target: the truncated value must be the converted one
                            if *bits < 64 {
                                let fits = self.lbl("f2ifit");
                                self.e("mov rdx, rax");
                                self.norm(&t);
                                self.e("cmp rax, rdx");
                                self.e(format!("je {}", fits));
                                self.e("lea rcx, rt_msg_floatrange");
                                self.at("rdx");
                                self.e("call rt_abort");
                                self.label(&fits);
                            }
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
                if (name == "__abort" || name == "abort" && !self.funcs.contains_key(&(self.cur_mod.clone(), name.clone()))) && args.len() == 1 {
                    let t = self.expr(&args[0].value, Some(&Ty::Text))?;
                    self.check(&Ty::Text, &t, "abort message")?;
                    self.e("mov rcx, rax");
                    self.at("rdx");
                    self.e("call rt_abort");
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
                if name == "__kind" && args.len() == 1 {
                    // the interned kind for a name: the one way a kind is made at run time
                    let t = self.expr(&args[0].value, Some(&Ty::Text))?;
                    self.check(&Ty::Text, &t, "__kind")?;
                    let keep = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                    self.e("mov rcx, rax");
                    self.e("call rt_kind_intern");
                    let res = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", res));
                    self.release_slot(keep);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", res));
                    self.free(res);
                    self.free(keep);
                    return Ok(Ty::Kind);
                }
                if name == "error" && (1..=3).contains(&args.len()) {
                    let (k, m, c) = (self.temp(), self.temp(), self.temp());
                    if args.len() == 1 {
                        let t = self.expr(&args[0].value, Some(&Ty::Text))?;
                        self.check(&Ty::Text, &t, "error message")?;
                        self.e(format!("mov qword ptr [rbp - {}], rax", m));
                        self.e("call rt_kind_other");
                        self.e(format!("mov qword ptr [rbp - {}], rax", k));
                    } else {
                        let kt = self.expr(&args[0].value, Some(&Ty::Kind))?;
                        if kt == Ty::Text {
                            return self.err("error(kind, message): expected an ErrorKind, found text; make a kind with errors.kind(\"package.Name\")");
                        }
                        self.check(&Ty::Kind, &kt, "error kind")?;
                        self.e(format!("mov qword ptr [rbp - {}], rax", k));
                        let t = self.expr(&args[1].value, Some(&Ty::Text))?;
                        self.check(&Ty::Text, &t, "error message")?;
                        self.e(format!("mov qword ptr [rbp - {}], rax", m));
                    }
                    if args.len() == 3 {
                        let ct = self.expr(&args[2].value, Some(&Ty::Err))?;
                        self.check(&Ty::Err, &ct, "error cause")?;
                        self.e(format!("mov qword ptr [rbp - {}], rax", c));
                    } else {
                        self.e(format!("mov qword ptr [rbp - {}], 0", c));
                    }
                    self.alloc(24, "rt_drop_error");
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", m));
                    self.e("mov qword ptr [rax], rcx");
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", k));
                    self.e("mov qword ptr [rax + 8], rcx");
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", c));
                    self.e("mov qword ptr [rax + 16], rcx");
                    self.free(c);
                    self.free(m);
                    self.free(k);
                    return Ok(Ty::Err);
                }
                if name == "char" && args.len() == 1 && !self.funcs.contains_key(&(self.cur_mod.clone(), name.clone())) {
                    let t = self.expr(&args[0].value, None)?;
                    if !t.is_int() {
                        return self.err(format!("char() needs an integer code point, found {}", t.show()));
                    }
                    let (bad, ok) = (self.lbl("charbad"), self.lbl("charok"));
                    self.e("cmp rax, 1114111");
                    self.e(format!("ja {}", bad));
                    self.e("lea rcx, [rax - 55296]");
                    self.e("cmp rcx, 2047");
                    self.e(format!("ja {}", ok));
                    self.label(&bad);
                    self.e("mov rdx, rax");
                    self.e("lea rcx, rt_msg_char1");
                    self.e("lea r8, rt_msg_char2");
                    self.at("r9");
                    self.e("call rt_abort_val");
                    self.label(&ok);
                    self.e("mov eax, eax");
                    return Ok(Ty::Char);
                }
                if name == "heap_bytes" && args.is_empty() {
                    if self.custom_allocator.is_some() {
                        return self.err("heap_bytes() needs the built-in heap");
                    }
                    if self.target != Target::Bare {
                        return self.err("heap_bytes() is only available in a bare-metal kernel");
                    }
                    self.e("mov rax, qword ptr rt_brk");
                    self.e("sub rax, 2097152");
                    return Ok(Ty::i64());
                }
                if name == "weak" && !self.funcs.contains_key(&(self.cur_mod.clone(), name.clone())) {
                    return self.weak_of(args);
                }
                if name == "live_objects" && args.is_empty() {
                    self.e("mov rax, qword ptr rt_live");
                    return Ok(Ty::int());
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
                    return self.call_generic(&key.0, &fd, line, args, targs, want);
                }
                if let Some((q, idx)) = self.variants.get(&key).cloned() {
                    return self.enum_construct(&q, idx, args);
                }
                if self.globals.contains_key(&key) {
                    let t = self.expr(callee, None)?;
                    return self.call_value(t, args);
                }
                if name == "chr" {
                    return self.err("unknown function chr: use char(n).to_text() to make a text from a code point");
                }
                self.err(format!("undefined function '{}'", name))
            }
            ExprKind::Field { expr: obj, name, .. } => {
                if let ExprKind::Ident(m) = &obj.kind {
                    if self.is_module(m) {
                        if m == "c" {
                            return self.c_intrinsic(name, args);
                        }
                        let mk = self.modkey(&self.cur_mod, m);
                        let m = &mk;
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
                            self.check_member_vis(m, name, fd.public)?;
                            return self.call_generic(&key.0, &fd, line, args, targs, want);
                        }
                        return match self.funcs.get(&key).cloned() {
                            Some(sig) => {
                                self.check_member_vis(m, name, sig.public)?;
                                self.call_sig(&sig, None, args)
                            }
                            None if self.modules.iter().any(|x| &x.name == m) => {
                                if let Some((q, idx)) = self.variants.get(&key).cloned() {
                                    self.check_member_vis(m, name, self.pub_types.contains(&q))?;
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
                if let Some(pt) = self.path_ty(obj) {
                    if let Some(res) = self.cow_method(obj, &pt, name, args)? {
                        return Ok(res);
                    }
                }
                let ot = self.expr(obj, None)?;
                if let Ty::Weak(inner) = &ot {
                    if name == "upgrade" {
                        return self.weak_upgrade(inner, args);
                    }
                    return self.err(format!("{} has no method '{}'; use upgrade()", ot.show(), name));
                }
                let ot = rc_view(&ot);
                if let Ty::Param { name: tname, bound } = &ot {
                    return self.param_method_call("", bound, tname, name, args);
                }
                if !matches!(ot, Ty::Rec(_)) {
                    let mut found = self.methods.get(&(method_key(&ot), name.clone())).cloned();
                    if let Some(sg) = &found {
                        self.check_method_vis(sg, name)?;
                    }
                    if found.is_none() {
                        found = self.std_sig(&ot, name)?;
                    }
                    if let Some(sig) = found {
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
                    let mut found = self.methods.get(&(r.clone(), name.clone())).cloned();
                    if let Some(sg) = &found {
                        self.check_method_vis(sg, name)?;
                    }
                    if found.is_none() {
                        found = self.std_sig(&ot, name)?;
                    }
                    if let Some(sig) = found {
                        // the method owns its receiver, which expr() handed over
                        let t = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", t));
                        let res = self.call_sig(&sig, Some(t), args);
                        self.free(t);
                        return res;
                    }
                    if let Some((fty @ Ty::Func { .. }, off)) = self.fld(&r, name)? {
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
                if ot == Ty::Char && matches!(name.as_str(), "to_text" | "len_utf8" | "is_ascii") {
                    if !args.is_empty() {
                        return self.err(format!("{}() takes no arguments", name));
                    }
                    match name.as_str() {
                        "to_text" => {
                            self.e("mov rcx, rax");
                            self.e("call rt_text_from_char");
                            return Ok(Ty::Text);
                        }
                        "len_utf8" => {
                            self.e("mov ecx, eax");
                            self.e("mov eax, 1");
                            for (lim, n) in [(128, 2), (2048, 3), (65536, 4)] {
                                self.e(format!("mov edx, {}", n));
                                self.e(format!("cmp ecx, {}", lim));
                                self.e("cmovae eax, edx");
                            }
                            return Ok(Ty::int());
                        }
                        _ => {
                            self.e("cmp eax, 128");
                            self.e("setb al");
                            self.e("movzx eax, al");
                            return Ok(Ty::Bool);
                        }
                    }
                }
                if ot == Ty::Text && matches!(name.as_str(), "bytes" | "char_count") {
                    if !args.is_empty() {
                        return self.err(format!("{}() takes no arguments", name));
                    }
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    self.e("mov rcx, rax");
                    self.e(format!("call {}", if name == "bytes" { "rt_text_bytes" } else { "rt_text_char_count" }));
                    let keep = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                    self.release_slot(t);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
                    self.free(keep);
                    self.free(t);
                    return Ok(if name == "bytes" { Ty::List(Box::new(Ty::Int { bits: 8, signed: false })) } else { Ty::int() });
                }
                if ot == Ty::Text && name == "byte" {
                    if args.len() != 1 {
                        return self.err("byte(i) takes one position");
                    }
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    let it = self.expr(&args[0].value, Some(&Ty::i64()))?;
                    if !it.is_int() {
                        return self.err(format!("byte position must be a number, found {}", it.show()));
                    }
                    self.e("mov rdx, rax");
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                    self.at("r9");
                    self.e("call rt_text_byte_at");
                    let keep = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                    self.release_slot(t);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
                    self.free(keep);
                    self.free(t);
                    return Ok(Ty::Int { bits: 8, signed: false });
                }
                if ot == Ty::Text && (name == "slice" || name == "__raw_slice") {
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
                    self.at("r9");
                    self.e(if name == "slice" { "call rt_text_slice" } else { "call rt_text_slice_raw" });
                    let keep = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                    self.release_slot(t);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
                    self.free(keep);
                    self.free(a);
                    self.free(t);
                    return Ok(Ty::Text);
                }
                if ot == Ty::Err && (name == "context" || name == "is") {
                    if args.len() != 1 {
                        return self.err(format!("'{}' takes one argument", name));
                    }
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    if name == "context" {
                        // a new error with the kind of this one; this one becomes its cause
                        let mt = self.expr(&args[0].value, Some(&Ty::Text))?;
                        self.check(&Ty::Text, &mt, "context message")?;
                        let m = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", m));
                        self.alloc(24, "rt_drop_error");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", m));
                        self.e("mov qword ptr [rax], rcx");
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", t));
                        self.e("mov rcx, qword ptr [rdx + 8]");
                        self.e("mov qword ptr [rax + 8], rcx");
                        self.e("call rt_retain");
                        self.e("mov qword ptr [rax + 16], rdx");
                        self.free(m);
                        self.free(t);
                        return Ok(Ty::Err);
                    }
                    let kt = self.expr(&args[0].value, Some(&Ty::Kind))?;
                    if kt == Ty::Text {
                        return self.err("is(kind): expected an ErrorKind, found text; make a kind with errors.kind(\"package.Name\")");
                    }
                    self.check(&Ty::Kind, &kt, "is")?;
                    let k = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", k));
                    self.e("mov rdx, rax");
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                    self.e("call rt_error_is");
                    self.release_slot(k);
                    self.release_slot(t);
                    self.free(k);
                    self.free(t);
                    return Ok(Ty::Bool);
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
                        if name == "get" {
                            // the element at an index, or nothing when there is none (negative
                            // included): `T?` for a type that can be nil, `T or error` otherwise
                            if args.len() != 1 || args[0].name.is_some() {
                                return self.err("'get' takes one index");
                            }
                            let it = self.expr(&args[0].value, Some(&Ty::i64()))?;
                            if !it.is_int() {
                                return self.err(format!("list index must be a number, found {}", it.show()));
                            }
                            let (miss, done) = (self.lbl("getmiss"), self.lbl("getdone"));
                            self.e("mov rdx, rax");
                            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                            self.e("cmp rdx, qword ptr [rcx]");
                            self.e(format!("jae {}", miss));
                            self.e("mov rax, qword ptr [rcx + 16]");
                            self.e("mov rax, qword ptr [rax + rdx * 8]");
                            self.retain_if(el);
                            if el.nullable() {
                                self.e(format!("jmp {}", done));
                                self.label(&miss);
                                self.e("xor eax, eax");
                                self.label(&done);
                                return Ok((**el).clone());
                            }
                            self.mk_ok(el);
                            self.e(format!("jmp {}", done));
                            self.label(&miss);
                            let at = self.line;
                            let mk = |kind: ExprKind| Expr { kind, line: at };
                            let lit = |s: &str| Arg { name: None, by_ref: false, spread: false, value: mk(ExprKind::Text(vec![TextPart::Lit(s.into())])) };
                            let kind = mk(ExprKind::Call { callee: Box::new(mk(ExprKind::Ident("__kind".into()))), args: vec![lit("std.InvalidInput")], targs: Vec::new() });
                            let failed = mk(ExprKind::Call {
                                callee: Box::new(mk(ExprKind::Ident("error".into()))),
                                args: vec![Arg { name: None, by_ref: false, spread: false, value: kind }, lit("index out of bounds")],
                                targs: Vec::new(),
                            });
                            self.expr(&failed, None)?;
                            self.mk_err(el);
                            self.label(&done);
                            return Ok(Ty::Result(el.clone()));
                        }
                        if name == "take" {
                            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                            self.at("rdx");
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
        if self.is_rc(k) {
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
                    if p.by_ref && matches!(p.ty, Ty::List(_) | Ty::Map(..)) {
                        // the callee gets the address of the caller's own slot and writes through it
                        if self.path_ty(x).is_some() {
                            self.write_place(x)?
                        } else {
                            let vt = self.expr(x, Some(&p.ty))?;
                            self.temp_place();
                            vt
                        }
                    } else if p.by_ref {
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
        if sig.is_extern && self.target == Target::Linux {
            self.sysv_call(&temps, &float_args, &sig.sym);
        } else {
        for (idx, bits) in float_args.iter().filter(|(i, _)| *i < 4) {
            let reg = ARG_REGS[*idx];
            self.e(format!("mov {}, qword ptr [rbp - {}]", reg, temps[*idx]));
            if *bits == 32 {
                self.e(format!("movd xmm{}, {}", idx, ["ecx", "edx", "r8d", "r9d"][*idx]));
            } else {
                self.e(format!("movq xmm{}, {}", idx, reg));
            }
        }
        let target = if sig.is_extern { format!("call qword ptr GOT_{}", sig.sym) } else { format!("call {}", sig.sym) };
        self.emit_call(&temps, &target, None);
        }
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
                Some(Local { off: 0, ty: p.ty.clone(), mutable: false, captured: false, reg: None, imm: Some(v), is_ref: false, ind: false })
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
                    Local { off: t, ty: p.ty.clone(), mutable: false, captured: false, reg: None, imm: None, is_ref: false, ind: false }
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
            "package t\nfunc f()\n    let i = future.timeout_wait({}, {})\n    if i == 1\n{}    end\n    return future.timed_out()\nend\n",
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
        Ty::Rc(e) => format!("Rc<{}>", ty_full(e)),
        Ty::Weak(e) => format!("Weak<{}>", ty_full(e)),
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
        Type::Optional(i) | Type::Shared(i) | Type::Pointer(i) | Type::Slice(i) | Type::Array(_, i) => mentions_param(i, names),
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
        let q = self.qname(m, path);
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
            derived: self.derived.clone(),
            map_thunks: self.map_thunks.clone(),
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
        self.derived = s.derived;
        self.map_thunks = s.map_thunks;
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
        } else if bound.len() == 1 && matches!(bound[0].as_str(), "Eq" | "Ord" | "Hash") {
            Ok(format!("std.{}", bound[0]))
        } else {
            self.err(format!("unknown interface '{}'", bound.join(".")))
        }
    }

    /// Parameter and result types of the interface method `sg`; `Self` is `self_ty`.
    fn interface_method(&mut self, iface_mod: &str, sg: &MethodSig, self_ty: &Ty) -> R<(Vec<Ty>, Ty)> {
        let mut b = HashMap::new();
        b.insert("Self".to_string(), self_ty.clone());
        let saved = std::mem::replace(&mut self.bindings, b);
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

    /// The method `mname` a bound provides: its own, or `eq` when the bound is `Ord` or `Hash`.
    fn bound_sig(&self, bound: &str, mname: &str) -> Option<(String, MethodSig)> {
        let (bm, bsigs) = self.interfaces.get(bound)?;
        if let Some(x) = bsigs.iter().find(|x| x.name == mname) {
            return Some((bm.clone(), x.clone()));
        }
        if mname == "eq" && matches!(bound, "std.Ord" | "std.Hash") {
            let (em, esigs) = self.interfaces.get("std.Eq")?;
            return esigs.iter().find(|x| x.name == "eq").map(|x| (em.clone(), x.clone()));
        }
        None
    }

    /// Checks that `ty` has every method of the interface `iface`, with the same signature.
    fn satisfies(&mut self, ty: &Ty, iface: &str) -> R<()> {
        let (im, sigs) = self.interfaces[iface].clone();
        let iname = display_name(iface);
        let standard = matches!(iface, "std.Eq" | "std.Ord" | "std.Hash");
        for sg in &sigs {
            let (want_params, want_ret) = self.interface_method(&im, sg, ty)?;
            let have: Option<(Vec<Ty>, Ty)> = match ty {
                Ty::Param { bound: Some(b), .. } => match self.bound_sig(b, &sg.name) {
                    Some((bm, x)) => Some(self.interface_method(&bm, &x, ty)?),
                    None => None,
                },
                Ty::Param { .. } => None,
                _ => {
                    let found = self.methods.get(&(method_key(ty), sg.name.clone())).cloned();
                    if let Some(m) = &found {
                        if !standard && !m.public && m.module != im {
                            return self.private_name(&sg.name, &m.module);
                        }
                    }
                    let found = match found {
                        Some(m) => Some(m),
                        None => self.std_sig(ty, &sg.name)?,
                    };
                    found.map(|m| (m.params.iter().map(|p| p.ty.clone()).collect(), m.ret.clone()))
                }
            };
            let Some((have_params, have_ret)) = have else {
                if standard {
                    let cap = Cap::of(&sg.name);
                    let why = self.lacks(ty, cap).map(|(_, w)| format!(": {}", w)).unwrap_or_default();
                    return self.err(format!("{} does not satisfy {}{}", ty.show(), iname, why));
                }
                return self.err(format!("{} does not satisfy {}: it has no method '{}'", ty.show(), iname, sg.name));
            };
            if have_params != want_params {
                let show = |v: &[Ty]| v.iter().map(|t| t.show()).collect::<Vec<_>>().join(", ");
                return self.err(format!(
                    "{} does not satisfy {}: method '{}' {}takes ({}) where {} needs ({})",
                    ty.show(),
                    iname,
                    sg.name,
                    if standard { "has the wrong signature for the interface: it " } else { "" },
                    show(&have_params),
                    iname,
                    show(&want_params)
                ));
            }
            if have_ret != want_ret {
                let r = |t: &Ty| if *t == Ty::Void { "nothing".to_string() } else { t.show() };
                return self.err(format!(
                    "{} does not satisfy {}: method '{}' {}returns {} where {} needs {}",
                    ty.show(),
                    iname,
                    sg.name,
                    if standard { "has the wrong signature for the interface: it " } else { "" },
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
        let Some((im, sg)) = self.bound_sig(b, mname) else {
            return self.err(format!("{} has no method '{}': its bound {} does not list it", tname, mname, display_name(b)));
        };
        let _ = name;
        let self_ty = Ty::Param { name: tname.to_string(), bound: bound.clone() };
        let (ps, ret) = self.interface_method(&im, &sg, &self_ty)?;
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
        let q = self.qname(&m, path);
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
            Type::Shared(inner) => return self.unify(inner, actual, params, b, m),
            Type::Named { path, args } => {
                if path.len() == 1 && args.is_empty() && params.contains(&path[0]) {
                    match b.get(&path[0]) {
                        Some(prev) if prev != actual => {
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
                        ("Rc", Ty::Rc(el)) | ("Weak", Ty::Weak(el)) if args.len() == 1 => return self.unify(&args[0], el, params, b, m),
                        _ => {}
                    }
                }
                let q = self.qname(m, path);
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
    fn call_generic(&mut self, module: &str, fd: &FuncDecl, decl_line: u32, args: &[Arg], targs: &[Type], want: Option<&Ty>) -> R<Ty> {
        let names: Vec<String> = fd.generics.iter().map(|g| g.name.clone()).collect();
        let mut b: HashMap<String, Ty> = HashMap::new();
        if targs.len() > names.len() {
            return self.err(format!("'{}' takes {} type argument(s), but {} were given", fd.name, names.len(), targs.len()));
        }
        let caller = self.cur_mod.clone();
        for (n, t) in names.iter().zip(targs) {
            let ty = self.resolve(t, &caller)?;
            b.insert(n.clone(), ty);
        }
        let mut next = 0;
        let mut lambdas: Vec<(Type, &Expr)> = Vec::new();
        let mut late_lits: Vec<(Type, &Expr)> = Vec::new();
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
            // a bare number literal adapts to a type another argument fixed: it only binds an open parameter
            if is_int_literal(&a.value) {
                if let Type::Named { path, args: pargs } = pt {
                    if path.len() == 1 && pargs.is_empty() && b.contains_key(&path[0]) {
                        continue;
                    }
                }
                late_lits.push((pt.clone(), &a.value));
                continue;
            }
            // type the argument by compiling it into a scratch buffer
            let snap = self.snapshot();
            let r = self.expr(&a.value, None);
            self.restore(snap);
            let aty = r?;
            self.unify(pt, &aty, &names, &mut b, module)?;
        }
        for (pt, lit) in late_lits {
            if let Type::Named { path, args: pargs } = &pt {
                if path.len() == 1 && pargs.is_empty() && b.contains_key(&path[0]) {
                    continue;
                }
            }
            let aty = self.expr(lit, None)?;
            self.unify(&pt, &aty, &names, &mut b, module)?;
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
            ExprKind::Int(n) => lit_fits(*n, false, t).then(|| Const::Int(*n as i64)),
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
                    let i64t = Ty::int();
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
                    let lit = self.literal_side_type(op, lhs, rhs, None);
                    let (lt, lown) = self.operand(lhs, lit.as_ref())?;
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
        let intlike = |t: &Ty| t.is_int() || matches!(t, Ty::Char | Ty::Bool | Ty::Ptr(_));
        if intlike(lt) && intlike(rt) && (compatible(lt, rt) || compatible(rt, lt)) {
            let unsigned = cmp_unsigned(lt, rt);
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
            // sign-extending twice in a row is the same as once
            if (t == "movsxd rax, eax" || t == "movsx rax, al" || t == "movsx rax, ax") && out.last().is_some_and(|p| p.trim() == t) {
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
        Ty::Int { bits: 32, signed: false } | Ty::Char => "mov eax, eax",
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

fn fd_has_await(fd: &FuncDecl) -> bool {
    let mut names = Vec::new();
    collect_block(&fd.body, &mut names);
    names.iter().any(|n| n == "$await")
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

/// Whether a file uses `shared` or `spawn_thread` (the runtime then counts references atomically).
fn file_uses_threads(f: &File) -> (bool, bool) {
    let mut names = Vec::new();
    for d in &f.decls {
        if let DeclKind::Func(fd) = &d.kind {
            collect_block(&fd.body, &mut names);
        }
    }
    let spawn = names.iter().any(|n| n == "$spawn_thread");
    (spawn || names.iter().any(|n| n == "$shared"), spawn)
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
        ExprKind::Shared(expr) => {
            out.push("$shared".into());
            collect_expr(expr, out)
        }
        ExprKind::SpawnThread(expr) => {
            out.push("$spawn_thread".into());
            collect_expr(expr, out)
        }
        ExprKind::New(expr) => collect_expr(expr, out),
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
        ExprKind::Call { callee, args, .. } => {
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
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rcx, qword ptr [rcx]
    call rt_release
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 8]
    call rt_release
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    call rt_release
    mov rsp, rbp
    pop rbp
    ret
rt_drop_error endp

; rax = the interned kind named by text rcx, +1 reference for the caller (rcx is not consumed).
; Equal names give the same object, so kinds compare by address.
rt_kind_intern proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rax, qword ptr rt_kinds
rt_ki_loop:
    test rax, rax
    jz rt_ki_new
    mov qword ptr [rbp - 16], rax
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rax + 8]
    call rt_text_eq
    test rax, rax
    jnz rt_ki_found
    mov rax, qword ptr [rbp - 16]
    mov rax, qword ptr [rax]
    jmp rt_ki_loop
rt_ki_found:
    mov rax, qword ptr [rbp - 16]
    mov rax, qword ptr [rax + 8]
    mov rcx, rax
    call rt_retain
    mov rsp, rbp
    pop rbp
    ret
rt_ki_new:
    mov ecx, 16
    xor edx, edx
    call rt_alloc
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rax + 8], rcx
    mov rdx, qword ptr rt_kinds
    mov qword ptr [rax], rdx
    mov qword ptr rt_kinds, rax
    call rt_retain
    call rt_retain
    mov rax, qword ptr [rbp - 8]
    mov rsp, rbp
    pop rbp
    ret
rt_kind_intern endp

; rax = the kind `std.Other` (+1 reference): the kind of error("message"). It is the first
; entry of the kind table, a static node, so it costs no allocation.
rt_kind_other proc
    lea rax, rt_kind_other_name
    mov rcx, rax
    call rt_retain
    ret
rt_kind_other endp

; rax = 1 when the error rcx or one of its causes has kind rdx
rt_error_is proc
rt_eis_loop:
    test rcx, rcx
    jz rt_eis_no
    cmp qword ptr [rcx + 8], rdx
    je rt_eis_yes
    mov rcx, qword ptr [rcx + 16]
    jmp rt_eis_loop
rt_eis_yes:
    mov eax, 1
    ret
rt_eis_no:
    xor eax, eax
    ret
rt_error_is endp

; rax = new text: the messages of error rcx and its causes, joined with ": "
rt_error_full proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rcx, qword ptr [rcx + 16]
    test rcx, rcx
    jnz rt_ef_chain
    mov rax, qword ptr [rbp - 8]
    mov rax, qword ptr [rax]
    mov rcx, rax
    call rt_retain
    mov rsp, rbp
    pop rbp
    ret
rt_ef_chain:
    call rt_error_full
    mov qword ptr [rbp - 16], rax
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx]
    lea rdx, rt_msg_sep
    call rt_concat
    mov qword ptr [rbp - 24], rax
    mov rcx, rax
    mov rdx, qword ptr [rbp - 16]
    call rt_concat
    mov qword ptr [rbp - 32], rax
    mov rcx, qword ptr [rbp - 24]
    call rt_release
    mov rcx, qword ptr [rbp - 16]
    call rt_release
    mov rax, qword ptr [rbp - 32]
    mov rsp, rbp
    pop rbp
    ret
rt_error_full endp

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

; write "abort: <location rdx>: <message rcx>" to standard error and exit with code 101
; (rdx = 0 when there is no location). Nothing else runs: no defer, no drop, no flush.
rt_abort proc
    and rsp, -16
    sub rsp, 48
    mov qword ptr [rsp + 32], rcx
    mov qword ptr [rsp + 40], rdx
    lea rcx, rt_msg_abort
    call rt_write_err
    mov rdx, qword ptr [rsp + 40]
    test rdx, rdx
    jz rt_abort_msg
    mov rcx, rdx
    call rt_write_err
    lea rcx, rt_msg_sep
    call rt_write_err
rt_abort_msg:
    mov rcx, qword ptr [rsp + 32]
    call rt_write_err
    lea rcx, rt_msg_nl
    call rt_write_err
    mov ecx, 101
    call rt_exit
rt_abort endp

; rax = new text with room for rcx bytes (zeroed, length set)
rt_text_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    cmp rcx, qword ptr rt_max_len
    ja rt_too_long
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

; rax = byte rdx of text rcx (r9 = location); aborts out of range
rt_text_byte_at proc
    cmp rdx, qword ptr [rcx]
    jae rt_tba_oob
    movzx eax, byte ptr [rcx + rdx + 8]
    ret
rt_tba_oob:
    lea rcx, rt_msg_bytepos
    lea r8, rt_msg_outofrange
    jmp rt_abort_val
rt_text_byte_at endp

; abort "<text rcx><number rdx><text r8>" at location r9
rt_abort_val proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], r8
    mov qword ptr [rbp - 24], r9
    mov rcx, rdx
    call rt_text_from_int
    mov rdx, rax
    mov rcx, qword ptr [rbp - 8]
    call rt_concat
    mov rcx, rax
    mov rdx, qword ptr [rbp - 16]
    call rt_concat
    mov rcx, rax
    mov rdx, qword ptr [rbp - 24]
    jmp rt_abort
rt_abort_val endp

; rax = new text: bytes [rdx, r8) of text rcx (r9 = location); aborts when a position is out of
; range, when rdx > r8 or when one is inside a multibyte character
rt_text_slice proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov qword ptr [rbp - 24], r8
    mov qword ptr [rbp - 48], r9
    mov rax, qword ptr [rcx]
    cmp rdx, rax
    ja rt_ts_oob1
    cmp r8, rax
    ja rt_ts_oob2
    cmp rdx, r8
    ja rt_ts_order
    cmp rdx, rax
    jae rt_ts_b1
    movzx r10d, byte ptr [rcx + rdx + 8]
    and r10d, 192
    cmp r10d, 128
    je rt_ts_cut1
rt_ts_b1:
    cmp r8, rax
    jae rt_ts_ok
    movzx r10d, byte ptr [rcx + r8 + 8]
    and r10d, 192
    cmp r10d, 128
    je rt_ts_cut2
rt_ts_ok:
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
rt_ts_oob1:
    lea rcx, rt_msg_textpos
    lea r8, rt_msg_outofrange
    jmp rt_ts_fail
rt_ts_oob2:
    lea rcx, rt_msg_textpos
    lea r8, rt_msg_outofrange
    mov rdx, qword ptr [rbp - 24]
    jmp rt_ts_fail
rt_ts_order:
    lea rcx, rt_msg_slicestart
    lea r8, rt_msg_afterend
    jmp rt_ts_fail
rt_ts_cut1:
    lea rcx, rt_msg_textpos
    lea r8, rt_msg_notboundary
    jmp rt_ts_fail
rt_ts_cut2:
    lea rcx, rt_msg_textpos
    lea r8, rt_msg_notboundary
    mov rdx, qword ptr [rbp - 24]
rt_ts_fail:
    mov r9, qword ptr [rbp - 48]
    jmp rt_abort_val
rt_text_slice endp

; like rt_text_slice without the character-boundary check: for protocol and buffer code that
; cuts bytes, not characters (the result may be invalid UTF-8)
rt_text_slice_raw proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov qword ptr [rbp - 24], r8
    mov qword ptr [rbp - 48], r9
    mov rax, qword ptr [rcx]
    cmp rdx, rax
    ja rt_ts_oob1
    cmp r8, rax
    ja rt_ts_oob2
    cmp rdx, r8
    ja rt_ts_order
    jmp rt_ts_ok
rt_text_slice_raw endp

; rax = new text holding the UTF-8 bytes of code point rcx
rt_text_from_char proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov eax, 1
    cmp ecx, 128
    jb rt_tchr_len
    mov eax, 2
    cmp ecx, 2048
    jb rt_tchr_len
    mov eax, 3
    cmp ecx, 65536
    jb rt_tchr_len
    mov eax, 4
rt_tchr_len:
    mov qword ptr [rbp - 16], rax
    mov rcx, rax
    call rt_text_alloc
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rbp - 16]
    cmp rdx, 1
    jne rt_tchr_2
    mov byte ptr [rax + 8], cl
    jmp rt_tchr_done
rt_tchr_2:
    cmp rdx, 2
    jne rt_tchr_3
    mov r8d, ecx
    shr r8d, 6
    or r8d, 192
    mov byte ptr [rax + 8], r8b
    and ecx, 63
    or ecx, 128
    mov byte ptr [rax + 9], cl
    jmp rt_tchr_done
rt_tchr_3:
    cmp rdx, 3
    jne rt_tchr_4
    mov r8d, ecx
    shr r8d, 12
    or r8d, 224
    mov byte ptr [rax + 8], r8b
    mov r8d, ecx
    shr r8d, 6
    and r8d, 63
    or r8d, 128
    mov byte ptr [rax + 9], r8b
    and ecx, 63
    or ecx, 128
    mov byte ptr [rax + 10], cl
    jmp rt_tchr_done
rt_tchr_4:
    mov r8d, ecx
    shr r8d, 18
    or r8d, 240
    mov byte ptr [rax + 8], r8b
    mov r8d, ecx
    shr r8d, 12
    and r8d, 63
    or r8d, 128
    mov byte ptr [rax + 9], r8b
    mov r8d, ecx
    shr r8d, 6
    and r8d, 63
    or r8d, 128
    mov byte ptr [rax + 10], r8b
    and ecx, 63
    or ecx, 128
    mov byte ptr [rax + 11], cl
rt_tchr_done:
    mov rsp, rbp
    pop rbp
    ret
rt_text_from_char endp

; eax = the code point of text rcx at offset rdx (rdx < length), rdx = the offset after it.
; Bytes that are not valid UTF-8 decode as U+FFFD, one per byte. Clobbers rcx, r8-r11.
rt_text_next_char proc
    mov r8, qword ptr [rcx]
    lea r9, [rcx + rdx + 8]
    sub r8, rdx
    movzx eax, byte ptr [r9]
    cmp eax, 128
    jb rt_nc_1
    cmp eax, 194
    jb rt_nc_bad
    cmp eax, 224
    jb rt_nc_2
    cmp eax, 240
    jb rt_nc_3
    cmp eax, 244
    ja rt_nc_bad
    cmp r8, 4
    jb rt_nc_bad
    and eax, 7
    mov r10d, 4
    mov r11d, 65536
    jmp rt_nc_cont
rt_nc_3:
    cmp r8, 3
    jb rt_nc_bad
    and eax, 15
    mov r10d, 3
    mov r11d, 2048
    jmp rt_nc_cont
rt_nc_2:
    cmp r8, 2
    jb rt_nc_bad
    and eax, 31
    mov r10d, 2
    mov r11d, 128
rt_nc_cont:
    mov ecx, 1
rt_nc_loop:
    movzx r8d, byte ptr [r9 + rcx]
    xor r8d, 128
    test r8d, 192
    jnz rt_nc_bad
    shl eax, 6
    or eax, r8d
    inc rcx
    cmp rcx, r10
    jb rt_nc_loop
    cmp eax, r11d
    jb rt_nc_bad
    cmp eax, 1114111
    ja rt_nc_bad
    lea r8d, [rax - 55296]
    cmp r8d, 2047
    jbe rt_nc_bad
    add rdx, r10
    ret
rt_nc_bad:
    mov eax, 65533
rt_nc_1:
    inc rdx
    ret
rt_text_next_char endp

; rax = number of characters of text rcx (an invalid byte counts as one)
rt_text_char_count proc
    push rbp
    mov rbp, rsp
    push rsi
    push rdi
    sub rsp, 16
    mov rsi, rcx
    mov qword ptr [rbp - 24], 0
    xor edi, edi
rt_cc_loop:
    mov rdx, rdi
    mov rcx, rsi
    cmp rdx, qword ptr [rsi]
    jae rt_cc_done
    call rt_text_next_char
    mov rdi, rdx
    inc qword ptr [rbp - 24]
    jmp rt_cc_loop
rt_cc_done:
    mov rax, qword ptr [rbp - 24]
    add rsp, 16
    pop rdi
    pop rsi
    pop rbp
    ret
rt_text_char_count endp

; rax = new []byte holding the bytes of text rcx
rt_text_bytes proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    xor ecx, ecx
    call rt_list_new
    mov qword ptr [rbp - 16], rax
    mov qword ptr [rbp - 24], 0
rt_tb_loop:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 24]
    cmp rax, qword ptr [rcx]
    jae rt_tb_done
    movzx edx, byte ptr [rcx + rax + 8]
    mov rcx, qword ptr [rbp - 16]
    call rt_list_push
    inc qword ptr [rbp - 24]
    jmp rt_tb_loop
rt_tb_done:
    mov rax, qword ptr [rbp - 16]
    mov rsp, rbp
    pop rbp
    ret
rt_text_bytes endp

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
    cmp rax, qword ptr rt_max_len
    jae rt_too_long
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
    jmp rt_abort
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

; rax = a new list with the elements of list rcx (rdx = function that copies a heap element, or 0 to retain it)
rt_list_clone proc
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov rcx, qword ptr [rcx + 24]
    call rt_list_new
    mov qword ptr [rbp - 24], rax
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx]
    test rcx, rcx
    jz rt_lcl_done
    mov qword ptr [rbp - 32], rcx
    shl rcx, 3
    xor edx, edx
    call rt_alloc
    mov rcx, qword ptr [rbp - 24]
    mov qword ptr [rcx + 16], rax
    mov rdx, qword ptr [rbp - 32]
    mov qword ptr [rcx + 8], rdx
    mov qword ptr [rcx], rdx
    mov qword ptr [rbp - 40], 0
rt_lcl_loop:
    mov rax, qword ptr [rbp - 40]
    cmp rax, qword ptr [rbp - 32]
    jae rt_lcl_done
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    mov rcx, qword ptr [rcx + rax * 8]
    mov rdx, qword ptr [rbp - 24]
    cmp qword ptr [rdx + 24], 0
    je rt_lcl_store
    mov rax, qword ptr [rbp - 16]
    test rax, rax
    jz rt_lcl_retain
    call rax
    mov rcx, rax
    jmp rt_lcl_store
rt_lcl_retain:
    call rt_retain
rt_lcl_store:
    mov rax, qword ptr [rbp - 24]
    mov rax, qword ptr [rax + 16]
    mov rdx, qword ptr [rbp - 40]
    mov qword ptr [rax + rdx * 8], rcx
    inc qword ptr [rbp - 40]
    jmp rt_lcl_loop
rt_lcl_done:
    mov rax, qword ptr [rbp - 24]
    mov rsp, rbp
    pop rbp
    ret
rt_list_clone endp

; copy on write: rax = the list in the slot at rcx, copied into the slot first when someone else holds it
; (rdx = the element copy function for rt_list_clone)
rt_list_unique proc
    mov rax, qword ptr [rcx]
    test rax, rax
    jz rt_lu_ret
    cmp qword ptr [rax - 16], 1
    jbe rt_lu_ret
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rcx, rax
    call rt_list_clone
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rcx]
    mov qword ptr [rcx], rax
    mov rcx, rdx
    call rt_release
    mov rsp, rbp
    pop rbp
rt_lu_ret:
    ret
rt_list_unique endp

rt_index_oob proc
    lea rcx, rt_msg_oob
    mov rdx, r9
    jmp rt_abort
rt_index_oob endp

; a list, map or text would hold more than rt_max_len elements (2,147,483,647 bytes for a text)
rt_too_long proc
    lea rcx, rt_msg_toolong
    xor edx, edx
    jmp rt_abort
rt_too_long endp
"#;

/// Removes the `name proc` ... `name endp` block from runtime text.
fn strip_proc(rt: &str, name: &str) -> String {
    let start = format!("\n{} proc\n", name);
    let end = format!("\n{} endp\n", name);
    match (rt.find(&start), rt.find(&end)) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &rt[..a], &rt[b + end.len()..]),
        _ => rt.to_string(),
    }
}

/// The built-in heap procedures, left out when the program declares an allocator.
fn strip_heap(rt: &str) -> String {
    strip_proc(&strip_proc(rt, "rt_alloc"), "rt_free_obj")
}

/// The built-in heap's data, left out when the program declares an allocator.
fn strip_heap_data(data: &str) -> String {
    let mut out = String::new();
    let mut skip_table = false;
    for line in data.lines() {
        let t = line.trim();
        if skip_table {
            skip_table = false;
            if t.starts_with("times") {
                continue;
            }
        }
        if t.starts_with("rt_heap ") || t.starts_with("rt_brk ") || t.starts_with("rt_heap_end ") {
            continue;
        }
        if t == "rt_freelists:" {
            skip_table = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// `rt_alloc` and `rt_free_obj` on top of the program's `#[allocator]` functions (and `rt_realloc_obj`
/// when it declares `realloc`). The runtime keeps the header: [total size][refcount][drop], 24 bytes.
fn custom_runtime(c: &crate::noalloc::CustomAllocatorInfo) -> String {
    let mut s = RUNTIME_CUSTOM.replace("ALLOC_FN", &c.alloc_sym).replace("FREE_FN", &c.free_sym);
    if let Some(r) = &c.realloc_sym {
        s.push_str(&RUNTIME_CUSTOM_REALLOC.replace("REALLOC_FN", r));
    }
    s
}

const RUNTIME_CUSTOM: &str = r#"
; ---- the program's allocator: rt_alloc asks for payload + 24 bytes (rounded up to 16), aligned to 16 ----
; rax = zeroed heap object of rcx bytes; rdx = drop procedure (or 0).
rt_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    lea rax, [rcx + 39]
    and rax, -16
    mov qword ptr [rbp - 8], rax
    mov qword ptr [rbp - 16], rdx
    mov rcx, rax
    mov edx, 16
    call ALLOC_FN
    test rax, rax
    jz rt_alloc_fail
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
    xor edx, edx
    call rt_abort
rt_alloc endp

; frees the object whose payload is at rcx
rt_free_obj proc
    sub rsp, 40
    sub rcx, 24
    mov rdx, qword ptr [rcx]
    mov r8d, 16
    dec qword ptr rt_live
    call FREE_FN
    add rsp, 40
    ret
rt_free_obj endp
"#;

const RUNTIME_CUSTOM_REALLOC: &str = r#"
; rax = the object at payload rcx grown to rdx payload bytes (the new tail is zeroed); its refcount must be 1
rt_realloc_obj proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    lea rax, [rdx + 39]
    and rax, -16
    mov qword ptr [rbp - 8], rax
    sub rcx, 24
    mov rdx, qword ptr [rcx]
    mov qword ptr [rbp - 16], rdx
    mov r8, rax
    mov r9d, 16
    call REALLOC_FN
    test rax, rax
    jz rt_alloc_fail
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rbp - 16]
    mov qword ptr [rax], rcx
    lea r9, [rax + rdx]
    lea r10, [rax + rcx]
rt_realloc_zero:
    cmp r9, r10
    jae rt_realloc_done
    mov qword ptr [r9], 0
    add r9, 8
    jmp rt_realloc_zero
rt_realloc_done:
    add rax, 24
    mov rsp, rbp
    pop rbp
    ret
rt_realloc_obj endp
"#;

/// In `rt_list_push`: grow the element buffer with the program's `realloc` when it is not shared.
const RUNTIME_LIST_REALLOC: &str = "    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    test rcx, rcx
    jz rt_lp_slow
    cmp qword ptr [rcx - 16], 1
    jne rt_lp_slow
    mov rdx, qword ptr [rbp - 24]
    shl rdx, 3
    call rt_realloc_obj
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rcx + 16], rax
    mov rdx, qword ptr [rbp - 24]
    mov qword ptr [rcx + 8], rdx
    jmp rt_lp_store
rt_lp_slow:
    mov rax, qword ptr [rbp - 24]
";

const RUNTIME_MAP: &str = include_str!("runtime_map.asm");
const RUNTIME_DERIVE: &str = include_str!("runtime_derive.asm");

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
    call qword ptr GOT_GetProcessHeap
    mov qword ptr rt_heap, rax
rt_alloc_have:
    mov rcx, rax
    mov edx, 8
    mov r8, qword ptr [rbp - 8]
    add r8, 16
    call qword ptr GOT_HeapAlloc
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
    xor edx, edx
    call rt_abort
rt_alloc endp

; write rdx bytes at rcx to stdout
rt_write proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov ecx, -11
    call qword ptr GOT_GetStdHandle
    mov rcx, rax
    mov rdx, qword ptr [rbp - 8]
    mov r8, qword ptr [rbp - 16]
    lea r9, [rbp - 24]
    mov qword ptr [rsp + 32], 0
    call qword ptr GOT_WriteFile
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


; write text rcx to standard error
rt_write_err proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov rdx, qword ptr [rcx]
    add rcx, 8
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov ecx, -12
    call qword ptr GOT_GetStdHandle
    mov rcx, rax
    mov rdx, qword ptr [rbp - 8]
    mov r8, qword ptr [rbp - 16]
    lea r9, [rbp - 24]
    mov qword ptr [rsp + 32], 0
    call qword ptr GOT_WriteFile
    mov rsp, rbp
    pop rbp
    ret
rt_write_err endp

; frees the object whose payload is at rcx
rt_free_obj proc
    sub rsp, 40
    mov r8, rcx
    sub r8, 16
    mov rcx, qword ptr rt_heap
    xor edx, edx
    call qword ptr GOT_HeapFree
    dec qword ptr rt_live
    add rsp, 40
    ret
rt_free_obj endp

; exit the process with code ecx
rt_exit proc
    sub rsp, 40
    call qword ptr GOT_ExitProcess
rt_exit endp

rt_div_zero proc
    sub rsp, 8
    lea rcx, rt_msg_divzero
    jmp rt_abort
rt_div_zero endp
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
    xor edx, edx
    call rt_abort
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

; the kernel has one console: an abort message goes there too
rt_write_err proc
    jmp rt_write_out
rt_write_err endp

; stop the machine
rt_exit proc
    cli
rt_exit_loop:
    hlt
    jmp rt_exit_loop
rt_exit endp
"#;

const RUNTIME_DATA_WIN: &str = "rt_msg_divzero dq 16
    db \"division by zero\", 0
rt_heap dq 0
";

const RUNTIME_DATA_BARE: &str = "rt_brk dq 2097152
rt_freelists:
    times 256 dq 0
";

const RUNTIME_DATA_COMMON: &str = r#"rt_live dq 0
rt_kinds dq rt_kind_other_node
rt_kind_other_node dq 0, rt_kind_other_name
dq 4611686018427387904, 0
rt_kind_other_name dq 9
    db "std.Other", 0
dq 4611686018427387904, 0
rt_msg_sep dq 2
    db ": ", 0
dq 4611686018427387904, 0
rt_msg_oom dq 13
    db "out of memory", 0
dq 4611686018427387904, 0
rt_max_len dq 2147483647
rt_msg_toolong dq 29
    db "list, map or text is too long", 0
dq 4611686018427387904, 0
rt_msg_oob dq 19
    db "index out of bounds", 0
dq 4611686018427387904, 0
rt_msg_bytepos dq 14
    db "byte position ", 0
dq 4611686018427387904, 0
rt_msg_textpos dq 14
    db "text position ", 0
dq 4611686018427387904, 0
rt_msg_slicestart dq 12
    db "slice start ", 0
dq 4611686018427387904, 0
rt_msg_outofrange dq 16
    db " is out of range", 0
dq 4611686018427387904, 0
rt_msg_notboundary dq 31
    db " is not on a character boundary", 0
dq 4611686018427387904, 0
rt_msg_afterend dq 23
    db " is after the slice end", 0
dq 4611686018427387904, 0
rt_msg_char1 dq 5
    db "char(", 0
dq 4611686018427387904, 0
rt_msg_char2 dq 26
    db ") is not a valid character", 0
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
rt_msg_abort dq 7
    db "abort: ", 0
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
    let asm = c.generate().map_err(crate::comptime::map_error)?;
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
    c.test_mode = options.test_mode;
    c.load(file).map_err(crate::comptime::map_error)?;
    c.declare().map_err(crate::comptime::map_error)?;
    c.generate().map_err(crate::comptime::map_error)
}

const RUNTIME_LINUX: &str = r#"
; ---- Linux runtime: system calls, a size-class heap and mmap for large blocks ----

; rax = zeroed heap object of rcx bytes; rdx = drop procedure (or 0).
; Block layout: [total size][refcount][drop][payload...]. Blocks of less than 4096 bytes come
; from per-size-class free lists and a bump pointer over the zeroed memory after the image;
; larger blocks are their own mmap regions.
rt_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    lea rax, [rcx + 39]
    and rax, -16
    mov qword ptr [rbp - 8], rax
    mov qword ptr [rbp - 16], rdx
    cmp rax, 4096
    jae rt_alloc_big
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
    cmp rdx, qword ptr rt_heap_end
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
    jmp rt_alloc_hdr
rt_alloc_big:
    mov qword ptr [rbp - 24], rdi
    mov qword ptr [rbp - 32], rsi
    xor edi, edi
    mov rsi, rax
    mov edx, 3
    mov r10d, 34
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    mov rdi, qword ptr [rbp - 24]
    mov rsi, qword ptr [rbp - 32]
    cmp rax, -4096
    ja rt_alloc_fail
rt_alloc_hdr:
    mov rcx, qword ptr [rbp - 8]
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
    xor edx, edx
    call rt_abort
rt_alloc endp

; frees the object whose payload is at rcx
rt_free_obj proc
    sub rcx, 24
    dec qword ptr rt_live
    mov rax, qword ptr [rcx]
    cmp rax, 4096
    jb rt_free_small
    push rdi
    push rsi
    mov rdi, rcx
    mov rsi, rax
    mov eax, 11
    syscall
    pop rsi
    pop rdi
    ret
rt_free_small:
    shr rax, 4
    lea r8, rt_freelists
    mov r9, qword ptr [r8 + rax * 8]
    mov qword ptr [rcx + 8], r9
    mov qword ptr [r8 + rax * 8], rcx
    ret
rt_free_obj endp

; write rdx bytes at rcx to stdout
rt_write proc
    push rdi
    push rsi
    mov rsi, rcx
rt_write_loop:
    test rdx, rdx
    jle rt_write_done
    mov edi, 1
    mov eax, 1
    syscall
    test rax, rax
    jle rt_write_done
    add rsi, rax
    sub rdx, rax
    jmp rt_write_loop
rt_write_done:
    pop rsi
    pop rdi
    ret
rt_write endp

; write text rcx to stdout
rt_write_out proc
    sub rsp, 40
    mov rdx, qword ptr [rcx]
    add rcx, 8
    call rt_write
    add rsp, 40
    ret
rt_write_out endp

; write text rcx to standard error (file descriptor 2)
rt_write_err proc
    push rdi
    push rsi
    mov rdx, qword ptr [rcx]
    lea rsi, [rcx + 8]
    mov edi, 2
    mov eax, 1
    syscall
    pop rsi
    pop rdi
    ret
rt_write_err endp

; exit the process with code ecx
rt_exit proc
    mov edi, ecx
    mov eax, 231
    syscall
rt_exit endp

rt_div_zero proc
    sub rsp, 8
    lea rcx, rt_msg_divzero
    jmp rt_abort
rt_div_zero endp
"#;

const RUNTIME_DATA_LINUX: &str = "rt_msg_divzero dq 16
    db \"division by zero\", 0
rt_brk dq 0
rt_heap_end dq 0
rt_freelists:
    times 256 dq 0
";

/// Where a Linux executable is loaded, and how much zeroed memory follows it for the small-object heap.
const LINUX_BASE: u64 = 0x400000;
const LINUX_HEAP: u64 = 256 * 1024 * 1024;
const ELF_HEADERS: u64 = 64 + 56;

/// A static ELF64 executable holding `code` (assembled at `LINUX_BASE + ELF_HEADERS`) that starts at
/// `entry`, with `LINUX_HEAP` bytes of zeroed memory after it (`p_memsz` beyond `p_filesz`).
pub fn elf_image(code: &[u8], entry: u64) -> Vec<u8> {
    let filesz = ELF_HEADERS + code.len() as u64;
    let memsz = (filesz + 4095) / 4096 * 4096 + LINUX_HEAP;
    let mut out = Vec::with_capacity(filesz as usize);
    out.extend_from_slice(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    out.extend_from_slice(&0x3Eu16.to_le_bytes()); // x86-64
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&entry.to_le_bytes());
    out.extend_from_slice(&64u64.to_le_bytes()); // program headers follow the ELF header
    out.extend_from_slice(&0u64.to_le_bytes()); // no section headers
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&56u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // the one loadable segment: readable, writable, executable, from the start of the file
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&LINUX_BASE.to_le_bytes());
    out.extend_from_slice(&LINUX_BASE.to_le_bytes());
    out.extend_from_slice(&filesz.to_le_bytes());
    out.extend_from_slice(&memsz.to_le_bytes());
    out.extend_from_slice(&4096u64.to_le_bytes());
    out.extend_from_slice(code);
    out
}

/// `generate_asm_with` for a given target.
pub fn generate_asm_for(file: &File, options: &CompileOptions, target: Target) -> Result<String, String> {
    let mut c = Compiler::new(target);
    c.opt = options.optimize;
    c.test_mode = options.test_mode;
    c.load(file).map_err(crate::comptime::map_error)?;
    c.declare().map_err(crate::comptime::map_error)?;
    c.generate().map_err(crate::comptime::map_error)
}

/// The entry C uses to call Brasa function `name`: its System V arguments (integers in rdi, rsi,
/// rdx, rcx, r8, r9 and floats in xmm0-7, in order of their kind) are moved to where Brasa
/// functions expect them (position i in rcx, rdx, r8, r9, then the stack; floats as bits in the
/// integer registers), and a float result goes to xmm0. `kinds`: 0 integer, 1 f32, 2 f64.
fn callback_thunk(name: &str, kinds: &[u8], ret: u8) -> String {
    const IREGS: [&str; 6] = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
    let mut t = format!("CB_{} proc
    sub rsp, 120
", name);
    let (mut ni, mut nf) = (0, 0);
    for (i, k) in kinds.iter().enumerate().take(6) {
        match k {
            0 if ni < 6 => {
                t.push_str(&format!("    mov qword ptr [rsp + {}], {}
", 64 + 8 * i, IREGS[ni]));
                ni += 1;
            }
            1 if nf < 8 => {
                t.push_str(&format!("    movd eax, xmm{}
    mov qword ptr [rsp + {}], rax
", nf, 64 + 8 * i));
                nf += 1;
            }
            2 if nf < 8 => {
                t.push_str(&format!("    movq rax, xmm{}
    mov qword ptr [rsp + {}], rax
", nf, 64 + 8 * i));
                nf += 1;
            }
            _ => {}
        }
    }
    for i in 0..kinds.len().min(6) {
        if i < 4 {
            t.push_str(&format!("    mov {}, qword ptr [rsp + {}]
", ARG_REGS[i], 64 + 8 * i));
        } else {
            t.push_str(&format!("    mov rax, qword ptr [rsp + {}]
    mov qword ptr [rsp + {}], rax
", 64 + 8 * i, 32 + 8 * (i - 4)));
        }
    }
    t.push_str(&format!("    call {}
", name));
    match ret {
        1 => t.push_str("    movd xmm0, eax
"),
        2 => t.push_str("    movq xmm0, rax
"),
        _ => {}
    }
    t.push_str(&format!("    add rsp, 120
    ret
CB_{} endp
", name));
    t
}

/// The C functions a Linux program imports: the names of the `GOT_<name>` slots in its assembly.
fn linux_imports(asm: &str) -> Vec<String> {
    asm.lines().filter_map(|l| l.strip_prefix("GOT_").and_then(|r| r.strip_suffix(" dq 0"))).map(|n| n.to_string()).collect()
}

/// The libraries a Linux program links (`; link <name>` lines of its assembly) as shared-object names.
fn linux_needed(asm: &str) -> Vec<String> {
    asm.lines()
        .filter_map(|l| l.strip_prefix("; link "))
        .map(|n| match n.trim() {
            "c" => "libc.so.6".to_string(),
            "m" => "libm.so.6".to_string(),
            "dl" => "libdl.so.2".to_string(),
            "pthread" => "libpthread.so.0".to_string(),
            "rt" => "librt.so.1".to_string(),
            "ssl" => "libssl.so.3".to_string(),
            "crypto" => "libcrypto.so.3".to_string(),
            other => format!("lib{}.so", other),
        })
        .collect()
}

/// The part of a dynamically linked executable that comes before the code: the ELF header, the
/// program headers (interpreter, the one loadable segment, the dynamic section, a non-executable
/// stack), the interpreter name, the dynamic string and symbol tables, a hash table, the
/// relocations that fill the import slots, and the dynamic section. The size depends only on
/// the names, not on `slots` (the addresses of the import slots), `entry` or `code_len`.
pub fn elf_dynamic_head(imports: &[String], needed: &[String], slots: &[u64], entry: u64, code_len: u64) -> Vec<u8> {
    const INTERP: &[u8] = b"/lib64/ld-linux-x86-64.so.2\0";
    let phnum = 5u64;
    let interp_off = 64 + 56 * phnum;
    let str_off = (interp_off + INTERP.len() as u64 + 7) / 8 * 8;
    // dynamic strings: "\0", the libraries, then the imported names
    let mut strtab = vec![0u8];
    let name_at = |strtab: &mut Vec<u8>, n: &str| {
        let at = strtab.len() as u32;
        strtab.extend_from_slice(n.as_bytes());
        strtab.push(0);
        at
    };
    let needed_at: Vec<u32> = needed.iter().map(|n| name_at(&mut strtab, n)).collect();
    let import_at: Vec<u32> = imports.iter().map(|n| name_at(&mut strtab, n)).collect();
    let n = imports.len() as u64;
    let sym_off = (str_off + strtab.len() as u64 + 7) / 8 * 8;
    let hash_off = sym_off + 24 * (n + 1);
    let hash_len = 4 * (2 + 1 + (n + 1));
    let rela_off = (hash_off + hash_len + 7) / 8 * 8;
    let dyn_off = (rela_off + 24 * n + 15) / 16 * 16;
    let dyn_entries = needed.len() as u64 + 11;
    let head_len = dyn_off + 16 * dyn_entries;
    let filesz = (head_len + 15) / 16 * 16 + code_len;
    let memsz = (filesz + 4095) / 4096 * 4096 + LINUX_HEAP;
    let at = |off: u64| LINUX_BASE + off;

    let mut out = Vec::new();
    // ELF header
    out.extend_from_slice(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&0x3Eu16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&entry.to_le_bytes());
    out.extend_from_slice(&64u64.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&56u16.to_le_bytes());
    out.extend_from_slice(&(phnum as u16).to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    let phdr = |out: &mut Vec<u8>, kind: u32, flags: u32, off: u64, filesz: u64, memsz: u64, align: u64| {
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
        out.extend_from_slice(&at(off).to_le_bytes());
        out.extend_from_slice(&at(off).to_le_bytes());
        out.extend_from_slice(&filesz.to_le_bytes());
        out.extend_from_slice(&memsz.to_le_bytes());
        out.extend_from_slice(&align.to_le_bytes());
    };
    phdr(&mut out, 6, 4, 64, 56 * phnum, 56 * phnum, 8); // PT_PHDR
    phdr(&mut out, 3, 4, interp_off, INTERP.len() as u64, INTERP.len() as u64, 1); // PT_INTERP
    phdr(&mut out, 1, 7, 0, filesz, memsz, 4096); // PT_LOAD
    phdr(&mut out, 2, 6, dyn_off, 16 * dyn_entries, 16 * dyn_entries, 8); // PT_DYNAMIC
    // PT_GNU_STACK: no executable stack
    out.extend_from_slice(&0x6474E551u32.to_le_bytes());
    out.extend_from_slice(&6u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 48]);
    out.extend_from_slice(INTERP);
    out.resize(str_off as usize, 0);
    out.extend_from_slice(&strtab);
    out.resize(sym_off as usize, 0);
    // symbols: the null symbol, then one undefined global per import
    out.extend_from_slice(&[0u8; 24]);
    for &name in &import_at {
        out.extend_from_slice(&name.to_le_bytes());
        out.push(0x10); // STB_GLOBAL, STT_NOTYPE
        out.push(0);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
    }
    // hash table: one bucket chaining every symbol
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&((n + 1) as u32).to_le_bytes());
    out.extend_from_slice(&(if n > 0 { 1u32 } else { 0 }).to_le_bytes());
    for i in 0..=n {
        out.extend_from_slice(&(if i >= 1 && i < n { i as u32 + 1 } else { 0 }).to_le_bytes());
    }
    out.resize(rela_off as usize, 0);
    // R_X86_64_GLOB_DAT: the slot receives the address of the symbol
    for (i, slot) in slots.iter().enumerate() {
        out.extend_from_slice(&slot.to_le_bytes());
        out.extend_from_slice(&(((i as u64 + 1) << 32) | 6).to_le_bytes());
        out.extend_from_slice(&0i64.to_le_bytes());
    }
    out.resize(dyn_off as usize, 0);
    let dynamic = |out: &mut Vec<u8>, tag: u64, val: u64| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&val.to_le_bytes());
    };
    for &name in &needed_at {
        dynamic(&mut out, 1, name as u64); // DT_NEEDED
    }
    dynamic(&mut out, 4, at(hash_off)); // DT_HASH
    dynamic(&mut out, 5, at(str_off)); // DT_STRTAB
    dynamic(&mut out, 6, at(sym_off)); // DT_SYMTAB
    dynamic(&mut out, 10, strtab.len() as u64); // DT_STRSZ
    dynamic(&mut out, 11, 24); // DT_SYMENT
    dynamic(&mut out, 7, at(rela_off)); // DT_RELA
    dynamic(&mut out, 8, 24 * n); // DT_RELASZ
    dynamic(&mut out, 9, 24); // DT_RELAENT
    dynamic(&mut out, 30, 8); // DT_FLAGS: DF_BIND_NOW
    dynamic(&mut out, 0, 0); // DT_NULL
    dynamic(&mut out, 0, 0);
    debug_assert_eq!(out.len() as u64, head_len);
    out.resize(((head_len + 15) / 16 * 16) as usize, 0);
    out
}

/// Compiles to a Linux x86-64 executable with the compiler's own assembler (no linker): a static
/// one that needs no library, or, when the program calls C functions, a dynamically linked one.
pub fn compile_to_linux(file: &File, output: &str, options: &CompileOptions) -> Result<(), String> {
    let asm = generate_asm_for(file, options, Target::Linux)?;
    let imports = linux_imports(&asm);
    let image = if imports.is_empty() {
        let assembled = crate::asm_x86::assemble(&asm, LINUX_BASE + ELF_HEADERS, 64)?;
        let Some(&entry) = assembled.symbols.get("brasa_start") else {
            return Err("internal error: no entry point".into());
        };
        elf_image(&assembled.bytes, entry)
    } else {
        let needed = linux_needed(&asm);
        let head_len = elf_dynamic_head(&imports, &needed, &vec![0; imports.len()], 0, 0).len() as u64;
        let assembled = crate::asm_x86::assemble(&asm, LINUX_BASE + head_len, 64)?;
        let Some(&entry) = assembled.symbols.get("brasa_start") else {
            return Err("internal error: no entry point".into());
        };
        let mut slots = Vec::new();
        for n in &imports {
            let Some(&a) = assembled.symbols.get(&format!("GOT_{}", n)) else {
                return Err(format!("internal error: no import slot for {}", n));
            };
            slots.push(a);
        }
        let mut image = elf_dynamic_head(&imports, &needed, &slots, entry, assembled.bytes.len() as u64);
        image.extend_from_slice(&assembled.bytes);
        image
    };
    std::fs::write(output, &image).map_err(|e| format!("cannot write {}: {}", output, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(output, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// What a symbol of the generated assembly belongs to, for the link report: the key of a module,
/// or one of the shared groups.
fn symbol_owner(sym: &str, modules: &[(String, String)]) -> Option<(String, bool)> {
    // (owner, is_code)
    let in_module = |rest: &str| -> Option<String> {
        modules.iter().filter(|(_, s)| rest.starts_with(&format!("{}_", s)) || rest == s.as_str()).max_by_key(|(_, s)| s.len()).map(|(k, _)| k.clone())
    };
    if let Some(rest) = sym.strip_prefix("B_init_") {
        return Some((in_module(rest).unwrap_or_else(|| "(helpers)".into()), true));
    }
    if sym.starts_with("B_drop_") || sym.starts_with("B_copy_") || sym.starts_with("B_eq_") || sym.starts_with("B_hash_") {
        return Some(("(helpers)".into(), true));
    }
    if let Some(rest) = sym.strip_prefix("B_") {
        return Some((in_module(rest).unwrap_or_else(|| "(helpers)".into()), true));
    }
    if let Some(rest) = sym.strip_prefix("G_") {
        return Some((in_module(rest).unwrap_or_else(|| "(helpers)".into()), false));
    }
    if sym.starts_with("S_") {
        return Some(("(strings)".into(), false));
    }
    if sym.starts_with("E_") || sym.starts_with("C_") {
        return Some(("(helpers)".into(), false));
    }
    if sym.starts_with("rt_msg_") || sym == "rt_heap" || sym == "rt_image_end" {
        return Some(("(runtime)".into(), false));
    }
    if sym.starts_with("rt_") || sym.starts_with("CB_") || sym == "brasa_start" {
        return Some(("(runtime)".into(), true));
    }
    None
}

/// `--explain-link`: the bytes of code and data each package contributes to the program, from the
/// positions of the symbols in the assembled image (the same assembly the executable is made of).
pub fn link_report(file: &File, options: &CompileOptions, target: Target) -> Result<String, String> {
    let mut c = Compiler::new(target);
    c.opt = options.optimize;
    c.test_mode = options.test_mode;
    c.load(file).map_err(crate::comptime::map_error)?;
    c.declare().map_err(crate::comptime::map_error)?;
    let asm = c.generate().map_err(crate::comptime::map_error)?;
    let program = c.modules.first().map(|m| m.name.clone()).unwrap_or_default();
    let modules: Vec<(String, String)> = c.modules.iter().map(|m| (m.name.clone(), sym_of(&m.name))).collect();
    let (text, org) = match target {
        Target::Linux => (asm.clone(), LINUX_BASE + ELF_HEADERS),
        _ => (asm.replacen("\n.data\n", "\nrt_text_end:\nalign 4096\nrt_data_start:\n", 1), crate::pe::PE_BASE + 0x1000),
    };
    let assembled = crate::asm_x86::assemble(&text, org, 64)?;
    let end = org + assembled.bytes.len() as u64;
    let mut syms: Vec<(&String, u64)> = assembled.symbols.iter().map(|(k, v)| (k, *v)).filter(|(_, v)| *v >= org && *v <= end).collect();
    syms.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(b.0)));
    // bytes between one symbol and the next belong to the owner of the last symbol that has one
    let mut sizes: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut owner: Option<(String, bool)> = None;
    for (i, (name, at)) in syms.iter().enumerate() {
        if let Some(o) = symbol_owner(name, &modules) {
            owner = Some(o);
        }
        let next = syms.get(i + 1).map(|s| s.1).unwrap_or(end);
        if let Some((o, is_code)) = &owner {
            if matches!(name.as_str(), "rt_text_end" | "rt_data_start") {
                continue;
            }
            let e = sizes.entry(o.clone()).or_insert((0, 0));
            if *is_code {
                e.0 += next - at;
            } else {
                e.1 += next - at;
            }
        }
    }
    // what each package's reachable code can do: a function counts when its symbol is in the
    // program, an `extern` when its import slot is; generic code is always counted
    let effects_of = |m: &Module| -> crate::effects::Effects {
        let ms = sym_of(&m.name);
        let keep = |d: &Decl| -> bool {
            match &d.kind {
                DeclKind::Func(f) => {
                    if !f.generics.is_empty() {
                        return true;
                    }
                    let name = match &f.receiver {
                        Some(r) => match &r.ty {
                            Type::Named { path, args } if args.is_empty() => format!("B_{}_{}_{}", ms, path.last().cloned().unwrap_or_default(), f.name),
                            _ => return true,
                        },
                        None => format!("B_{}_{}", ms, f.name),
                    };
                    assembled.symbols.contains_key(&name)
                }
                DeclKind::Extern(e) => assembled.symbols.contains_key(&format!("GOT_{}", e.name)),
                _ => false,
            }
        };
        crate::effects::of_decls(&m.file.decls, &keep)
    };
    let effects: BTreeMap<String, String> = c.modules.iter().map(|m| (m.name.clone(), effects_of(m).show())).collect();
    let mut rows: Vec<(String, &str, u64, u64)> = sizes
        .into_iter()
        .map(|(k, (code, data))| {
            let kind = if k.starts_with('(') {
                "-"
            } else if k == program {
                "program"
            } else if std_source(&k).is_some() {
                "std"
            } else {
                "package"
            };
            (k, kind, code, data)
        })
        .collect();
    rows.sort_by(|a, b| (b.2 + b.3).cmp(&(a.2 + a.3)).then(a.0.cmp(&b.0)));
    let width = rows.iter().map(|r| r.0.len()).max().unwrap_or(7).max(7);
    let mut out = format!("{:<w$}  {:<8} {:>8} {:>8}  {}\n", "package", "kind", "code", "data", "effects", w = width);
    let (mut tc, mut td) = (0, 0);
    for (k, kind, code, data) in &rows {
        out.push_str(&format!("{:<w$}  {:<8} {:>8} {:>8}  {}\n", k, kind, code, data, effects.get(k).map(|s| s.as_str()).unwrap_or(""), w = width));
        tc += code;
        td += data;
    }
    out.push_str(&format!("{:<w$}  {:<8} {:>8} {:>8}\n", "total", "", tc, td, w = width));
    out.push_str(&format!("{} bytes of code and data\n", tc + td));
    Ok(out)
}

/// Compiles to a native Windows x86-64 executable. The compiler writes the PE file itself
/// (`pe::pe_image`): no assembler, linker or Visual Studio is involved.
pub fn compile_to_native(file: &File, output_exe: &str) -> Result<(), String> {
    compile_to_native_with(file, output_exe, &CompileOptions::default())
}

/// `compile_to_native` with explicit options. Nothing is written unless the image is complete.
pub fn compile_to_native_with(file: &File, output_exe: &str, options: &CompileOptions) -> Result<(), String> {
    let mut c = Compiler::new(Target::Windows);
    c.opt = options.optimize;
    c.test_mode = options.test_mode;
    c.load(file).map_err(crate::comptime::map_error)?;
    c.declare().map_err(crate::comptime::map_error)?;
    let asm = c.generate().map_err(crate::comptime::map_error)?;
    let image = crate::pe::pe_image(&asm, &c.win_imports, c.uses_ui)?;
    std::fs::write(output_exe, &image).map_err(|e| format!("cannot write {}: {}", output_exe, e))
}

/// The reference-count and live-object instructions of the runtime, made atomic (`lock` prefix)
/// for programs that use `shared` or `spawn_thread`.
fn lock_counts(asm: &str) -> String {
    let mut out = String::with_capacity(asm.len() + 1024);
    for line in asm.lines() {
        let t = line.trim_start();
        let atomic = matches!(t, "inc qword ptr [rcx - 16]" | "dec qword ptr [rcx - 16]" | "inc qword ptr rt_live" | "dec qword ptr rt_live");
        if atomic {
            out.push_str("    lock ");
            out.push_str(t);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Threads: transferable types, `shared`, `spawn_thread`
// ---------------------------------------------------------------------------

/// Names declared by `let`/`var`/`for` in a block (at any depth).
fn declared_names(b: &[Stmt], out: &mut HashSet<String>) {
    for s in b {
        match &s.kind {
            StmtKind::Var(vd) => {
                out.insert(vd.name.clone());
            }
            StmtKind::For { vars, body, .. } => {
                out.extend(vars.iter().cloned());
                declared_names(body, out);
            }
            StmtKind::While { body, .. } | StmtKind::Unsafe(body) => declared_names(body, out),
            StmtKind::Expr(e) => {
                if let ExprKind::If { branches, else_body } = &e.kind {
                    for (_, b) in branches {
                        declared_names(b, out);
                    }
                    if let Some(b) = else_body {
                        declared_names(b, out);
                    }
                }
            }
            _ => {}
        }
    }
}

impl Compiler {
    /// The first part of `ty` that cannot cross a thread boundary, as (path, type as written).
    /// Numbers, bool, text, char, enums, closures and `Thread` are transferable; records, lists,
    /// maps, channels and results are when everything inside is; `new T`, weak references,
    /// raw pointers and futures are not. Recursive types are visited once.
    fn transfer_problem(&self, ty: &Ty, path: &str, seen: &mut HashSet<String>) -> Option<(String, String)> {
        match ty {
            Ty::Rc(inner) => Some((path.to_string(), format!("new {}", inner.show()))),
            Ty::Weak(inner) => Some((path.to_string(), format!("a weak reference to new {}", inner.show()))),
            Ty::Ptr(_) | Ty::Future(_) => Some((path.to_string(), ty.show())),
            Ty::List(t) => self.transfer_problem(t, &format!("{}[]", path), seen),
            Ty::Map(k, v) => self
                .transfer_problem(k, &format!("{}[key]", path), seen)
                .or_else(|| self.transfer_problem(v, &format!("{}[]", path), seen)),
            Ty::Chan(t) | Ty::Result(t) => self.transfer_problem(t, path, seen),
            Ty::Rec(name) => {
                if !seen.insert(name.clone()) {
                    return None;
                }
                let info = self.records.get(name)?;
                for (f, fty, _) in &info.fields {
                    if let Some(p) = self.transfer_problem(fty, &format!("{}.{}", path, f), seen) {
                        return Some(p);
                    }
                }
                None
            }
            Ty::Enum(name) => {
                if !seen.insert(name.clone()) {
                    return None;
                }
                let info = self.enums.get(name)?;
                for (v, tys) in &info.variants {
                    for t in tys {
                        if let Some(p) = self.transfer_problem(t, &format!("{}.{}", path, v), seen) {
                            return Some(p);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// An error naming `what` (an argument or a captured variable) when `ty` is not transferable.
    fn transfer_check(&self, what: &str, root: &str, ty: &Ty) -> R<()> {
        let root = if root.starts_with('$') { "value" } else { root };
        if let Some((path, shown)) = self.transfer_problem(ty, root, &mut HashSet::new()) {
            let hint = "it cannot be used by another thread; share data between threads with `shared`, `Mutex<T>` or `Atomic`";
            if path == root {
                return self.err(format!("{} has type '{}', which is not transferable between threads: {}", what, shown, hint));
            }
            return self.err(format!("{}: field '{}' has type '{}', which is not transferable between threads: {}", what, path, shown, hint));
        }
        Ok(())
    }

    fn thread_safe_global(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Rec(n) if n.contains("sync.Mutex") || n.contains("sync.Atomic"))
    }

    /// A mutable global `var` that the function started with `spawn_thread` (or a function it calls
    /// by name) uses must be a `Mutex` or an `Atomic`. Calls through function values, methods and
    /// qualified names are not followed; library code keeps its own per-thread state.
    fn check_thread_globals(&self, callee: &Expr) -> R<()> {
        let ExprKind::Ident(start) = &callee.kind else { return Ok(()) };
        let mut todo = vec![(self.cur_mod.clone(), start.clone())];
        let mut done = HashSet::new();
        while let Some((m, f)) = todo.pop() {
            if !done.insert((m.clone(), f.clone())) || crate::comptime::package_source(&m, self.target).is_some() {
                continue;
            }
            let Some(module) = self.modules.iter().find(|x| x.name == m) else { continue };
            let Some(fd) = module.file.decls.iter().find_map(|d| match &d.kind {
                DeclKind::Func(fd) if fd.name == f && fd.receiver.is_none() => Some(fd),
                _ => None,
            }) else {
                continue;
            };
            let mut names = Vec::new();
            collect_block(&fd.body, &mut names);
            let mut locals = HashSet::new();
            declared_names(&fd.body, &mut locals);
            locals.extend(fd.params.iter().map(|p| p.name.clone()));
            for n in names {
                if locals.contains(&n) {
                    continue;
                }
                if let Some(g) = self.globals.get(&(m.clone(), n.clone())) {
                    if g.mutable && g.konst.is_none() && !self.thread_safe_global(&g.ty) {
                        return self.err(format!(
                            "global variable '{}' is mutable and is used by '{}', which runs on another thread: threads cannot share a plain global; use `shared Atomic` or `shared Mutex<{}>` instead",
                            n,
                            f,
                            g.ty.show()
                        ));
                    }
                } else if self.funcs.contains_key(&(m.clone(), n.clone())) {
                    todo.push((m.clone(), n));
                }
            }
        }
        Ok(())
    }

    /// The `shared` local that the place `e` is rooted at.
    fn shared_root(&self, e: &Expr, include_root: bool) -> Option<String> {
        match &e.kind {
            ExprKind::Ident(n) if include_root && self.fs.shared_locals.contains(n) && self.lookup(n).is_some() => Some(n.clone()),
            ExprKind::Field { expr, .. } | ExprKind::Index { expr, .. } => self.shared_root(expr, true),
            _ => None,
        }
    }

    /// Writing through a `shared` value is an error: its content is immutable.
    fn shared_write_check(&self, target: &Expr, include_root: bool) -> R<()> {
        if let Some(n) = self.shared_root(target, include_root) {
            let ty = self.lookup(&n).map(|l| l.ty.show()).unwrap_or_default();
            return self.err(format!(
                "'{}' is a `shared` value: its content is immutable; to change it from several threads put it in a `Mutex<{}>`",
                n, ty
            ));
        }
        Ok(())
    }

    /// `shared e`: the value of `e`, which every thread may read (atomic counts, immutable content).
    fn shared_expr(&mut self, inner: &Expr, want: Option<&Ty>) -> R<Ty> {
        let t = self.value(inner, want)?;
        if !matches!(t, Ty::Rec(_) | Ty::List(_) | Ty::Map(..) | Ty::Text) {
            return self.err(format!("`shared` needs a record, list, map or text, found {}", t.show()));
        }
        self.transfer_check("the value after `shared`", "value", &t)?;
        Ok(t)
    }

    /// `spawn_thread f(args)`: the arguments are evaluated here, then the call runs on a new thread.
    fn spawn_thread(&mut self, call: &Expr, line: u32) -> R<Ty> {
        let Some(sig) = self.funcs.get(&("thread".to_string(), "start".to_string())).cloned() else {
            return self.err("'spawn_thread' needs the thread library");
        };
        let ExprKind::Call { callee, args, targs } = &call.kind else {
            return self.err("'spawn_thread' needs a function call, like `spawn_thread work(x)`");
        };
        self.push_scope();
        let mut new_args: Vec<Arg> = Vec::new();
        let mut labels: HashMap<String, String> = HashMap::new();
        for (i, a) in args.iter().enumerate() {
            if a.by_ref {
                return self.err("'spawn_thread' cannot pass an argument by reference");
            }
            if let ExprKind::Lambda { params, body } = &a.value.kind {
                // what the closure captures must be transferable too
                let mut names = Vec::new();
                collect_block(body, &mut names);
                let own: HashSet<&String> = params.iter().map(|p| &p.name).collect();
                let mut seen_caps = HashSet::new();
                for n in names {
                    if own.contains(&n) || !seen_caps.insert(n.clone()) {
                        continue;
                    }
                    if let Some(l) = self.lookup(&n) {
                        self.transfer_check(&format!("captured variable '{}'", n), &n, &l.ty)?;
                    }
                }
            }
            let plain = matches!(
                a.value.kind,
                ExprKind::Ident(_) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Char(_) | ExprKind::Bool(_) | ExprKind::Nil
            );
            if plain {
                if let ExprKind::Ident(n) = &a.value.kind {
                    labels.insert(n.clone(), format!("argument '{}'", n));
                }
                new_args.push(a.clone());
                continue;
            }
            let name = format!("$thr{}", i);
            let decl = VarDecl { name: name.clone(), ty: None, value: Some(a.value.clone()), mutable: false };
            self.stmt_inner(&Stmt { kind: StmtKind::Var(decl), line })?;
            labels.insert(name.clone(), format!("argument {}", i + 1));
            let mut na = a.clone();
            na.value = Expr { kind: ExprKind::Ident(name), line };
            new_args.push(na);
        }
        let rewritten = Expr { kind: ExprKind::Call { callee: callee.clone(), args: new_args, targs: targs.clone() }, line };
        let mut used = Vec::new();
        collect_expr(&rewritten, &mut used);
        let mut seen = HashSet::new();
        for n in used {
            if !seen.insert(n.clone()) {
                continue;
            }
            if let Some(l) = self.lookup(&n) {
                let what = labels.get(&n).cloned().unwrap_or_else(|| format!("captured variable '{}'", n));
                self.transfer_check(&what, &n, &l.ty)?;
            }
        }
        self.check_thread_globals(callee)?;
        let body = vec![Stmt { kind: StmtKind::Expr(rewritten), line }];
        let want = Ty::Func { params: vec![], ret: Box::new(Ty::Void), ext: false };
        self.lambda(&[], &body, Some(&want))?;
        let t = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t));
        self.emit_call(&[t], &format!("call {}", sig.sym), None);
        self.free(t);
        self.pop_scope();
        Ok(sig.ret.clone())
    }
}

/// The package part of a qualified type name (`text.Builder`, `list.Stack<int>`).
fn type_owner(q: &str) -> &str {
    let base = q.split('<').next().unwrap_or(q);
    base.rsplit_once('.').map_or(base, |x| x.0)
}

/// Single-segment type names written in `t` (the package's own types), except type parameters.
fn named_types(t: &Type, generics: &[&str], out: &mut Vec<String>) {
    match t {
        Type::Named { path, args } => {
            if path.len() == 1 && !generics.contains(&path[0].as_str()) {
                out.push(path[0].clone());
            }
            for a in args {
                named_types(a, generics, out);
            }
        }
        Type::Optional(i) | Type::Pointer(i) | Type::Shared(i) | Type::Slice(i) | Type::Array(_, i) => named_types(i, generics, out),
        Type::Func { params, ret, .. } => {
            for p in params {
                named_types(p, generics, out);
            }
            if let Some(r) = ret {
                if let Some(t) = &r.ty {
                    named_types(t, generics, out);
                }
            }
        }
    }
}
