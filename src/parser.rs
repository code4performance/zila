//! Recursive-descent parser for Brasa (see `brasa.ebnf`).
//!
//! Newline handling:
//! - A newline ends a statement or declaration.
//! - Newlines are skipped inside `( )`, `[ ]`, `{ }` lists, after binary
//!   operators and after `=`.
//! - A line starting with `.` or `?.` continues the previous expression
//!   (method chains).

use crate::ast::*;
use crate::lexer::*;
use std::collections::{BTreeMap, BTreeSet};

type R<T> = Result<T, Diag>;

pub fn parse_source(src: &str) -> R<File> {
    parse_source_with(src, crate::comptime::block_names(src))
}

/// `parse_source` with the names that start a block form given (instead of found by the
/// pre-scan of the `use` lines).
pub fn parse_source_with(src: &str, blocks: std::collections::HashSet<String>) -> R<File> {
    let toks = Lexer::new(src).with_blocks(blocks).tokenize()?;
    Parser::new(toks).parse_file()
}

/// Where the blocks of a file are, by token index, for tools that rewrite source.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// The token that opens a block -> the `end` token that closes it.
    pub blocks: BTreeMap<usize, usize>,
    /// The `else` tokens of `if` chains.
    pub elses: BTreeSet<usize>,
    /// Multi-line bodies of `match` arms: (the `->` token, the token after the body).
    pub arms: Vec<(usize, usize)>,
}

/// Parses `src` and also returns the tokens, the comments and where the blocks are.
pub fn parse_with_layout(src: &str) -> R<(File, Layout, Vec<Token>, Vec<Comment>)> {
    let (toks, comments) = Lexer::new(src).with_blocks(crate::comptime::block_names(src)).tokenize_all()?;
    let mut p = Parser::new(toks.clone());
    let file = p.parse_file()?;
    Ok((file, p.layout, toks, comments))
}

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    layout: Layout,
}

