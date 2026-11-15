//! Diagnostics: syntax errors, the errors and warnings of the compiler's checking, and publishing
//! them.

use super::documents::{position, uri_to_path, utf16_col};
use super::json::{num, obj, Json};
use super::transport::send;
use crate::ast::Span;
use crate::codegen_x64::{check_program, CheckError};
use crate::parser::parse_partial;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

/// The diagnostics for the open document `uri`: its syntax error or, for a document that parses,
/// what the compiler's checking reports over the text of every open document (unsaved edits of the
/// other files of the package count).
pub fn diagnostics(uri: &str, docs: &HashMap<String, String>) -> Json {
    let Some(text) = docs.get(uri) else { return Json::Arr(Vec::new()) };
    let mut list = Vec::new();
    let partial = parse_partial(text);
    match (partial.errors.first(), partial.file) {
        (None, Some(file)) => {
            // a program that asks for code generation is not checked here: the server never runs
            // comptime code, so opening a file from an untrusted package executes nothing
            if file.package == "main" && !crate::comptime::uses_comptime(&file) {
                let path = uri_to_path(uri);
                let overlay: HashMap<PathBuf, String> = docs.iter().filter_map(|(u, t)| Some((uri_to_path(u)?, t.clone()))).collect();
                crate::warn::clear();
                // a bug in the compiler must not take the server down with it
                let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check_program(&file, path.as_deref(), overlay)));
                match run {
                    Ok(checked) => {
                        for e in &checked.errors {
                            list.push(error_diagnostic(text, &file.package, e));
                        }
                        for w in crate::warn::take() {
                            list.push(diagnostic(text, whole_line(text, 0), 2, w));
                        }
                    }
                    Err(_) => list.push(diagnostic(text, whole_line(text, 0), 2, "the checker stopped on an internal error; no semantic diagnostics for this text".into())),
                }
            }
        }
        // only the first syntax error is shown: the ones after it are often a result of it
        (Some(d), _) => {
            let line = (d.line.max(1) - 1) as i64;
            let src_line = text.lines().nth(line as usize).unwrap_or("");
            let start = (d.col.max(1) - 1) as usize;
            let end = src_line.chars().count().max(start + 1);
            list.push(diagnostic(text, (line, start, line, end), 1, d.msg.clone()));
        }
        (None, None) => {}
    }
    Json::Arr(list)
}

/// A range as (line, start character, line, end character), counting characters from 0.
type Range = (i64, usize, i64, usize);

/// The text of line `line` (from 0) without its indentation, as a range; one character wide for an
/// empty line.
fn whole_line(text: &str, line: i64) -> Range {
    let src_line = text.lines().nth(line.max(0) as usize).unwrap_or("");
    let start = src_line.chars().take_while(|c| c.is_whitespace()).count();
    (line, start, line, src_line.chars().count().max(start + 1))
}

fn from_span(s: Span) -> Range {
    (s.line as i64 - 1, s.col.max(1) as usize - 1, s.end_line as i64 - 1, s.end_col.max(1) as usize - 1)
}

fn error_diagnostic(text: &str, package: &str, e: &CheckError) -> Json {
    if e.module != package {
        // an error inside another package: its line counts that package's merged text, so it is
        // shown at the top of this file with where it is
        return diagnostic(text, whole_line(text, 0), 1, format!("in package '{}' (line {}): {}", e.module, e.line, e.message));
    }
    let range = if e.span.is_none() { whole_line(text, e.line.max(1) as i64 - 1) } else { from_span(e.span) };
    diagnostic(text, range, 1, e.message.clone())
}

fn diagnostic(text: &str, r: Range, severity: i64, message: String) -> Json {
    let col = |line: i64, chars: usize| utf16_col(text.lines().nth(line.max(0) as usize).unwrap_or(""), chars);
    obj(vec![
        ("range", obj(vec![("start", position(r.0, col(r.0, r.1))), ("end", position(r.2, col(r.2, r.3)))])),
        ("severity", num(severity)),
        ("source", Json::Str("zila".into())),
        ("message", Json::Str(message)),
    ])
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

/// Publishes the diagnostics of the document that changed, then those of the other open
/// documents, whose answer may depend on it.
pub(super) fn publish_all(out: &mut dyn Write, changed: &str, docs: &HashMap<String, String>) {
    publish(out, changed, diagnostics(changed, docs));
    let mut others: Vec<&String> = docs.keys().filter(|u| u.as_str() != changed).collect();
    others.sort();
    for u in others {
        publish(out, u, diagnostics(u, docs));
    }
}
