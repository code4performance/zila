//! A small language server (`zila --lsp`) speaking the Language Server Protocol over
//! standard input and output. It reports syntax errors as you type and formats documents with the
//! standard style (`--fmt`). No dependencies: it carries its own tiny JSON reader and writer.
//!
//! The modules: `json` (values), `transport` (framing), `documents` (positions and files),
//! `diagnostics` (errors of a document), `features` (answers from a document); this file routes
//! the messages.

mod diagnostics;
mod documents;
mod features;
mod json;
mod transport;

pub use diagnostics::diagnostics;
pub use features::formatting;
pub use json::{parse_json, Json};

use diagnostics::publish;
use documents::register_neighbours;
use json::{num, obj};
use std::collections::HashMap;
use std::io::Write;
use transport::{read_message, send};

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
                ("serverInfo", obj(vec![("name", Json::Str("zila".into()))])),
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
                register_neighbours(uri, text);
                publish(out, uri, diagnostics(text));
            }
        }
        "textDocument/didChange" => {
            let uri = params.get("textDocument").get("uri").str().unwrap_or("").to_string();
            if let Json::Arr(changes) = params.get("contentChanges") {
                if let Some(text) = changes.last().and_then(|c| c.get("text").str()) {
                    docs.insert(uri.clone(), text.to_string());
                    register_neighbours(&uri, text);
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