const ASSIGN_OPS: &[&str] = &["=", "+=", "-=", "*=", "/=", "%="];
const CMP_OPS: &[&str] = &["==", "!=", "<", "<=", ">", ">="];

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser { toks, pos: 0, layout: Layout::default() }
    }

    /// Consumes the `end` that closes the block opened at token `open`.
    fn end_block(&mut self, open: usize) -> R<()> {
        let at = self.pos;
        self.expect_kw("end")?;
        self.layout.blocks.insert(open, at);
        Ok(())
    }

    /// Like `end_block`, when the next token is `end`.
    fn eat_end(&mut self, open: usize) -> bool {
        if self.is_kw("end") {
            let at = self.pos;
            self.bump();
            self.layout.blocks.insert(open, at);
            true
        } else {
            false
        }
    }

    // ---------- token helpers ----------

    fn cur(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn line(&self) -> u32 {
        self.toks[self.pos].line
    }

    fn bump(&mut self) {
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
    }

    fn err<T>(&self, msg: impl Into<String>) -> R<T> {
        let t = &self.toks[self.pos];
        Err(Diag { line: t.line, col: t.col, msg: msg.into() })
    }

    fn describe(&self) -> String {
        match self.cur() {
            Tok::Ident(s) => format!("identifier '{}'", s),
            Tok::Kw(k) => format!("'{}'", k),
            Tok::Op(o) => format!("'{}'", o),
            Tok::Int(_) | Tok::Float(_) => "a number".into(),
            Tok::Char(_) => "a char literal".into(),
            Tok::Text(_) => "a text literal".into(),
            Tok::Newline => "end of line".into(),
            Tok::Eof => "end of file".into(),
            Tok::Block { name, .. } => format!("block '{}'", name),
        }
    }

    fn is_op(&self, s: &str) -> bool {
        matches!(self.cur(), Tok::Op(o) if *o == s)
    }

    fn is_kw(&self, s: &str) -> bool {
        matches!(self.cur(), Tok::Kw(k) if *k == s)
    }

    fn kw(&self) -> Option<&'static str> {
        match self.cur() {
            Tok::Kw(k) => Some(*k),
            _ => None,
        }
    }

    fn eat_op(&mut self, s: &str) -> bool {
        if self.is_op(s) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, s: &str) -> bool {
        if self.is_kw(s) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_op(&mut self, s: &str) -> R<()> {
        if self.eat_op(s) {
            Ok(())
        } else {
            self.err(format!("expected '{}', found {}", s, self.describe()))
        }
    }

    fn expect_kw(&mut self, s: &str) -> R<()> {
        if self.eat_kw(s) {
            Ok(())
        } else {
            self.err(format!("expected '{}', found {}", s, self.describe()))
        }
    }

    fn expect_ident(&mut self) -> R<String> {
        if let Tok::Ident(s) = self.cur() {
            let s = s.clone();
            self.bump();
            Ok(s)
        } else {
            self.err(format!("expected identifier, found {}", self.describe()))
        }
    }

    /// Identifier or keyword (method and field names may be keywords: `app.use(...)`).
    fn expect_name(&mut self) -> R<String> {
        match self.cur() {
            Tok::Ident(s) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            Tok::Kw(k) => {
                let s = k.to_string();
                self.bump();
                Ok(s)
            }
            _ => self.err(format!("expected a name, found {}", self.describe())),
        }
    }

    fn skip_nl(&mut self) {
        while matches!(self.cur(), Tok::Newline) {
            self.bump();
        }
    }

    /// End of a statement or declaration: newline, or a block terminator.
    fn stmt_end(&mut self) -> R<()> {
        match self.cur() {
            Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::Eof | Tok::Kw("end") | Tok::Kw("else") => Ok(()),
            _ => self.err(format!("expected end of line, found {}", self.describe())),
        }
    }

    fn at_stmt_boundary(&self) -> bool {
        matches!(self.cur(), Tok::Newline | Tok::Eof | Tok::Kw("end") | Tok::Kw("else"))
    }

    /// Consume a closing `>`, splitting `>>` and `>=` when generics close.
    fn expect_gt(&mut self) -> R<()> {
        match self.cur() {
            Tok::Op(">") => {
                self.bump();
                Ok(())
            }
            Tok::Op(">>") => {
                self.toks[self.pos].tok = Tok::Op(">");
                Ok(())
            }
            Tok::Op(">=") => {
                self.toks[self.pos].tok = Tok::Op("=");
                Ok(())
            }
            _ => self.err(format!("expected '>', found {}", self.describe())),
        }
    }

    // ---------- file and declarations ----------

    pub fn parse_file(&mut self) -> R<File> {
        self.skip_nl();
        self.expect_kw("package")?;
        let package = self.expect_ident()?;
        self.stmt_end()?;
        let mut uses = Vec::new();
        loop {
            self.skip_nl();
            if self.eat_kw("use") {
                loop {
                    uses.push(self.qname()?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.stmt_end()?;
            } else {
                break;
            }
        }
        let mut decls = Vec::new();
        loop {
            self.skip_nl();
            if matches!(self.cur(), Tok::Eof) {
                break;
            }
            decls.push(self.decl()?);
            self.stmt_end()?;
        }
        Ok(File { package, uses, decls })
    }

    fn qname(&mut self) -> R<Vec<String>> {
        let mut v = vec![self.expect_ident()?];
        while self.is_op(".") && matches!(self.at(1), Tok::Ident(_)) {
            self.bump();
            v.push(self.expect_ident()?);
        }
        Ok(v)
    }

    fn attrs(&mut self) -> R<Vec<Attr>> {
        let mut v = Vec::new();
        while self.eat_op("#[") {
            let name = self.expect_name()?;
            let mut args = Vec::new();
            if self.eat_op("(") {
                while !self.is_op(")") {
                    args.push(self.expr()?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
            }
            self.expect_op("]")?;
            if name == "generated" && !(args.len() == 1 && path_of(&args[0]).is_some()) {
                return self.err("`#[generated]` needs the name of a comptime function: #[generated(name)]");
            }
            v.push(Attr { name, args });
            // attributes may be stacked on separate lines
            if matches!(self.cur(), Tok::Newline) && matches!(self.at(1), Tok::Op("#[")) {
                self.bump();
            }
        }
        Ok(v)
    }

    fn decl(&mut self) -> R<Decl> {
        let attrs = self.attrs()?;
        self.skip_nl();
        let line = self.line();
        let public = self.eat_kw("pub");
        if public && self.is_kw("embed") {
            return self.err("'pub' cannot be applied to 'embed'");
        }
        if public && (self.is_kw("use") || self.is_kw("package")) {
            return self.err(format!("'pub' cannot be applied to '{}'", self.kw().unwrap()));
        }
        let kind = match self.kw() {
            Some("func") => DeclKind::Func(self.func_decl(false, attrs.iter().any(|a| a.name == "generated"))?),
            Some("comptime") => {
                self.bump();
                if !self.is_kw("func") {
                    return self.err(format!("'comptime' applies only to functions, found {} after it", self.describe()));
                }
                let mut f = self.func_decl(false, false)?;
                f.comptime = true;
                DeclKind::Func(f)
            }
            Some("async") => {
                self.bump();
                if !self.is_kw("func") {
                    return self.err(format!("expected 'func' after 'async', found {}", self.describe()));
                }
                DeclKind::Func(self.func_decl(true, false)?)
            }
            Some("type") => DeclKind::Type(self.type_decl()?),
            Some("let") | Some("var") => DeclKind::Var(self.var_decl()?),
            Some("extern") => DeclKind::Extern(self.extern_decl()?),
            Some("embed") => {
                self.bump();
                match self.cur().clone() {
                    Tok::Text(parts) => {
                        let mut s = String::new();
                        for p in parts {
                            match p {
                                StrPart::Lit(l) => s.push_str(&l),
                                StrPart::Interp { .. } => {
                                    return self.err("embed path must be a plain text literal")
                                }
                            }
                        }
                        self.bump();
                        DeclKind::Embed(s)
                    }
                    _ => return self.err("expected a text literal after 'embed'"),
                }
            }
            _ => return self.err(format!("expected a declaration, found {}", self.describe())),
        };
        let mut kind = kind;
        if let DeclKind::Func(f) = &mut kind {
            f.public = public;
        }
        Ok(Decl { attrs, public, kind, line })
    }

    fn var_decl(&mut self) -> R<VarDecl> {
        let mutable = self.is_kw("var");
        self.bump();
        let name = self.expect_ident()?;
        let ty = if self.eat_op(":") { Some(self.ty()?) } else { None };
        let value = if self.eat_op("=") {
            self.skip_nl();
            Some(self.expr()?)
        } else {
            None
        };
        if !mutable && value.is_none() {
            return self.err("'let' requires an initial value");
        }
        Ok(VarDecl { name, ty, value, mutable })
    }

    fn extern_decl(&mut self) -> R<ExternDecl> {
        self.expect_kw("extern")?;
        let abi = self.abi_string()?.unwrap_or_else(|| "C".to_string());
        self.expect_kw("func")?;
        let name = self.expect_ident()?;
        self.expect_op("(")?;
        let (params, c_variadic) = self.extern_params()?;
        self.expect_op(")")?;
        let ret = if self.eat_op("->") { Some(self.ty()?) } else { None };
        Ok(ExternDecl { abi, name, params, c_variadic, ret })
    }

    /// Optional ABI string after `extern`: `extern "C" func ...`
    fn abi_string(&mut self) -> R<Option<String>> {
        if matches!(self.cur(), Tok::Text(_)) {
            Ok(Some(self.plain_text("calling convention")?))
        } else {
            Ok(None)
        }
    }

    /// A text literal without interpolation.
    fn plain_text(&mut self, what: &str) -> R<String> {
        match self.cur().clone() {
            Tok::Text(parts) => {
                let mut s = String::new();
                for p in parts {
                    match p {
                        StrPart::Lit(l) => s.push_str(&l),
                        StrPart::Interp { .. } => {
                            return self.err(format!("{} must be a plain text literal", what))
                        }
                    }
                }
                self.bump();
                Ok(s)
            }
            _ => self.err(format!("expected a text literal for {}, found {}", what, self.describe())),
        }
    }

    /// Parameters of an extern function; a trailing `...` means C varargs.
    fn extern_params(&mut self) -> R<(Vec<Param>, bool)> {
        let mut v = Vec::new();
        let mut variadic = false;
        self.skip_nl();
        while !self.is_op(")") {
            if self.eat_op("...") {
                variadic = true;
                self.skip_nl();
                break;
            }
            v.push(self.param(false)?);
            self.skip_nl();
            if !self.eat_op(",") {
                break;
            }
            self.skip_nl();
        }
        Ok((v, variadic))
    }

    fn generics(&mut self) -> R<Vec<TypeParam>> {
        let mut v = Vec::new();
        if self.eat_op("<") {
            loop {
                let name = self.expect_ident()?;
                let bound = if self.eat_op(":") { Some(self.qname()?) } else { None };
                v.push(TypeParam { name, bound });
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_gt()?;
        }
        Ok(v)
    }

    fn type_decl(&mut self) -> R<TypeDecl> {
        self.expect_kw("type")?;
        let name = self.expect_ident()?;
        let generics = self.generics()?;
        self.expect_op("=")?;
        let body = match self.kw() {
            Some("record") => {
                let open = self.pos;
                self.bump();
                let mut fields = Vec::new();
                loop {
                    self.skip_nl();
                    if self.eat_end(open) {
                        break;
                    }
                    let attrs = self.attrs()?;
                    if !attrs.is_empty() {
                        self.skip_nl();
                    }
                    let line = self.line();
                    let public = self.eat_kw("pub");
                    let name = self.expect_ident()?;
                    self.expect_op(":")?;
                    let ty = self.ty()?;
                    fields.push(Field { attrs, public, name, ty, line });
                    self.eat_op(",");
                    self.stmt_end()?;
                }
                TypeBody::Record(fields)
            }
            Some("enum") => {
                let open = self.pos;
                self.bump();
                let mut variants = Vec::new();
                loop {
                    self.skip_nl();
                    if self.eat_end(open) {
                        break;
                    }
                    let attrs = self.attrs()?;
                    if !attrs.is_empty() {
                        self.skip_nl();
                    }
                    let line = self.line();
                    if self.is_kw("pub") {
                        return self.err("'pub' cannot be applied to an enum variant; a 'pub' enum exports all its variants");
                    }
                    let name = self.expect_ident()?;
                    let mut fields = Vec::new();
                    let mut field_names = Vec::new();
                    if self.eat_op("(") {
                        loop {
                            if matches!(self.cur(), Tok::Ident(_)) && matches!(self.at(1), Tok::Op(":")) {
                                field_names.push(self.expect_ident()?);
                                self.bump();
                            } else {
                                field_names.push(String::new());
                            }
                            fields.push(self.ty()?);
                            if !self.eat_op(",") {
                                break;
                            }
                        }
                        self.expect_op(")")?;
                    }
                    variants.push(Variant { attrs, name, fields, field_names, line });
                    if !self.eat_op(",") && !matches!(self.cur(), Tok::Newline) {
                        return self.err(format!("expected ',' or a new line after a variant, found {}", self.describe()));
                    }
                }
                TypeBody::Enum(variants)
            }
            Some("interface") => {
                let open = self.pos;
                self.bump();
                let mut sigs = Vec::new();
                loop {
                    self.skip_nl();
                    if self.eat_end(open) {
                        break;
                    }
                    if self.is_kw("pub") {
                        return self.err("'pub' cannot be applied to an interface method signature");
                    }
                    self.expect_kw("func")?;
                    let name = self.expect_name()?;
                    self.expect_op("(")?;
                    self.expect_kw("self")?;
                    let mut params = Vec::new();
                    if self.eat_op(",") {
                        params = self.params(false)?;
                    }
                    self.expect_op(")")?;
                    let ret = if self.eat_op("->") { Some(self.result()?) } else { None };
                    sigs.push(MethodSig { name, params, ret });
                    self.stmt_end()?;
                }
                TypeBody::Interface(sigs)
            }
            _ => TypeBody::Alias(self.ty()?),
        };
        Ok(TypeDecl { name, generics, body })
    }

    /// `body_less`: a `#[generated]` function has no body; the generator writes it.
    fn func_decl(&mut self, is_async: bool, body_less: bool) -> R<FuncDecl> {
        let open = self.pos;
        self.expect_kw("func")?;
        let receiver = if self.eat_op("(") {
            let by_ref = self.eat_kw("ref");
            let name = self.expect_ident()?;
            self.expect_op(":")?;
            let ty = self.ty()?;
            self.expect_op(")")?;
            Some(Receiver { by_ref, name, ty })
        } else {
            None
        };
        let name = if receiver.is_some() { self.expect_name()? } else { self.expect_ident()? };
        let generics = self.generics()?;
        self.expect_op("(")?;
        let params = self.params(false)?;
        self.expect_op(")")?;
        let ret = if self.eat_op("->") { Some(self.result()?) } else { None };
        if body_less {
            return Ok(FuncDecl { receiver, name, generics, params, ret, body: Vec::new(), is_async, comptime: false, public: false });
        }
        let body = self.block()?;
        self.end_block(open)?;
        Ok(FuncDecl { receiver, name, generics, params, ret, body, is_async, comptime: false, public: false })
    }

    /// Parameters up to (not including) `)`. Lambdas may omit types.
    fn params(&mut self, lambda: bool) -> R<Vec<Param>> {
        let mut v = Vec::new();
        self.skip_nl();
        while !self.is_op(")") {
            v.push(self.param(lambda)?);
            self.skip_nl();
            if !self.eat_op(",") {
                break;
            }
            self.skip_nl();
        }
        Ok(v)
    }

    fn param(&mut self, lambda: bool) -> R<Param> {
        if self.is_kw("pub") {
            return self.err("'pub' cannot be applied to a parameter");
        }
        let comptime = self.eat_kw("comptime");
        if comptime && lambda {
            return self.err("'comptime' parameters are only allowed in function declarations");
        }
        let by_ref = self.eat_kw("ref");
        let name = self.expect_ident()?;
        let mut variadic = false;
        let mut ty = None;
        if self.eat_op(":") {
            variadic = self.eat_op("...");
            ty = Some(self.ty()?);
        } else if !lambda {
            return self.err(format!("expected ':' and a type for parameter '{}'", name));
        }
        let default = if self.eat_op("=") { Some(self.expr()?) } else { None };
        Ok(Param { by_ref, name, ty, variadic, default, comptime })
    }

    // ---------- types ----------

    /// `T`, `T or error` or `error`
    fn result(&mut self) -> R<RetType> {
        let ty = self.ty()?;
        if matches!(&ty, Type::Named { path, args } if path.len() == 1 && path[0] == "error" && args.is_empty()) {
            return Ok(RetType { ty: None, fallible: true });
        }
        if self.eat_kw("or") {
            match self.cur().clone() {
                Tok::Ident(n) if n == "error" => {
                    self.bump();
                }
                _ => return self.err("expected 'error' after 'or' in a result type"),
            }
            return Ok(RetType { ty: Some(ty), fallible: true });
        }
        Ok(RetType { ty: Some(ty), fallible: false })
    }

    /// Speculatively parse `<T, U>` followed by `(` at a call site. On success the tokens up to (not
    /// including) the `(` are consumed; otherwise nothing is consumed and `None` is returned.
    fn try_type_args(&mut self) -> Option<Vec<Type>> {
        let pos = self.pos;
        // `f<T>(x)` has no spaces around the brackets; `a < b, c > (d)` does, and stays a comparison
        if pos == 0 || self.toks[pos].span.0 != self.toks[pos - 1].span.1 {
            return None;
        }
        let end = (pos + 64).min(self.toks.len());
        let saved: Vec<Token> = self.toks[pos..end].to_vec();
        let r = (|| -> R<Vec<Type>> {
            self.expect_op("<")?;
            let mut v = vec![self.ty()?];
            while self.eat_op(",") {
                v.push(self.ty()?);
            }
            self.expect_gt()?;
            if !self.is_op("(") || self.toks[self.pos].span.0 != self.toks[self.pos - 1].span.1 {
                return self.err("not a call");
            }
            Ok(v)
        })();
        if r.is_err() {
            self.pos = pos;
            self.toks.splice(pos..end, saved);
        }
        r.ok()
    }

    fn ty(&mut self) -> R<Type> {
        let t = self.base_type()?;
        if self.eat_op("?") {
            Ok(Type::Optional(Box::new(t)))
        } else {
            Ok(t)
        }
    }

    fn func_type(&mut self, abi: Option<String>) -> R<Type> {
        self.expect_kw("func")?;
        self.expect_op("(")?;
        let mut params = Vec::new();
        while !self.is_op(")") {
            // optional parameter name: `func(Request, next: Handler)`
            if matches!(self.cur(), Tok::Ident(_)) && matches!(self.at(1), Tok::Op(":")) {
                self.bump();
                self.bump();
            }
            params.push(self.ty()?);
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_op(")")?;
        let ret = if self.eat_op("->") { Some(Box::new(self.result()?)) } else { None };
        Ok(Type::Func { abi, params, ret })
    }

    fn base_type(&mut self) -> R<Type> {
        match self.cur().clone() {
            Tok::Op("*") => {
                self.bump();
                Ok(Type::Pointer(Box::new(self.ty()?)))
            }
            Tok::Op("[") => {
                self.bump();
                if self.eat_op("]") {
                    return Ok(Type::Slice(Box::new(self.ty()?)));
                }
                match self.cur().clone() {
                    Tok::Int(n) => {
                        self.bump();
                        self.expect_op("]")?;
                        Ok(Type::Array(n, Box::new(self.ty()?)))
                    }
                    _ => self.err("expected ']' or an array size"),
                }
            }
            Tok::Kw("shared") => {
                self.bump();
                Ok(Type::Shared(Box::new(self.ty()?)))
            }
            Tok::Kw("func") => self.func_type(None),
            Tok::Kw("extern") => {
                self.bump();
                let abi = self.abi_string()?.unwrap_or_else(|| "C".to_string());
                self.func_type(Some(abi))
            }
            Tok::Ident(_) => {
                let path = self.qname()?;
                let mut args = Vec::new();
                if self.eat_op("<") {
                    loop {
                        args.push(self.ty()?);
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    self.expect_gt()?;
                }
                Ok(Type::Named { path, args })
            }
            _ => self.err(format!("expected a type, found {}", self.describe())),
        }
    }

    // ---------- statements ----------

    fn block(&mut self) -> R<Block> {
        let mut v = Vec::new();
        loop {
            self.skip_nl();
            if matches!(self.cur(), Tok::Eof) || self.is_kw("end") || self.is_kw("else") {
                break;
            }
            v.push(self.stmt()?);
            self.stmt_end()?;
        }
        Ok(v)
    }

    fn stmt(&mut self) -> R<Stmt> {
        let line = self.line();
        let kind = match self.kw() {
            Some("let") | Some("var") => StmtKind::Var(self.var_decl()?),
            Some("return") => {
                self.bump();
                if self.at_stmt_boundary() {
                    StmtKind::Return(None)
                } else {
                    StmtKind::Return(Some(self.expr()?))
                }
            }
            Some("defer") => {
                self.bump();
                StmtKind::Defer(self.expr()?)
            }
            Some("spawn") => {
                self.bump();
                let e = self.expr()?;
                if !matches!(e.kind, ExprKind::Call { .. }) {
                    return Err(Diag { line, col: 1, msg: "'spawn' requires a function call".into() });
                }
                StmtKind::Spawn(e)
            }
            Some("unsafe") => {
                let open = self.pos;
                self.bump();
                let b = self.block()?;
                self.end_block(open)?;
                StmtKind::Unsafe(b)
            }
            Some("asm") => StmtKind::Asm(self.asm_block()?),
            None if matches!(self.cur(), Tok::Block { .. }) && matches!(self.at(1), Tok::Newline | Tok::Eof) => {
                let Tok::Block { name, raw } = self.cur().clone() else { unreachable!() };
                self.bump();
                StmtKind::Block { name, raw }
            }
            Some("for") => {
                let open = self.pos;
                self.bump();
                let mut vars = vec![self.expect_ident()?];
                if self.eat_op(",") {
                    vars.push(self.expect_ident()?);
                }
                self.expect_kw("in")?;
                let iter = self.expr()?;
                let body = self.block()?;
                self.end_block(open)?;
                StmtKind::For { vars, iter, body }
            }
            Some("while") => {
                let open = self.pos;
                self.bump();
                let cond = self.expr()?;
                let body = self.block()?;
                self.end_block(open)?;
                StmtKind::While { cond, body }
            }
            Some("break") => {
                self.bump();
                StmtKind::Break
            }
            Some("continue") => {
                self.bump();
                StmtKind::Continue
            }
            _ => {
                let e = self.expr()?;
                let op = match self.cur() {
                    Tok::Op(o) if ASSIGN_OPS.contains(o) => Some(*o),
                    _ => None,
                };
                match op {
                    Some(op) => {
                        if !is_place(&e) {
                            return self.err("invalid assignment target");
                        }
                        self.bump();
                        self.skip_nl();
                        let value = self.expr()?;
                        StmtKind::Assign { target: e, op, value }
                    }
                    None => StmtKind::Expr(e),
                }
            }
        };
        Ok(Stmt { kind, line })
    }

    /// `asm` block: text lines, then `in|out|inout name = expr` and `clobber "reg", ...`.
    fn asm_block(&mut self) -> R<AsmBlock> {
        let open = self.pos;
        self.expect_kw("asm")?;
        let (mut lines, mut operands, mut clobbers) = (Vec::new(), Vec::new(), Vec::new());
        loop {
            self.skip_nl();
            if self.eat_end(open) {
                break;
            }
            match self.cur().clone() {
                Tok::Eof => return self.err("unterminated 'asm' block: expected 'end'"),
                Tok::Text(_) => lines.push(self.plain_text("an assembly line")?),
                Tok::Kw("in") => {
                    self.bump();
                    operands.push(self.asm_operand(AsmDir::In)?);
                }
                Tok::Ident(n) if n == "out" || n == "inout" => {
                    self.bump();
                    let dir = if n == "out" { AsmDir::Out } else { AsmDir::InOut };
                    operands.push(self.asm_operand(dir)?);
                }
                Tok::Ident(n) if n == "clobber" => {
                    self.bump();
                    loop {
                        match self.cur().clone() {
                            Tok::Ident(name) => {
                                self.bump();
                                clobbers.push(name);
                            }
                            _ => clobbers.push(self.plain_text("a clobbered register")?),
                        }
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                }
                _ => {
                    return self.err(format!(
                        "in 'asm': expected an assembly line, 'in', 'out', 'inout', 'clobber' or 'end', found {}",
                        self.describe()
                    ))
                }
            }
            self.stmt_end()?;
        }
        Ok(AsmBlock { lines, operands, clobbers })
    }

    fn asm_operand(&mut self, dir: AsmDir) -> R<AsmOperand> {
        let name = self.expect_ident()?;
        self.expect_op("=")?;
        let value = self.expr()?;
        Ok(AsmOperand { dir, name, value })
    }

    // ---------- expressions ----------

    pub fn expr(&mut self) -> R<Expr> {
        self.coalesce()
    }

    fn bin(&mut self, ops: &[&'static str], next: fn(&mut Self) -> R<Expr>) -> R<Expr> {
        let mut lhs = next(self)?;
        loop {
            let op = match self.cur() {
                Tok::Op(o) if ops.contains(o) => *o,
                Tok::Kw(k) if ops.contains(k) => *k,
                _ => break,
            };
            let line = self.line();
            self.bump();
            self.skip_nl();
            let rhs = next(self)?;
            lhs = Expr {
                kind: ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) },
                line,
            };
        }
        Ok(lhs)
    }

    fn coalesce(&mut self) -> R<Expr> {
        self.bin(&["??"], Self::or_expr)
    }

    fn or_expr(&mut self) -> R<Expr> {
        self.bin(&["or"], Self::and_expr)
    }

    fn and_expr(&mut self) -> R<Expr> {
        self.bin(&["and"], Self::cmp_expr)
    }

    /// Comparisons do not chain: `a < b < c` is an error.
    fn cmp_expr(&mut self) -> R<Expr> {
        let lhs = self.range_expr()?;
        let op = match self.cur() {
            Tok::Op(o) if CMP_OPS.contains(o) => *o,
            _ => return Ok(lhs),
        };
        let line = self.line();
        self.bump();
        self.skip_nl();
        let rhs = self.range_expr()?;
        if matches!(self.cur(), Tok::Op(o) if CMP_OPS.contains(o)) {
            return self.err("comparison operators cannot be chained; use 'and'");
        }
        Ok(Expr { kind: ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }, line })
    }

    fn range_expr(&mut self) -> R<Expr> {
        let lhs = self.add_expr()?;
        if self.is_op("..") {
            let line = self.line();
            self.bump();
            let rhs = self.add_expr()?;
            return Ok(Expr {
                kind: ExprKind::Binary { op: "..", lhs: Box::new(lhs), rhs: Box::new(rhs) },
                line,
            });
        }
        Ok(lhs)
    }

    fn add_expr(&mut self) -> R<Expr> {
        self.bin(&["+", "-", "bitor", "bitxor"], Self::mul_expr)
    }

    fn mul_expr(&mut self) -> R<Expr> {
        self.bin(&["*", "/", "%", "<<", ">>", "bitand"], Self::unary)
    }

    fn unary(&mut self) -> R<Expr> {
        let line = self.line();
        let op = match self.cur() {
            Tok::Kw("not") => "not",
            Tok::Kw("bitnot") => "bitnot",
            Tok::Op("-") => "-",
            Tok::Op("&") => "&",
            Tok::Op("*") => "*",
            Tok::Kw("try") => {
                self.bump();
                let e = self.unary()?;
                return Ok(Expr { kind: ExprKind::Propagate(Box::new(e)), line });
            }
            Tok::Kw("await") => {
                self.bump();
                let e = self.unary()?;
                return Ok(Expr { kind: ExprKind::Await(Box::new(e)), line });
            }
            _ => return self.postfix(),
        };
        self.bump();
        let e = self.unary()?;
        Ok(Expr { kind: ExprKind::Unary { op, expr: Box::new(e) }, line })
    }

    fn postfix(&mut self) -> R<Expr> {
        let mut e = self.primary()?;
        loop {
            let line = self.line();
            // method chain continued on the next line
            if matches!(self.cur(), Tok::Newline)
                && matches!(self.at(1), Tok::Op(".") | Tok::Op("?."))
            {
                self.bump();
            }
            match self.cur() {
                Tok::Op(".") | Tok::Op("?.") => {
                    let safe = self.is_op("?.");
                    self.bump();
                    let name = self.expect_name()?;
                    e = Expr { kind: ExprKind::Field { expr: Box::new(e), name, safe }, line };
                }
                Tok::Op("[") => {
                    self.bump();
                    self.skip_nl();
                    let index = self.expr()?;
                    self.skip_nl();
                    self.expect_op("]")?;
                    e = Expr {
                        kind: ExprKind::Index { expr: Box::new(e), index: Box::new(index) },
                        line,
                    };
                }
                Tok::Op("(") => {
                    self.bump();
                    let args = self.args()?;
                    e = Expr { kind: ExprKind::Call { callee: Box::new(e), args, targs: Vec::new() }, line };
                }
                Tok::Op("<") if path_of(&e).is_some() => {
                    let Some(targs) = self.try_type_args() else { break };
                    self.bump();
                    let args = self.args()?;
                    e = Expr { kind: ExprKind::Call { callee: Box::new(e), args, targs }, line };
                }
                Tok::Op("^") => {
                    self.bump();
                    e = Expr { kind: ExprKind::Deref(Box::new(e)), line };
                }
                Tok::Op("?") => {
                    return self.err("postfix '?' was replaced by 'try': write `try call()`");
                }
                Tok::Op("{") if path_of(&e).is_some() => {
                    let path = path_of(&e).unwrap();
                    self.bump();
                    let fields = self.record_fields()?;
                    e = Expr { kind: ExprKind::Record { path, fields }, line };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn record_fields(&mut self) -> R<Vec<(String, Expr)>> {
        let mut fields = Vec::new();
        self.skip_nl();
        while !self.is_op("}") {
            let name = self.expect_name()?;
            self.expect_op(":")?;
            self.skip_nl();
            let value = self.expr()?;
            fields.push((name, value));
            self.skip_nl();
            if !self.eat_op(",") {
                break;
            }
            self.skip_nl();
        }
        self.expect_op("}")?;
        Ok(fields)
    }

    fn args(&mut self) -> R<Vec<Arg>> {
        let mut v = Vec::new();
        self.skip_nl();
        while !self.is_op(")") {
            v.push(self.arg()?);
            self.skip_nl();
            if !self.eat_op(",") {
                break;
            }
            self.skip_nl();
        }
        self.expect_op(")")?;
        Ok(v)
    }

    fn arg(&mut self) -> R<Arg> {
        let spread = self.eat_op("...");
        let by_ref = self.eat_kw("ref");
        let mut name = None;
        if let (Tok::Ident(n), Tok::Op(":")) = (self.cur(), self.at(1)) {
            name = Some(n.clone());
        }
        if name.is_some() {
            self.bump();
            self.bump();
            self.skip_nl();
        }
        let value = self.expr()?;
        Ok(Arg { name, by_ref, spread, value })
    }

    fn primary(&mut self) -> R<Expr> {
        let line = self.line();
        let kind = match self.cur().clone() {
            Tok::Int(n) => {
                self.bump();
                ExprKind::Int(n)
            }
            Tok::Float(f) => {
                self.bump();
                ExprKind::Float(f)
            }
            Tok::Char(c) => {
                self.bump();
                ExprKind::Char(c)
            }
            Tok::Text(parts) => {
                self.bump();
                ExprKind::Text(self.text_parts(parts)?)
            }
            Tok::Ident(n) => {
                self.bump();
                ExprKind::Ident(n)
            }
            Tok::Block { name, raw } => {
                self.bump();
                ExprKind::Block { name, raw }
            }
            Tok::Kw("true") => {
                self.bump();
                ExprKind::Bool(true)
            }
            Tok::Kw("false") => {
                self.bump();
                ExprKind::Bool(false)
            }
            Tok::Kw("nil") => {
                self.bump();
                ExprKind::Nil
            }
            Tok::Kw("func") => return self.lambda(),
            Tok::Kw("async") => {
                return self.err("lambdas cannot be async; declare an `async func` and call it");
            }
            Tok::Kw("if") => return self.if_expr(),
            Tok::Kw("match") => return self.match_expr(),
            Tok::Kw("shared") => {
                self.bump();
                let e = self.unary()?;
                ExprKind::Shared(Box::new(e))
            }
            Tok::Kw("spawn_thread") => {
                self.bump();
                let e = self.unary()?;
                if !matches!(e.kind, ExprKind::Call { .. }) {
                    return self.err("'spawn_thread' requires a function call, like `spawn_thread work(x)`");
                }
                ExprKind::SpawnThread(Box::new(e))
            }
            Tok::Kw("new") => {
                self.bump();
                let mut path = vec![self.expect_name()?];
                while self.is_op(".") {
                    self.bump();
                    path.push(self.expect_name()?);
                }
                if !self.is_op("{") {
                    return self.err(format!("'new' needs a record literal such as `new {}{{ ... }}`", path.join(".")));
                }
                self.bump();
                let fields = self.record_fields()?;
                ExprKind::New(Box::new(Expr { kind: ExprKind::Record { path, fields }, line }))
            }
            Tok::Op("(") => {
                self.bump();
                self.skip_nl();
                let e = self.expr()?;
                self.skip_nl();
                self.expect_op(")")?;
                return Ok(e);
            }
            Tok::Op("[") => {
                self.bump();
                let mut items = Vec::new();
                self.skip_nl();
                while !self.is_op("]") {
                    items.push(self.expr()?);
                    self.skip_nl();
                    if !self.eat_op(",") {
                        break;
                    }
                    self.skip_nl();
                }
                self.expect_op("]")?;
                ExprKind::List(items)
            }
            Tok::Op("{") => {
                self.bump();
                let mut pairs = Vec::new();
                self.skip_nl();
                while !self.is_op("}") {
                    let k = self.expr()?;
                    self.expect_op(":")?;
                    self.skip_nl();
                    let v = self.expr()?;
                    pairs.push((k, v));
                    self.skip_nl();
                    if !self.eat_op(",") {
                        break;
                    }
                    self.skip_nl();
                }
                self.expect_op("}")?;
                ExprKind::Map(pairs)
            }
            _ => return self.err(format!("expected an expression, found {}", self.describe())),
        };
        Ok(Expr { kind, line })
    }

    fn text_parts(&self, parts: Vec<StrPart>) -> R<Vec<TextPart>> {
        let mut out = Vec::new();
        for p in parts {
            match p {
                StrPart::Lit(s) => out.push(TextPart::Lit(s)),
                StrPart::Interp { src, line, col } => {
                    let toks = Lexer::at(&src, line, col).tokenize()?;
                    let mut sub = Parser::new(toks);
                    sub.skip_nl();
                    let e = sub.expr()?;
                    sub.skip_nl();
                    if !matches!(sub.cur(), Tok::Eof) {
                        return sub.err(format!(
                            "unexpected {} in text interpolation",
                            sub.describe()
                        ));
                    }
                    out.push(TextPart::Expr(e));
                }
            }
        }
        Ok(out)
    }

    fn lambda(&mut self) -> R<Expr> {
        let line = self.line();
        let open = self.pos;
        self.expect_kw("func")?;
        self.expect_op("(")?;
        let params = self.params(true)?;
        self.expect_op(")")?;
        if self.is_op("->") {
            return self.err("lambdas do not use '->'; write `func(x) x * 2 end`");
        }
        let body = self.block()?;
        self.end_block(open)?;
        Ok(Expr { kind: ExprKind::Lambda { params, body }, line })
    }

    fn if_expr(&mut self) -> R<Expr> {
        let line = self.line();
        let open = self.pos;
        self.expect_kw("if")?;
        let mut branches = Vec::new();
        let mut else_body = None;
        let cond = self.expr()?;
        let body = self.block()?;
        branches.push((cond, body));
        loop {
            let else_at = self.pos;
            if self.eat_kw("else") {
                self.layout.elses.insert(else_at);
                if self.eat_kw("if") {
                    let c = self.expr()?;
                    let b = self.block()?;
                    branches.push((c, b));
                    continue;
                }
                else_body = Some(self.block()?);
                self.end_block(open)?;
                break;
            } else {
                self.end_block(open)?;
                break;
            }
        }
        Ok(Expr { kind: ExprKind::If { branches, else_body }, line })
    }

    fn match_expr(&mut self) -> R<Expr> {
        let line = self.line();
        let open = self.pos;
        self.expect_kw("match")?;
        let scrutinee = Box::new(self.expr()?);
        let mut arms = Vec::new();
        loop {
            self.skip_nl();
            if self.eat_end(open) {
                break;
            }
            if matches!(self.cur(), Tok::Eof) {
                return self.err("unterminated 'match': expected 'end'");
            }
            let pattern = self.pattern()?;
            let arrow = self.pos;
            self.expect_op("->")?;
            let body = if matches!(self.cur(), Tok::Newline) {
                let b = self.arm_block()?;
                self.layout.arms.push((arrow, self.pos));
                b
            } else {
                let s = self.stmt()?;
                self.stmt_end()?;
                vec![s]
            };
            arms.push(MatchArm { pattern, body });
        }
        Ok(Expr { kind: ExprKind::Match { scrutinee, arms }, line })
    }

    /// Multi-line arm body: runs until the next arm or the closing `end`.
    fn arm_block(&mut self) -> R<Block> {
        let mut v = Vec::new();
        loop {
            self.skip_nl();
            if matches!(self.cur(), Tok::Eof) || self.is_kw("end") || self.is_arm_start() {
                break;
            }
            v.push(self.stmt()?);
            self.stmt_end()?;
        }
        Ok(v)
    }

    fn is_arm_start(&mut self) -> bool {
        let save = self.pos;
        let ok = self.pattern().is_ok() && self.is_op("->");
        self.pos = save;
        ok
    }

    fn pattern(&mut self) -> R<Vec<PatAtom>> {
        let mut v = vec![self.pat_atom()?];
        while self.eat_op("|") {
            v.push(self.pat_atom()?);
        }
        Ok(v)
    }

    fn pat_atom(&mut self) -> R<PatAtom> {
        let line = self.line();
        match self.cur().clone() {
            Tok::Ident(n) => {
                self.bump();
                if n == "_" && !self.is_op("(") {
                    return Ok(PatAtom::Wildcard);
                }
                if self.eat_op("(") {
                    let mut b = Vec::new();
                    while !self.is_op(")") {
                        b.push(self.expect_ident()?);
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    self.expect_op(")")?;
                    Ok(PatAtom::Name { name: n, bindings: Some(b) })
                } else {
                    Ok(PatAtom::Name { name: n, bindings: None })
                }
            }
            Tok::Int(_)
            | Tok::Float(_)
            | Tok::Char(_)
            | Tok::Text(_)
            | Tok::Kw("true")
            | Tok::Kw("false")
            | Tok::Kw("nil") => Ok(PatAtom::Literal(self.primary()?)),
            Tok::Op("-") => {
                self.bump();
                let e = self.primary()?;
                Ok(PatAtom::Literal(Expr {
                    kind: ExprKind::Unary { op: "-", expr: Box::new(e) },
                    line,
                }))
            }
            _ => self.err(format!("expected a pattern, found {}", self.describe())),
        }
    }
}

/// `a` or `a.b.c` made only of plain identifiers (a possible record type name).
fn path_of(e: &Expr) -> Option<Vec<String>> {
    match &e.kind {
        ExprKind::Ident(n) => Some(vec![n.clone()]),
        ExprKind::Field { expr, name, safe: false } => {
            let mut p = path_of(expr)?;
            p.push(name.clone());
            Some(p)
        }
        _ => None,
    }
}

fn is_place(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Ident(_) | ExprKind::Index { .. } | ExprKind::Deref(_) => true,
        ExprKind::Field { safe, .. } => !*safe,
        ExprKind::Unary { op: "*", .. } => true,
        _ => false,
    }
}
