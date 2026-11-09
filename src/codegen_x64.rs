//! Native x86-64 back end for Zila Lang (Windows, Win64 ABI).
//!
//! Zila Lang source -> x86-64 assembly (MASM-style syntax) -> built-in assembler -> PE .exe (`pe.rs`) or ELF.
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
//! Every Zila Lang function uses the Win64 calling convention and only volatile
//! registers, so any top-level function can be handed to C as a callback.

use crate::ast::*;
use crate::parser::parse_source;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

type R<T> = Result<T, String>;

#[path = "derive_ops.rs"]
mod derive_ops;
use derive_ops::Cap;
mod analysis;
mod calls;
mod drop;
mod expressions;
mod layout;
mod lifecycle;
mod memory;
mod statements;
mod fold;
mod generics;
mod output;
mod runtime;
mod threads;
mod typed;
mod checker;
use typed::TypedProgram;
use analysis::*;
use fold::*;
use generics::*;
pub use output::*;
use runtime::*;
use threads::*;

/// The Linux layer of a library: `std/<name>.linux.zila`, used instead of `std/<name>.zila` when the
/// target is Linux. Libraries that touch the operating system have one.
pub fn std_linux_source(name: &str) -> Option<&'static str> {
    match name {
        "sys" => Some(include_str!("../std/sys.linux.zila")),
        "task" => Some(include_str!("../std/task.linux.zila")),
        "fs" => Some(include_str!("../std/fs.linux.zila")),
        "net" => Some(include_str!("../std/net.linux.zila")),
        "tls" => Some(include_str!("../packages/tls/linux/tls.zila")),
        "ui" => Some(include_str!("../packages/ui/linux/ui.zila")),
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

/// Standard library modules written in Zila Lang and embedded in the compiler.
/// The standard library packages the compiler and the runtime rest on, or that most programs need:
/// their names are reserved and a package of a project cannot take them. Every other standard
/// library is a package of `packages/` that the compiler embeds, and a package of the project
/// may shadow it.
pub const CORE_STD: [&str; 19] = [
    "bufio", "derive", "errors", "float", "format", "fs", "future", "list", "math", "net", "reflect", "set", "sorted", "sync", "sys", "task", "text", "thread", "ws",
];

pub fn is_core_std(name: &str) -> bool {
    CORE_STD.contains(&name)
}

pub fn std_source(name: &str) -> Option<&'static str> {
    match name {
        "ui" => Some(include_str!("../packages/ui/ui.zila")),
        "task" => Some(include_str!("../std/task.zila")),
        "future" => Some(include_str!("../std/future.zila")),
        "thread" => Some(include_str!("../std/thread.zila")),
        "sync" => Some(include_str!("../std/sync.zila")),
        "fs" => Some(include_str!("../std/fs.zila")),
        "net" => Some(include_str!("../std/net.zila")),
        "sys" => Some(include_str!("../std/sys.zila")),
        "ws" => Some(include_str!("../std/ws.zila")),
        "tls" => Some(include_str!("../packages/tls/tls.zila")),
        "float" => Some(include_str!("../std/float.zila")),
        "math" => Some(include_str!("../std/math.zila")),
        "text" => Some(include_str!("../std/text.zila")),
        "errors" => Some(include_str!("../std/errors.zila")),
        "list" => Some(include_str!("../std/list.zila")),
        "format" => Some(include_str!("../std/format.zila")),
        "set" => Some(include_str!("../std/set.zila")),
        "sorted" => Some(include_str!("../std/sorted.zila")),
        "bufio" => Some(include_str!("../std/bufio.zila")),
        "derive" => Some(include_str!("../std/derive.zila")),
        "reflect" => Some(include_str!("../std/reflect.zila")),
        "json" => Some(include_str!("../packages/json/json.zila")),
        "web" => Some(include_str!("../packages/web/web.zila")),
        "crypto" => Some(include_str!("../packages/crypto/crypto.zila")),
        "db" => Some(include_str!("../packages/db/db.zila")),
        "postgres" => Some(include_str!("../packages/postgres/postgres.zila")),
        "sqlserver" => Some(include_str!("../packages/sqlserver/sqlserver.zila")),
        "ai" => Some(include_str!("../packages/ai/ai.zila")),
        "mcp" => Some(include_str!("../packages/mcp/mcp.zila")),
        "rag" => Some(include_str!("../packages/rag/rag.zila")),
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
    /// Zila Lang variadic: last parameter `name: ...T` collects a list.
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
    /// The declarations of the program: types, signatures and globals (see `typed.rs`).
    tp: TypedProgram,
    imports: HashMap<String, HashSet<String>>,
    /// `(importing module, qualifier)` to the module key when a package is imported under a name
    /// that is not its key (two packages with the same name, or one package under two names).
    mod_aliases: HashMap<(String, String), String>,
    enum_statics: BTreeSet<(String, usize)>,
    /// Functions whose calls are compiled in place (with `opt`): symbol -> (body, module).
    inline_bodies: HashMap<String, (Expr, String)>,
    inline_depth: usize,
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
    /// Type parameters bound to types while an instance is compiled.
    bindings: HashMap<String, Ty>,
    /// All signatures are known (bounds can be checked).
    declared: bool,
    /// Bound checks met before all methods were known: (type, interface, module, line).
    deferred_bounds: Vec<(Ty, String, u32)>,
    custom_allocator: Option<crate::noalloc::CustomAllocatorInfo>,
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
            tp: TypedProgram::default(),
            imports: HashMap::new(),
            mod_aliases: HashMap::new(),
            enum_statics: BTreeSet::new(),
            inline_bodies: HashMap::new(),
            inline_depth: 0,
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
            bindings: HashMap::new(),
            declared: false,
            deferred_bounds: Vec::new(),
            custom_allocator: None,
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
        Err(format!("{}.zila:{}: {}", self.cur_mod, self.line, msg.into()))
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

    /// Loads the text `file.zila:line` of the statement being compiled into `reg`, for a failing path.
    fn at(&mut self, reg: &str) {
        let l = self.string_lit(&format!("{}.zila:{}", self.cur_mod, self.line));
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

    fn generate(&mut self) -> R<String> {
        let mods: Vec<(String, Vec<Decl>)> = self.modules.iter().map(|m| (m.name.clone(), m.file.decls.clone())).collect();
        let main_mod = mods[0].0.clone();
        let main_ret = match self.tp.funcs.get(&(main_mod.clone(), "main".to_string())) {
            Some(s) => s.ret.clone(),
            None => return Err(format!("{}.zila: no 'main' function found", main_mod)),
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
                            if let Some(sg) = self.tp.funcs.get(&(m.to_string(), fd.name.clone())) {
                                self.tp.exported.insert(sg.sym.clone());
                            }
                        }
                        self.gen_func(m, fd, d.attrs.iter().any(|a| a.name == "naked"))?;
                    }
                }
            }
        }
        self.drain_pending()?;
        let mut recs: Vec<String> = self.tp.records.keys().cloned().collect();
        recs.sort();
        for q in recs {
            self.gen_record_helpers(&q);
        }
        let mut enums: Vec<String> = self.tp.enums.keys().cloned().collect();
        enums.sort();
        for q in enums {
            self.gen_enum_drop(&q);
        }
        let mut rcs: Vec<String> = self.tp.rc_recs.iter().filter(|q| self.tp.records.contains_key(*q)).cloned().collect();
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
        out.push_str("; Generated by the Zila Lang compiler. Do not edit.
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
    jz zila_main_ok
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
zila_main_ok:
"
        } else {
            ""
        };
        let mut start = String::new();
        if bare {
            // The loader jumps here in long mode with a flat identity map.
            start.push_str("zila_start proc
    cli
    cld
    mov rsp, 651272
    sub rsp, 40
");
        } else if linux {
            // The kernel enters with the stack pointing at argc. The heap is the zeroed memory
            // after the image, from the next page.
            start.push_str("zila_start proc
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
            start.push_str("zila_start proc
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
zila_start endp

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
                // and flush what C buffered before Zila Lang writes, so the output keeps its order
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
        // the thunk moves up to four of them where Zila Lang functions expect them
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
        let mut globals: Vec<&Global> = self.tp.globals.values().filter(|g| g.konst.is_none()).collect();
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
            for e in &self.tp.exported {
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
                let g = &self.tp.globals[&(m.to_string(), vd.name.clone())];
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
                self.tp.methods[&(method_key(&rt), fd.name.clone())].clone()
            }
            None => self.tp.funcs[&(m.to_string(), fd.name.clone())].clone(),
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
        if has_await && self.tp.exported.contains(&sig.sym) {
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
        self.tp.records[r].size
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
        self.tp.pub_types.contains(q) || type_owner(q) == from
    }

    /// The declared fields of record `r` (an instance of a generic record uses its template's).
    fn rec_field_decls(&self, r: &str) -> Option<(&str, &[Field])> {
        let (m, f) = self.tp.rec_decls.get(r)?;
        if f.is_empty() {
            if let Some((tq, _)) = self.tp.inst_args.get(r) {
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
        self.tp.records.get(r)?.fields.iter().find(|f| f.0 == name).map(|f| (f.1.clone(), f.2))
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
            && !self.tp.globals.contains_key(&(self.cur_mod.clone(), name.to_string()))
            && self.imports.get(&self.cur_mod).map_or(false, |i| i.contains(name))
    }

    /// Qualified name of the enum called `name` in the current module.
    fn enum_type_named(&self, name: &str) -> Option<String> {
        if self.lookup(name).is_some() {
            return None;
        }
        let q = format!("{}.{}", self.cur_mod, name);
        self.tp.enum_decls.contains_key(&q).then_some(q)
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
            if matches!(&t, Ty::Rec(r) if !self.tp.shared_recs.contains(r)) {
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
                let sig = match self.tp.funcs.get(&("float".to_string(), "to_text".to_string())) {
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

}
