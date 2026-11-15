//! Open documents: positions in them and where their files are.

use super::json::{obj, num, Json};

/// UTF-16 length of a line's first `chars` characters (LSP columns count UTF-16 units).
pub(super) fn utf16_col(line: &str, chars: usize) -> i64 {
    line.chars().take(chars).map(|c| c.len_utf16() as i64).sum()
}

/// The number of characters before UTF-16 unit `units` of a line (the inverse of `utf16_col`).
pub(super) fn char_col(line: &str, units: i64) -> usize {
    let mut seen = 0;
    for (i, c) in line.chars().enumerate() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16() as i64;
    }
    line.chars().count()
}

/// The LSP range of a range of the syntax tree (lines and columns from 1, in characters).
pub(super) fn range_of(text: &str, s: crate::ast::Span) -> Json {
    let col = |line: u32, c: u32| utf16_col(text.lines().nth(line as usize - 1).unwrap_or(""), c as usize - 1);
    obj(vec![
        ("start", position(s.line as i64 - 1, col(s.line, s.col))),
        ("end", position(s.end_line as i64 - 1, col(s.end_line, s.end_col))),
    ])
}

pub(super) fn position(line: i64, character: i64) -> Json {
    obj(vec![("line", num(line)), ("character", num(character))])
}

/// The file a `file://` URI names (`file:///c:/dir/x.zila` is `c:/dir/x.zila`).
pub(super) fn uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut path = String::new();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                path.push(v as char);
                i += 3;
                continue;
            }
        }
        path.push(b[i] as char);
        i += 1;
    }
    let path = if path.len() > 2 && path.as_bytes()[0] == b'/' && path.as_bytes()[2] == b':' { path[1..].to_string() } else { path };
    Some(std::path::PathBuf::from(path))
}

/// The `file://` URI of a file (the inverse of `uri_to_path`).
pub(super) fn path_to_uri(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_string();
    let enc: String = s.chars().map(|c| if c == ' ' { "%20".to_string() } else { c.to_string() }).collect();
    if enc.starts_with('/') {
        format!("file://{}", enc)
    } else {
        format!("file:///{}", enc)
    }
}

/// Finds the packages that sit next to the document, so that their block names are known: the
/// server only parses (block content is never looked into) and never runs comptime code.
pub(super) fn register_neighbours(uri: &str, text: &str) {
    let Some(path) = uri_to_path(uri) else { return };
    // the editor never fetches; a project that does not check out simply has no dependencies here
    if crate::package::install_for(&path).is_err() {
        crate::package::clear();
    }
    crate::comptime::register_siblings(&path, text);
}
