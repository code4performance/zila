//! AST for Zila Lang. Every node that can be reported in an error carries a line, and the nodes
//! the editor tools point at carry a `Span` (start and end, line and column).

/// A range in the source: lines and columns count from 1; the end is just after the last
/// character. `Span::default()` (all zero) means "no position", for nodes made by the compiler.
///
/// `Debug` prints just `Span`, whatever the range: code that compares or hashes the debug text of
/// a tree (the generator cache key, the library parity tests) must not change when code moves.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl std::fmt::Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Span")
    }
}

impl Span {
    pub fn new(line: u32, col: u32, end_line: u32, end_col: u32) -> Span {
        Span { line, col, end_line, end_col }
    }

    /// True for a span that records no position.
    pub fn is_none(&self) -> bool {
        self.line == 0
    }

    /// From the start of `self` to the end of `other`.
    pub fn to(self, other: Span) -> Span {
        Span { line: self.line, col: self.col, end_line: other.end_line, end_col: other.end_col }
    }

    /// Whether the position (line, col) is inside the range (the end is not included).
    pub fn contains(&self, line: u32, col: u32) -> bool {
        !self.is_none() && (line, col) >= (self.line, self.col) && (line, col) < (self.end_line, self.end_col)
    }
}

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
    /// From `pub` or the keyword to the end of the declaration (attributes not included).
    pub span: Span,
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
    pub name_span: Span,
    pub ty: Option<Type>,
    pub value: Option<Expr>,
    /// `var` = true, `let` = false
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub receiver: Option<Receiver>,
    pub name: String,
    pub name_span: Span,
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
    pub name_span: Span,
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
    pub name_span: Span,
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
    pub name_span: Span,
    pub params: Vec<Param>,
    /// C variadic function: `printf(fmt: *c.char, ...)`
    pub c_variadic: bool,
    pub ret: Option<Type>,
}

#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub name: String,
    pub name_span: Span,
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
    pub name_span: Span,
    pub ty: Type,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub attrs: Vec<Attr>,
    pub name: String,
    pub name_span: Span,
    pub fields: Vec<Type>,
    /// The names of the payload fields (`Circle(r: float)`); empty text for a positional one.
    pub field_names: Vec<String>,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct MethodSig {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub ret: Option<RetType>,
}

#[derive(Debug, Clone)]
pub enum Type {
    /// `span` covers the name and its type arguments; it is empty for types the compiler makes.
    Named { path: Vec<String>, args: Vec<Type>, span: Span },
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
    pub span: Span,
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
    /// `var_spans` are the ranges of the names in `vars`.
    For { vars: Vec<String>, var_spans: Vec<Span>, iter: Expr, body: Block },
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
    /// The whole expression; for `a.b` the name `b` ends the span, for a call the `)` does.
    pub span: Span,
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
    /// `field_spans` are the ranges of the field names, in order (empty for a literal the compiler made).
    Record { path: Vec<String>, fields: Vec<(String, Expr)>, field_spans: Vec<Span> },
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
    /// `span` is the range of `name`, `binding_spans` those of the bindings.
    Name { name: String, bindings: Option<Vec<String>>, span: Span, binding_spans: Vec<Span> },
}
