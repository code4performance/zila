//! A small language server (`brasa-parse --lsp`) speaking the Language Server Protocol over
//! standard input and output. It reports syntax errors as you type and formats documents with the
//! standard style (`--fmt`). No dependencies: it carries its own tiny JSON reader and writer.

use crate::fmt::format_source;
use crate::parser::parse_source;
use std::collections::HashMap;
use std::io::{BufRead, Write};

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> &Json {
        if let Json::Obj(v) = self {
            for (k, x) in v {
                if k == key {
                    return x;
                }
            }
        }
        &Json::Null
    }

    pub fn str(&self) -> Option<&str> {
        if let Json::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }

    pub fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => out.push_str(&format!("{}", n)),
            Json::Str(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            Json::Arr(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            Json::Obj(v) => {
                out.push('{');
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    Json::Str(k.clone()).write(out);
                    out.push(':');
                    x.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn obj(fields: Vec<(&str, Json)>) -> Json {
    Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn num(n: i64) -> Json {
    Json::Num(n as f64)
}

pub fn parse_json(src: &str) -> Result<Json, String> {
    let c: Vec<char> = src.chars().collect();
    let mut p = 0;
    let v = value(&c, &mut p)?;
    skip_ws(&c, &mut p);
    if p != c.len() {
        return Err("trailing characters".into());
    }
    Ok(v)
}

fn skip_ws(c: &[char], p: &mut usize) {
    while *p < c.len() && c[*p].is_whitespace() {
        *p += 1;
    }
}

fn value(c: &[char], p: &mut usize) -> Result<Json, String> {
    skip_ws(c, p);
    let Some(&ch) = c.get(*p) else { return Err("unexpected end".into()) };
    match ch {
        '{' => {
            *p += 1;
            let mut v = Vec::new();
            skip_ws(c, p);
            if c.get(*p) == Some(&'}') {
                *p += 1;
                return Ok(Json::Obj(v));
            }
            loop {
                skip_ws(c, p);
                let Json::Str(k) = value(c, p)? else { return Err("object key must be a string".into()) };
                skip_ws(c, p);
                if c.get(*p) != Some(&':') {
                    return Err("expected ':'".into());
                }
                *p += 1;
                v.push((k, value(c, p)?));
                skip_ws(c, p);
                match c.get(*p) {
                    Some(',') => *p += 1,
                    Some('}') => {
                        *p += 1;
                        return Ok(Json::Obj(v));
                    }
                    _ => return Err("expected ',' or '}'".into()),
                }
            }
        }
        '[' => {
            *p += 1;
            let mut v = Vec::new();
            skip_ws(c, p);
            if c.get(*p) == Some(&']') {
                *p += 1;
                return Ok(Json::Arr(v));
            }
            loop {
                v.push(value(c, p)?);
                skip_ws(c, p);
                match c.get(*p) {
                    Some(',') => *p += 1,
                    Some(']') => {
                        *p += 1;
                        return Ok(Json::Arr(v));
                    }
                    _ => return Err("expected ',' or ']'".into()),
                }
            }
        }
        '"' => {
            *p += 1;
            let mut s = String::new();
            loop {
                let Some(&ch) = c.get(*p) else { return Err("unterminated string".into()) };
                *p += 1;
                match ch {
                    '"' => return Ok(Json::Str(s)),
                    '\\' => {
                        let Some(&e) = c.get(*p) else { return Err("bad escape".into()) };
                        *p += 1;
                        match e {
                            'n' => s.push('\n'),
                            'r' => s.push('\r'),
                            't' => s.push('\t'),
                            'b' => s.push('\u{8}'),
                            'f' => s.push('\u{c}'),
                            'u' => {
                                let mut code = hex4(c, p)?;
                                if (0xD800..0xDC00).contains(&code) && c.get(*p) == Some(&'\\') && c.get(*p + 1) == Some(&'u') {
                                    *p += 2;
                                    let low = hex4(c, p)?;
                                    code = 0x10000 + ((code - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF);
                                }
                                s.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                            }
                            other => s.push(other),
                        }
                    }
                    ch => s.push(ch),
                }
            }
        }
        't' | 'f' | 'n' => {
            for (word, v) in [("true", Json::Bool(true)), ("false", Json::Bool(false)), ("null", Json::Null)] {
                if c[*p..].iter().take(word.len()).collect::<String>() == word {
                    *p += word.len();
                    return Ok(v);
                }
            }
            Err("bad literal".into())
        }
        _ => {
            let start = *p;
            while *p < c.len() && matches!(c[*p], '-' | '+' | '.' | 'e' | 'E' | '0'..='9') {
                *p += 1;
            }
            c[start..*p].iter().collect::<String>().parse::<f64>().map(Json::Num).map_err(|_| "bad number".to_string())
        }
    }
}

fn hex4(c: &[char], p: &mut usize) -> Result<u32, String> {
    let s: String = c.iter().skip(*p).take(4).collect();
    *p += 4;
    u32::from_str_radix(&s, 16).map_err(|_| "bad \\u escape".to_string())
}

/// Reads one framed message, or `None` at the end of the input.
fn read_message(input: &mut impl BufRead) -> Option<Json> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0u8; len?];
    input.read_exact(&mut body).ok()?;
    parse_json(&String::from_utf8_lossy(&body)).ok()
}

fn send(out: &mut dyn Write, msg: &Json) {
    let mut s = String::new();
    msg.write(&mut s);
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", s.len(), s);
    let _ = out.flush();
}

/// UTF-16 length of a line's first `chars` characters (LSP columns count UTF-16 units).
fn utf16_col(line: &str, chars: usize) -> i64 {
    line.chars().take(chars).map(|c| c.len_utf16() as i64).sum()
}

fn position(line: i64, character: i64) -> Json {
    obj(vec![("line", num(line)), ("character", num(character))])
}

/// The diagnostics for a document: a syntax error, if any.
pub fn diagnostics(text: &str) -> Json {
    let mut list = Vec::new();
    if let Err(d) = parse_source(text) {
        let line = (d.line.max(1) - 1) as i64;
        let src_line = text.lines().nth(line as usize).unwrap_or("");
        let start = (d.col.max(1) - 1) as usize;
        let end = src_line.chars().count().max(start + 1);
        list.push(obj(vec![
            ("range", obj(vec![("start", position(line, utf16_col(src_line, start))), ("end", position(line, utf16_col(src_line, end)))])),
            ("severity", num(1)),
            ("source", Json::Str("brasa".into())),
            ("message", Json::Str(d.msg)),
        ]));
    }
    Json::Arr(list)
}

/// The edits that format a document: one replacement of the whole text, or none.
pub fn formatting(text: &str) -> Json {
    match format_source(text) {
        Ok(new) if new != text => {
            let lines = text.lines().count() as i64;
            let end = if text.ends_with('\n') || text.is_empty() { position(lines, 0) } else { position(lines - 1, utf16_col(text.lines().last().unwrap_or(""), usize::MAX)) };
            Json::Arr(vec![obj(vec![("range", obj(vec![("start", position(0, 0)), ("end", end)])), ("newText", Json::Str(new))])])
        }
        _ => Json::Arr(Vec::new()),
    }
}

fn publish(out: &mut dyn Write, uri: &str, diags: Json) {
    send(
        out,
        &obj(vec![
            ("jsonrpc", Json::Str("2.0".into())),
            ("method", Json::Str("textDocument/publishDiagnostics".into())),
            ("params", obj(vec![("uri", Json::Str(uri.into())), ("diagnostics", diags)])),
        ]),
    );
}

/// Handles one message. Returns false after `exit`.
pub fn handle(msg: &Json, docs: &mut HashMap<String, String>, out: &mut dyn Write, shutdown: &mut bool) -> bool {
    let method = msg.get("method").str().unwrap_or("");
    let id = msg.get("id").clone();
    let params = msg.get("params");
    let reply = |out: &mut dyn Write, result: Json| {
        send(out, &obj(vec![("jsonrpc", Json::Str("2.0".into())), ("id", id.clone()), ("result", result)]));
    };
    match method {
        "initialize" => reply(
            out,
            obj(vec![
                ("capabilities", obj(vec![("textDocumentSync", num(1)), ("documentFormattingProvider", Json::Bool(true))])),
                ("serverInfo", obj(vec![("name", Json::Str("brasa-parse".into()))])),
            ]),
        ),
        "shutdown" => {
            *shutdown = true;
            reply(out, Json::Null);
        }
        "exit" => return false,
        "textDocument/didOpen" => {
            let d = params.get("textDocument");
            if let (Some(uri), Some(text)) = (d.get("uri").str(), d.get("text").str()) {
                docs.insert(uri.to_string(), text.to_string());
                publish(out, uri, diagnostics(text));
            }
        }
        "textDocument/didChange" => {
            let uri = params.get("textDocument").get("uri").str().unwrap_or("").to_string();
            if let Json::Arr(changes) = params.get("contentChanges") {
                if let Some(text) = changes.last().and_then(|c| c.get("text").str()) {
                    docs.insert(uri.clone(), text.to_string());
                    publish(out, &uri, diagnostics(text));
                }
            }
        }
        "textDocument/didClose" => {
            if let Some(uri) = params.get("textDocument").get("uri").str() {
                docs.remove(uri);
                publish(out, uri, Json::Arr(Vec::new()));
            }
        }
        "textDocument/formatting" => {
            let uri = params.get("textDocument").get("uri").str().unwrap_or("");
            let edits = docs.get(uri).map(|t| formatting(t)).unwrap_or(Json::Arr(Vec::new()));
            reply(out, edits);
        }
        _ => {
            // unknown requests (they have an id) get an empty answer; notifications are ignored
            if id != Json::Null {
                reply(out, Json::Null);
            }
        }
    }
    true
}

/// Runs the server until `exit` or the end of the input; returns the process exit status.
pub fn run() -> i32 {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut docs = HashMap::new();
    let mut shutdown = false;
    while let Some(msg) = read_message(&mut input) {
        if !handle(&msg, &mut docs, &mut out, &mut shutdown) {
            break;
        }
    }
    if shutdown {
        0
    } else {
        1
    }
}
