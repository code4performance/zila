//! Diagnostics: syntax and compile errors of a document, and publishing them.

use super::documents::{position, utf16_col};
use super::json::{num, obj, Json};
use super::transport::send;
use crate::parser::parse_source;
use std::io::Write;

/// The diagnostics for a document: a syntax error or, for a program that parses, a compile error.
pub fn diagnostics(text: &str) -> Json {
    let mut list = Vec::new();
    let parsed = parse_source(text);
    if let Ok(file) = &parsed {
        // a program that parses is compiled, so the compiler's own errors (types, `#[no_alloc]`,
        // `#[allocator]`) show up as you type: they start with `main.zila:LINE:`
        // a program that asks for code generation is not compiled here: the server never runs
        // comptime code, so opening a file from an untrusted package executes nothing
        if file.package == "main" && !crate::comptime::uses_comptime(file) {
            if let Err(e) = crate::codegen_x64::generate_asm(file) {
                let line = e.strip_prefix("main.zila:").and_then(|r| r.split(':').next()).and_then(|n| n.trim().parse::<i64>().ok()).unwrap_or(1);
                let line = line.max(1) - 1;
                let src_line = text.lines().nth(line as usize).unwrap_or("");
                let end = src_line.chars().count();
                let start = src_line.chars().take_while(|c| c.is_whitespace()).count();
                list.push(obj(vec![
                    ("range", obj(vec![("start", position(line, utf16_col(src_line, start))), ("end", position(line, utf16_col(src_line, end.max(start + 1))))])),
                    ("severity", num(1)),
                    ("source", Json::Str("zila".into())),
                    ("message", Json::Str(e)),
                ]));
            }
        }
    }
    if let Err(d) = parsed {
        let line = (d.line.max(1) - 1) as i64;
        let src_line = text.lines().nth(line as usize).unwrap_or("");
        let start = (d.col.max(1) - 1) as usize;
        let end = src_line.chars().count().max(start + 1);
        list.push(obj(vec![
            ("range", obj(vec![("start", position(line, utf16_col(src_line, start))), ("end", position(line, utf16_col(src_line, end)))])),
            ("severity", num(1)),
            ("source", Json::Str("zila".into())),
            ("message", Json::Str(d.msg)),
        ]));
    }
    Json::Arr(list)
}

pub(super) fn publish(out: &mut dyn Write, uri: &str, diags: Json) {
    send(
        out,
        &obj(vec![
            ("jsonrpc", Json::Str("2.0".into())),
            ("method", Json::Str("textDocument/publishDiagnostics".into())),
            ("params", obj(vec![("uri", Json::Str(uri.into())), ("diagnostics", diags)])),
        ]),
    );
}
