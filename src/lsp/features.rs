//! The requests that answer from a document: formatting.

use super::documents::{position, utf16_col};
use super::json::{obj, Json};
use crate::fmt::format_source;

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
