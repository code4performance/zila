//! The typed program: the declarations the checker settles before any code is emitted.
//!
//! Records, enums, interfaces, signatures, globals and the facts derived from them live here, apart
//! from the emission state of `Compiler` (labels, frame slots, assembly text). Code generation reads
//! this form for everything it asks about a declaration, and `load`/`declare` are what fill it.
//!
//! `TypedExpr`, `TypedStmt`, and `TypedBody` are the typed intermediate tree produced by the checker
//! (see `checker.rs`) and consumed by emission. Each node carries its resolved `Ty` and line number;
//! call nodes carry an explicit `TypedCallee` so emission never re-resolves the callee.

use super::*;

// ---------------------------------------------------------------------------
// Typed expression tree
// ---------------------------------------------------------------------------

/// A fully-typed expression node.  `ty` is the resolved type of the whole expression; `line` is
/// the source line for error messages.
#[derive(Debug, Clone)]
#[allow(dead_code)] // until emission consumes the tree (tasks 2.2, 3.1)
pub(super) struct TypedExpr {
    pub(super) kind: TypedExprKind,
    pub(super) ty: Ty,
    pub(super) line: u32,
}

/// The shape of a typed expression.
#[derive(Debug, Clone)]
#[allow(dead_code)] // until emission consumes the tree (tasks 2.2, 3.1)
pub(super) enum TypedExprKind {
    Int(u64),
    Float(f64),
    Char(char),
    Bool(bool),
    Nil,
    Text(Vec<TextPart>),
    Ident(String),
    /// A global or constant of another module: `module.name`.
    Global { module: String, name: String },
    List(Vec<TypedExpr>),
    Map(Vec<(TypedExpr, TypedExpr)>),
    Record { rec_key: String, fields: Vec<(String, TypedExpr)> },
    New(Box<TypedExpr>),
    Shared(Box<TypedExpr>),
    Lambda { params: Vec<Param>, body: Vec<TypedStmt> },
    If { branches: Vec<(TypedExpr, Vec<TypedStmt>)>, else_body: Option<Vec<TypedStmt>> },
    Match { scrutinee: Box<TypedExpr>, arms: Vec<TypedMatchArm> },
    Unary { op: &'static str, expr: Box<TypedExpr> },
    Binary { op: &'static str, lhs: Box<TypedExpr>, rhs: Box<TypedExpr> },
    Field { expr: Box<TypedExpr>, name: String, safe: bool },
    Index { expr: Box<TypedExpr>, index: Box<TypedExpr> },
    Call { callee: TypedCallee, args: Vec<TypedArg> },
    Deref(Box<TypedExpr>),
    Propagate(Box<TypedExpr>),
    Await(Box<TypedExpr>),
    SpawnThread(Box<TypedExpr>),
    Sql { dialect: String, raw: String },
}

/// An arm of a `match`: its patterns as written and its typed body (the variables the patterns
/// bind are in scope there).
#[allow(dead_code)] // until emission consumes the tree (tasks 2.2, 3.1)
#[derive(Debug, Clone)]
pub(super) struct TypedMatchArm {
    pub(super) pattern: Vec<PatAtom>,
    pub(super) body: Vec<TypedStmt>,
}

/// An argument of a call, bound to its parameter and typed from the parameter's type.
#[allow(dead_code)] // until emission consumes the tree (tasks 2.2, 3.1)
#[derive(Debug, Clone)]
pub(super) struct TypedArg {
    /// Position of the parameter it binds to.
    pub(super) param: usize,
    pub(super) by_ref: bool,
    pub(super) value: TypedExpr,
}

/// The resolved call destination stored on a `TypedExprKind::Call` node.
#[derive(Debug, Clone)]
#[allow(dead_code)] // until emission consumes the tree (tasks 2.2, 3.1)
pub(super) enum TypedCallee {
    /// A named function in the given module: `module.name(args)` or `name(args)` in the same module.
    Func { module: String, name: String },
    /// A method on a concrete record type: `rec_key` is the record key (e.g. `"pkg.Foo"`).
    Method { rec_key: String, name: String },
    /// A generic function already instantiated: instance symbol is the final assembly label.
    GenericInstance { inst_name: String },
    /// A built-in intrinsic (`print`, `abort`, `channel`, …).
    Builtin(String),
    /// A variant of an enum used as a constructor: `enum_key` and the variant's index.
    Variant { enum_key: String, index: usize },
    /// A method of a built-in type (`list.push(x)`, `text.slice(a, b)`): the typed receiver.
    Member { receiver: Box<TypedExpr>, name: String },
    /// A local variable of function type (closure call).
    Local(String),
    /// Any call form not yet migrated; emission falls back to its own resolution.
    Opaque(Box<Expr>),
}

// ---------------------------------------------------------------------------
// Typed statement tree
// ---------------------------------------------------------------------------

/// A fully-typed statement node.
#[derive(Debug, Clone)]
#[allow(dead_code)] // until emission consumes the tree (tasks 2.2, 3.1)
pub(super) struct TypedStmt {
    pub(super) kind: TypedStmtKind,
    pub(super) line: u32,
}

/// The shape of a typed statement.
#[derive(Debug, Clone)]
#[allow(dead_code)] // until emission consumes the tree (tasks 2.2, 3.1)
pub(super) enum TypedStmtKind {
    Var { name: String, ty: Ty, value: Option<TypedExpr>, mutable: bool },
    Assign { target: TypedExpr, op: &'static str, value: TypedExpr },
    Return(Option<TypedExpr>),
    Defer(TypedExpr),
    Spawn(TypedExpr),
    For { vars: Vec<String>, iter: TypedExpr, body: Vec<TypedStmt> },
    While { cond: TypedExpr, body: Vec<TypedStmt> },
    Break,
    Continue,
    Expr(TypedExpr),
    Unsafe(Vec<TypedStmt>),
    Asm(AsmBlock),
    /// A statement the checker does not cover yet: emission handles it from the syntax.
    Opaque(Box<Stmt>),
}

/// A type-checked function body: the sequence of typed statements and the declared return type.
#[derive(Debug, Clone)]
#[allow(dead_code)] // until emission consumes the tree (tasks 2.2, 3.1)
pub(super) struct TypedBody {
    pub(super) stmts: Vec<TypedStmt>,
    /// The declared return type of the function (`Ty::Void` for `-> void` or no return clause).
    pub(super) ret: Ty,
    /// No statement of the body is `Opaque`.
    pub(super) complete: bool,
}

// ---------------------------------------------------------------------------
// Declaration tables
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(super) struct TypedProgram {
    pub(super) rec_decls: HashMap<String, (String, Vec<Field>)>,
    pub(super) records: HashMap<String, RecInfo>,
    pub(super) enum_decls: HashMap<String, (String, Vec<Variant>)>,
    pub(super) enums: HashMap<String, EnumInfo>,
    /// (module, variant name) -> (enum, index)
    pub(super) variants: HashMap<(String, String), (String, usize)>,
    pub(super) aliases: HashMap<String, (String, Type)>,
    /// `#[shared]` records have reference semantics: never copied on assignment, capture or iteration.
    pub(super) shared_recs: HashSet<String>,
    /// records that `new` allocates (they get a `B_rcdrop_*` wrapper); never rolled back
    pub(super) rc_recs: HashSet<String>,
    pub(super) enum_payload: HashSet<String>,
    /// qualified names of the types written with `pub`
    pub(super) pub_types: HashSet<String>,
    /// Interfaces: qualified name -> (module, method signatures).
    pub(super) interfaces: HashMap<String, (String, Vec<MethodSig>)>,
    pub(super) funcs: HashMap<(String, String), Sig>,
    pub(super) methods: HashMap<(String, String), Sig>,
    pub(super) globals: HashMap<(String, String), Global>,
    /// Methods the compiler wrote itself (derived `eq`, `compare`, `hash`): (type key, name).
    pub(super) derived: HashSet<(String, String)>,
    /// functions marked `#[export]`
    pub(super) exported: std::collections::HashSet<String>,
    // ---- generics: what the instantiations of the whole program decided ----
    /// Instance of a generic record -> (template, type arguments).
    pub(super) inst_args: HashMap<String, (String, Vec<Ty>)>,
    /// (module, function, type arguments) -> instance name.
    pub(super) inst_funcs: HashMap<(String, String, String), String>,
    /// Instances waiting to be compiled.
    pub(super) pending: Vec<Pending>,
    pub(super) inst_count: usize,
    /// Hash and equality thunks of general map keys, by key type: (hash symbol, equality symbol).
    pub(super) map_thunks: BTreeMap<String, (String, String)>,
    /// Checked function bodies by symbol (see `bodies.rs`).
    pub(super) bodies: HashMap<String, TypedBody>,
}
