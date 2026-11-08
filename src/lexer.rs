//! Lexer for Zila Lang.
//!
//! Newlines are significant (they end statements). Consecutive newlines are
//! collapsed into one `Tok::Newline`. Text literals are split into literal
//! pieces and `{expr}` interpolations, which the parser sub-parses.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Diag {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.msg)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Lit(String),
    Interp { src: String, line: u32, col: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Kw(&'static str),
    Op(&'static str),
    Int(u64),
    Float(f64),
    Char(char),
    Text(Vec<StrPart>),
    /// A block form: `name`, raw lines, `end` (spec `comptime-reflection`). `raw` is the text
    /// between the name line and the `end` line, unchanged.
    Block { name: String, raw: String },
    /// A `sql <dialect>` block (spec `sql-blocks`): the dialect name and the raw lines up to the
    /// `end` line, unchanged. The first raw line is on the line after the token.
    Sql { dialect: String, raw: String },
    Newline,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
    /// Where the token is in the source, as character offsets (start, end).
    pub span: (usize, usize),
}

/// A `//` or `/* */` comment, kept for tools that rewrite source (the formatter).
#[derive(Debug, Clone)]
pub struct Comment {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub block: bool,
}

pub const KEYWORDS: &[&str] = &[
    "package", "use", "type", "record", "enum", "interface", "func", "return", "var",
    "let", "if", "else", "match", "for", "in", "while", "break", "continue", "defer", "spawn",
    "end", "unsafe", "extern", "embed", "new", "and", "or", "not", "true", "false", "nil",
    "bitand", "bitor", "bitxor", "bitnot", "ref", "self", "asm", "try", "async", "await", "comptime",
    "shared", "spawn_thread", "pub",
];

/// Longest operators first.
const OPS: &[&str] = &[
    "...", "..", "->", "??", "?.", "==", "!=", "<=", ">=", "<<", ">>", "+=", "-=", "*=", "/=",
    "%=", "#[", "+", "-", "*", "/", "%", "&", "|", "^", "=", "<", ">", "?", "!", ":", ",", ".",
    "(", ")", "[", "]", "{", "}",
];

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
    out: Vec<Token>,
    /// Inside an `asm` block: its lines are assembly, read as they are.
    in_asm: bool,
    /// Offset where the token being read starts.
    tok_start: usize,
    comments: Vec<Comment>,
    /// Names of `#[block]` functions of the imported packages (see `comptime::block_names`).
    blocks: std::collections::HashSet<String>,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Self::at(src, 1, 1)
    }

    /// Start counting from a given position (used for interpolations).
    pub fn at(src: &str, line: u32, col: u32) -> Self {
        Lexer { chars: src.chars().collect(), pos: 0, line, col, out: Vec::new(), in_asm: false, tok_start: 0, comments: Vec::new(), blocks: std::collections::HashSet::new() }
    }

    /// The names that start a block form when alone at the end of a line.
    pub fn with_blocks(mut self, blocks: std::collections::HashSet<String>) -> Self {
        self.blocks = blocks;
        self
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, Diag> {
        Err(Diag { line: self.line, col: self.col, msg: msg.into() })
    }

    fn push(&mut self, tok: Tok, line: u32, col: u32) {
        self.out.push(Token { tok, line, col, span: (self.tok_start, self.pos) });
    }

    pub fn tokenize(self) -> Result<Vec<Token>, Diag> {
        self.tokenize_all().map(|(toks, _)| toks)
    }

    /// The tokens and the comments (which `tokenize` leaves out), with source positions.
    pub fn tokenize_all(mut self) -> Result<(Vec<Token>, Vec<Comment>), Diag> {
        while let Some(c) = self.peek() {
            self.tok_start = self.pos;
            let (line, col) = (self.line, self.col);
            match c {
                ' ' | '\t' | '\r' => {
                    self.bump();
                }
                '\n' => {
                    self.bump();
                    if !matches!(self.out.last().map(|t| &t.tok), None | Some(Tok::Newline)) {
                        self.push(Tok::Newline, line, col);
                    }
                    if self.in_asm {
                        self.asm_line()?;
                    }
                }
                '/' if self.peek_at(1) == Some('/') => {
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            break;
                        }
                        self.bump();
                    }
                    self.comments.push(Comment { start: self.tok_start, end: self.pos, line, block: false });
                }
                '/' if self.peek_at(1) == Some('*') => {
                    self.bump();
                    self.bump();
                    loop {
                        match self.bump() {
                            None => {
                                return Err(Diag { line, col, msg: "unterminated block comment".into() })
                            }
                            Some('*') if self.peek() == Some('/') => {
                                self.bump();
                                break;
                            }
                            _ => {}
                        }
                    }
                    self.comments.push(Comment { start: self.tok_start, end: self.pos, line, block: true });
                }
                '"' => {
                    let t = self.text()?;
                    self.push(t, line, col);
                }
                '\'' => {
                    let t = self.char_lit()?;
                    self.push(t, line, col);
                }
                c if c.is_ascii_digit() => {
                    let t = self.number()?;
                    self.push(t, line, col);
                }
                c if c.is_ascii_alphabetic() || c == '_' => {
                    let mut s = String::new();
                    while let Some(ch) = self.peek() {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            s.push(ch);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let tok = match KEYWORDS.iter().find(|k| **k == s) {
                        Some(k) => Tok::Kw(k),
                        None => Tok::Ident(s),
                    };
                    if let Tok::Ident(name) = &tok {
                        let after_dot_or_decl = matches!(
                            self.out.last().map(|t| &t.tok),
                            Some(Tok::Op(".")) | Some(Tok::Op("?.")) | Some(Tok::Kw("func")) | Some(Tok::Kw("type")) | Some(Tok::Kw("use")) | Some(Tok::Kw("package"))
                        );
                        if self.blocks.contains(name) && !after_dot_or_decl && self.rest_of_line_is_blank() {
                            let name = name.clone();
                            let raw = self.block_lines(&name, line, col)?;
                            self.push(Tok::Block { name, raw }, line, col);
                            continue;
                        }
                    }
                    if let Tok::Ident(name) = &tok {
                        if name == "sql" && !matches!(self.out.last().map(|t| &t.tok), Some(Tok::Op(".")) | Some(Tok::Op("?.")) | Some(Tok::Kw("func")) | Some(Tok::Kw("type")) | Some(Tok::Kw("use")) | Some(Tok::Kw("package"))) {
                            if let Some((dialect, after)) = self.sql_dialect_ahead() {
                                while self.pos < after {
                                    self.bump();
                                }
                                let raw = self.block_lines("sql", line, col)?;
                                self.push(Tok::Sql { dialect, raw }, line, col);
                                continue;
                            }
                        }
                    }
                    let starts_block = matches!(tok, Tok::Kw("asm")) && self.rest_of_line_is_blank();
                    self.push(tok, line, col);
                    if starts_block {
                        self.in_asm = true;
                    }
                }
                _ => match self.op() {
                    Some(op) => self.push(Tok::Op(op), line, col),
                    None => return self.err(format!("unexpected character '{}'", c)),
                },
            }
        }
        let (line, col) = (self.line, self.col);
        self.tok_start = self.pos;
        if !matches!(self.out.last().map(|t| &t.tok), None | Some(Tok::Newline)) {
            self.push(Tok::Newline, line, col);
        }
        self.push(Tok::Eof, line, col);
        Ok((self.out, self.comments))
    }

    /// After a block name: skips the rest of that line and returns the raw lines up to a line that
    /// is only `end`. Stops at the end of the `end` line's text (its newline is left to the
    /// main loop).
    fn block_lines(&mut self, name: &str, line: u32, col: u32) -> Result<String, Diag> {
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.bump();
        }
        let mut lines: Vec<String> = Vec::new();
        loop {
            if self.peek().is_none() {
                return Err(Diag { line, col, msg: if name == "sql" { "the `sql` block that begins here is not closed with 'end'".to_string() } else { format!("block '{}' is not closed with 'end'", name) } });
            }
            self.bump(); // the newline before this line
            let mut i = self.pos;
            while i < self.chars.len() && self.chars[i] != '\n' {
                i += 1;
            }
            let text: String = self.chars[self.pos..i].iter().collect();
            let text = text.trim_end_matches('\r').to_string();
            while self.pos < i {
                self.bump();
            }
            if text.trim() == "end" {
                return Ok(lines.join("
"));
            }
            lines.push(text);
            if self.peek().is_none() {
                return Err(Diag { line, col, msg: if name == "sql" { "the `sql` block that begins here is not closed with 'end'".to_string() } else { format!("block '{}' is not closed with 'end'", name) } });
            }
        }
    }

    /// After the word `sql`: an identifier (the dialect) and then the end of the line. Returns the
    /// dialect and the position after it, or `None` when `sql` is an ordinary identifier here.
    fn sql_dialect_ahead(&self) -> Option<(String, usize)> {
        let mut i = self.pos;
        let mut gap = 0;
        while matches!(self.chars.get(i), Some(' ' | '\t')) {
            i += 1;
            gap += 1;
        }
        let start = i;
        while self.chars.get(i).map_or(false, |c| c.is_ascii_alphanumeric() || *c == '_') {
            i += 1;
        }
        if gap == 0 || i == start || self.chars[start].is_ascii_digit() || !self.blank_from(i) {
            return None;
        }
        Some((self.chars[start..i].iter().collect(), i))
    }

    /// Only blanks or a `//` comment follow on this line.
    fn rest_of_line_is_blank(&self) -> bool {
        self.blank_from(self.pos)
    }

    fn blank_from(&self, from: usize) -> bool {
        let mut i = from;
        while let Some(c) = self.chars.get(i) {
            match c {
                ' ' | '\t' | '\r' => i += 1,
                '\n' => return true,
                '/' => return self.chars.get(i + 1) == Some(&'/'),
                _ => return false,
            }
        }
        true
    }

    /// At the start of a line inside an `asm` block: an assembly line becomes one text token
    /// holding the line as written; operands, clobbers, quoted lines and the closing `end` are
    /// left to the normal tokenizer.
    fn asm_line(&mut self) -> Result<(), Diag> {
        loop {
            let mut i = self.pos;
            while matches!(self.chars.get(i), Some(' ' | '\t' | '\r')) {
                i += 1;
            }
            let start = i;
            while i < self.chars.len() && self.chars[i] != '\n' {
                i += 1;
            }
            let text: String = self.chars[start..i].iter().collect();
            let text = text.trim_end().to_string();
            let skip_line = |lx: &mut Lexer| {
                while lx.pos < i {
                    lx.bump();
                }
                lx.bump();
            };
            if self.pos >= self.chars.len() {
                return Ok(());
            }
            if text.is_empty() || text.starts_with("//") {
                if !text.is_empty() {
                    let line = self.line;
                    self.comments.push(Comment { start, end: start + text.chars().count(), line, block: false });
                }
                skip_line(self);
                if i >= self.chars.len() {
                    return Ok(());
                }
                continue;
            }
            let word: String = text.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let rest = text[word.len()..].trim_start();
            if word == "end" && (rest.is_empty() || rest.starts_with("//")) {
                self.in_asm = false;
                return Ok(());
            }
            let is_operand = matches!(word.as_str(), "in" | "out" | "inout") && {
                let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                let after = rest[name.len()..].trim_start();
                !name.is_empty() && after.starts_with('=') && !after.starts_with("==")
            };
            let is_clobber = word == "clobber" && (rest.is_empty() || text[word.len()..].starts_with(|c: char| c == ' ' || c == '\t'));
            if text.starts_with('"') || is_operand || is_clobber {
                return Ok(());
            }
            // an assembly line: drop a trailing `//` comment that is not inside quotes
            let mut end = text.len();
            let mut quote: Option<char> = None;
            let bytes: Vec<(usize, char)> = text.char_indices().collect();
            for (k, &(pos, c)) in bytes.iter().enumerate() {
                match quote {
                    Some(q) => {
                        if c == q {
                            quote = None;
                        }
                    }
                    None => {
                        if c == '"' || c == '\'' {
                            quote = Some(c);
                        } else if c == '/' && bytes.get(k + 1).map(|b| b.1) == Some('/') {
                            end = pos;
                            break;
                        }
                    }
                }
            }
            let line_text = text[..end].trim_end().to_string();
            let (line, col) = (self.line, self.col + (start - self.pos) as u32);
            while self.pos < i {
                self.bump();
            }
            self.tok_start = start;
            self.push(Tok::Text(vec![StrPart::Lit(line_text)]), line, col);
            return Ok(());
        }
    }

    fn op(&mut self) -> Option<&'static str> {
        for op in OPS {
            let n = op.len();
            let matches = op.chars().enumerate().all(|(i, c)| self.peek_at(i) == Some(c));
            if matches {
                for _ in 0..n {
                    self.bump();
                }
                return Some(op);
            }
        }
        None
    }

    fn number(&mut self) -> Result<Tok, Diag> {
        if self.peek() == Some('0') && matches!(self.peek_at(1), Some('x' | 'X' | 'b' | 'B')) {
            let radix = if matches!(self.peek_at(1), Some('x' | 'X')) { 16 } else { 2 };
            self.bump();
            self.bump();
            let mut s = String::new();
            while let Some(c) = self.peek() {
                if c == '_' {
                    self.bump();
                } else if c.is_digit(radix) {
                    s.push(c);
                    self.bump();
                } else {
                    break;
                }
            }
            if s.is_empty() {
                return self.err("expected digits after radix prefix");
            }
            return match u64::from_str_radix(&s, radix) {
                Ok(n) => Ok(Tok::Int(n)),
                Err(_) => self.err("integer literal too large"),
            };
        }
        let mut s = String::new();
        let mut is_float = false;
        self.digits(&mut s);
        if self.peek() == Some('.') && matches!(self.peek_at(1), Some(d) if d.is_ascii_digit()) {
            is_float = true;
            s.push('.');
            self.bump();
            self.digits(&mut s);
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let ok = match self.peek_at(1) {
                Some(d) if d.is_ascii_digit() => true,
                Some('+' | '-') => matches!(self.peek_at(2), Some(d) if d.is_ascii_digit()),
                _ => false,
            };
            if ok {
                is_float = true;
                s.push('e');
                self.bump();
                if let Some(sign @ ('+' | '-')) = self.peek() {
                    s.push(sign);
                    self.bump();
                }
                self.digits(&mut s);
            }
        }
        if is_float {
            match s.parse::<f64>() {
                Ok(f) => Ok(Tok::Float(f)),
                Err(_) => self.err("invalid float literal"),
            }
        } else {
            match s.parse::<u64>() {
                Ok(n) => Ok(Tok::Int(n)),
                Err(_) => self.err("integer literal too large"),
            }
        }
    }

    fn digits(&mut self, s: &mut String) {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.bump();
            } else if c == '_' {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// Called right after a backslash was consumed.
    fn escape(&mut self) -> Result<char, Diag> {
        match self.bump() {
            Some('n') => Ok('\n'),
            Some('t') => Ok('\t'),
            Some('r') => Ok('\r'),
            Some('0') => Ok('\0'),
            Some('\\') => Ok('\\'),
            Some('\'') => Ok('\''),
            Some('"') => Ok('"'),
            Some('{') => Ok('{'),
            Some('}') => Ok('}'),
            Some('x') => {
                let mut v = 0u32;
                for _ in 0..2 {
                    match self.bump().and_then(|c| c.to_digit(16)) {
                        Some(d) => v = v * 16 + d,
                        None => return self.err("'\\x' needs two hex digits, like \\x1b"),
                    }
                }
                Ok(char::from_u32(v).unwrap())
            }
            Some(c) => self.err(format!("unknown escape '\\{}'", c)),
            None => self.err("unterminated escape sequence"),
        }
    }

    fn char_lit(&mut self) -> Result<Tok, Diag> {
        self.bump(); // opening quote
        let c = match self.bump() {
            Some('\\') => self.escape()?,
            Some('\'') | Some('\n') | None => return self.err("empty or unterminated char literal"),
            Some(c) => c,
        };
        if self.bump() != Some('\'') {
            return self.err("unterminated char literal");
        }
        Ok(Tok::Char(c))
    }

    fn text(&mut self) -> Result<Tok, Diag> {
        let (sl, sc) = (self.line, self.col);
        self.bump(); // opening quote
        let mut parts: Vec<StrPart> = Vec::new();
        let mut cur = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => {
                    return Err(Diag { line: sl, col: sc, msg: "unterminated text literal".into() })
                }
                Some('"') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    let c = self.escape()?;
                    cur.push(c);
                }
                Some('{') => {
                    self.bump();
                    let (il, ic) = (self.line, self.col);
                    let src = self.interp_src(sl, sc)?;
                    if !cur.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut cur)));
                    }
                    parts.push(StrPart::Interp { src, line: il, col: ic });
                }
                Some(c) => {
                    cur.push(c);
                    self.bump();
                }
            }
        }
        if !cur.is_empty() || parts.is_empty() {
            parts.push(StrPart::Lit(cur));
        }
        Ok(Tok::Text(parts))
    }

    /// Collect the source of `{ ... }` up to the matching brace.
    fn interp_src(&mut self, sl: u32, sc: u32) -> Result<String, Diag> {
        let unterminated =
            || Diag { line: sl, col: sc, msg: "unterminated interpolation in text literal".into() };
        let mut depth = 1;
        let mut s = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => return Err(unterminated()),
                Some('{') => {
                    depth += 1;
                    s.push('{');
                }
                Some('}') => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(s);
                    }
                    s.push('}');
                }
                Some('"') => {
                    s.push('"');
                    loop {
                        match self.bump() {
                            None | Some('\n') => return Err(unterminated()),
                            Some('\\') => {
                                s.push('\\');
                                if let Some(c) = self.bump() {
                                    s.push(c);
                                }
                            }
                            Some('"') => {
                                s.push('"');
                                break;
                            }
                            Some(c) => s.push(c),
                        }
                    }
                }
                Some(c) => s.push(c),
            }
        }
    }
}
