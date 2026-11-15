//! The framing of the protocol: `Content-Length` headers over standard input and output.

use super::json::{parse_json, Json};
use std::io::{BufRead, Write};

/// Reads one framed message, or `None` at the end of the input.
pub fn read_message(input: &mut impl BufRead) -> Option<Json> {
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

pub fn send(out: &mut dyn Write, msg: &Json) {
    let mut s = String::new();
    msg.write(&mut s);
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", s.len(), s);
    let _ = out.flush();
}
