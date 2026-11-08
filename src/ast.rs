//! AST for Brasa. Every node that can be reported in an error carries a line.

#[derive(Debug, Clone)]
pub struct File {
    pub package: String,
    pub uses: Vec<Vec<String>>,
    pub decls: Vec<Decl>,
}

#[derive(Debug, Clone)]
pub struct Attr {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub attrs: Vec<Attr>,
    pub public: bool,
    pub kind: DeclKind,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub enum DeclKind {
    Func(FuncDecl),
    Type(TypeDecl),
    Var(VarDecl),
    Extern(ExternDecl),
    Embed(String),
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub name: String,
    pub ty: Option<Type>,
    pub value: Option<Expr>,
    /// `var` = true, `let` = false
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub receiver: Option<Receiver>,
    pub name: String,
    pub generics: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub ret: Option<RetType>,
    pub body: Block,
    /// `async func`: runs as its own task and returns a `Future<T>`
    pub is_async: bool,
    /// `comptime func`: runs at build time and returns source text
    pub comptime: bool,
    /// written with `pub` (the parser copies `Decl::public` here)
    pub public: bool,
}

#[derive(Debug, Clone)]
pub struct Receiver {
    pub by_ref: bool,
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct TypeParam {
    pub name: String,
    /// Interface bound: `T: Printable`
    pub bound: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub by_ref: bool,
    pub name: String,
    /// Optional only for lambda parameters (inferred).
    pub ty: Option<Type>,
    pub variadic: bool,
    pub default: Option<Expr>,
    /// `comptime t: Type`: a type given at compile time
    pub comptime: bool,
}

/// Return type: `T`, `T!` or `!`.
#[derive(Debug, Clone)]
pub struct RetType {
    pub ty: Option<Type>,
    pub fallible: bool,
}

#[derive(Debug, Clone)]
pub struct ExternDecl {
    /// Calling convention: "C" (default), "stdcall", "system", ...
    pub abi: String,
    pub name: String,
    pub params: Vec<Param>,
    /// C variadic function: `printf(fmt: *c.char, ...)`
    pub c_variadic: bool,
    pub ret: Option<Type>,
}

#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub name: String,
    pub generics: Vec<TypeParam>,
    pub body: TypeBody,
}

#[derive(Debug, Clone)]
pub enum TypeBody {
    Record(Vec<Field>),
    /// Variants with optional payload: `Red`, `Circle(float)`.
    Enum(Vec<Variant>),
    Interface(Vec<MethodSig>),
    Alias(Type),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub attrs: Vec<Attr>,
    pub public: bool,
    pub name: String,
    pub ty: Type,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub attrs: Vec<Attr>,
    pub name: String,
    pub fields: Vec<Type>,
    /// The names of the payload fields (`Circle(r: float)`); empty text for a positional one.
    pub field_names: Vec<String>,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct MethodSig {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<RetType>,
}

#[derive(Debug, Clone)]
pub enum Type {
    Named { path: Vec<String>, args: Vec<Type> },
    Optional(Box<Type>),
    Pointer(Box<Type>),
    /// `shared T`: atomic reference counts, immutable content.
    Shared(Box<Type>),
    Slice(Box<Type>),
    Array(u64, Box<Type>),
    /// `abi` is set for `extern "C" func(...)` (callable from C).
    Func { abi: Option<String>, params: Vec<Type>, ret: Option<Box<RetType>> },
}

pub type Block = Vec<Stmt>;

/// Inline assembly. Template lines use `@name` for operands.
/// An operand named like a register (`eax`) binds that register.
#[derive(Debug, Clone)]
pub struct AsmBlock {
    pub lines: Vec<String>,
    pub operands: Vec<AsmOperand>,
    pub clobbers: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AsmOperand {
    pub dir: AsmDir,
    pub name: String,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AsmDir {
    In,
    Out,
    InOut,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Var(VarDecl),
    Assign { target: Expr, op: &'static str, value: Expr },
    Return(Option<Expr>),
    Defer(Expr),
    Spawn(Expr),
    Unsafe(Block),
    Asm(AsmBlock),
    For { vars: Vec<String>, iter: Expr, body: Block },
    While { cond: Expr, body: Block },
    Break,
    Continue,
    /// A block form standing alone on a line; replaced by statements before code generation.
    Block { name: String, raw: String },
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(u64),
    Float(f64),
    Char(char),
    Bool(bool),
    Nil,
    Text(Vec<TextPart>),
    Ident(String),
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    Record { path: Vec<String>, fields: Vec<(String, Expr)> },
    Lambda { params: Vec<Param>, body: Block },
    /// `if` / `else if` / `else`; usable as a statement or a value.
    If { branches: Vec<(Expr, Block)>, else_body: Option<Block> },
    Match { scrutinee: Box<Expr>, arms: Vec<MatchArm> },
    /// `new T{ ... }`: the operand is always an `ExprKind::Record`.
    New(Box<Expr>),
    /// `shared e`: `e` made visible to several threads (atomic counts, immutable content).
    Shared(Box<Expr>),
    /// `spawn_thread f(args)`: runs the call on a new OS thread and yields a `Thread`.
    SpawnThread(Box<Expr>),
    Unary { op: &'static str, expr: Box<Expr> },
    Binary { op: &'static str, lhs: Box<Expr>, rhs: Box<Expr> },
    /// `a.b` or, with `safe`, `a?.b`
    Field { expr: Box<Expr>, name: String, safe: bool },
    Index { expr: Box<Expr>, index: Box<Expr> },
    /// `f(args)`; `targs` holds explicit type arguments (`f<i32>(args)`).
    Call { callee: Box<Expr>, args: Vec<Arg>, targs: Vec<Type> },
    /// `p^`
    Deref(Box<Expr>),
    /// `try e` (propagate error)
    Propagate(Box<Expr>),
    /// `await e`: wait for a `Future<T>` without blocking other tasks
    Await(Box<Expr>),
    /// A block form used as an expression; replaced by the expansion before code generation.
    Block { name: String, raw: String },
    /// A `sql <dialect>` block (spec `sql-blocks`): the dialect name and the raw lines. The holes
    /// are read by the code generator, which knows the dialect.
    Sql { dialect: String, raw: String },
}

#[derive(Debug, Clone)]
pub enum TextPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct Arg {
    pub name: Option<String>,
    pub by_ref: bool,
    pub spread: bool,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    /// Alternatives joined by `|`.
    pub pattern: Vec<PatAtom>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub enum PatAtom {
    Wildcard,
    Literal(Expr),
    /// `Circle(r)` binds; a bare `Empty` has `bindings: None`.
    Name { name: String, bindings: Option<Vec<String>> },
}
