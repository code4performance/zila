//! Lexer for Brasa.
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
    Newline,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
}

pub const KEYWORDS: &[&str] = &[
    "package", "use", "type", "record", "enum", "interface", "func", "return", "var",
    "let", "if", "else", "match", "for", "in", "while", "break", "continue", "defer", "spawn",
    "end", "unsafe", "extern", "embed", "new", "and", "or", "not", "true", "false", "nil",
    "bitand", "bitor", "bitxor", "bitnot", "ref", "self", "asm", "try", "async", "await",
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
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Self::at(src, 1, 1)
    }

    /// Start counting from a given position (used for interpolations).
    pub fn at(src: &str, line: u32, col: u32) -> Self {
        Lexer { chars: src.chars().collect(), pos: 0, line, col, out: Vec::new() }
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
        self.out.push(Token { tok, line, col });
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, Diag> {
        while let Some(c) = self.peek() {
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
                }
                '/' if self.peek_at(1) == Some('/') => {
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            break;
                        }
                        self.bump();
                    }
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
                    self.push(tok, line, col);
                }
                _ => match self.op() {
                    Some(op) => self.push(Tok::Op(op), line, col),
                    None => return self.err(format!("unexpected character '{}'", c)),
                },
            }
        }
        let (line, col) = (self.line, self.col);
        if !matches!(self.out.last().map(|t| &t.tok), None | Some(Tok::Newline)) {
            self.push(Tok::Newline, line, col);
        }
        self.push(Tok::Eof, line, col);
        Ok(self.out)
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
