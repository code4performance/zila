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
pub use features::{completion, formatting, hover, signature_help};
pub use json::{parse_json, Json};

use diagnostics::{publish, publish_all};
use documents::register_neighbours;
use json::{num, obj};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;
use transport::{read_message, send};

/// Handles one message and publishes the diagnostics it calls for at once. Returns false after
/// `exit`.
pub fn handle(msg: &Json, docs: &mut HashMap<String, String>, out: &mut dyn Write, shutdown: &mut bool) -> bool {
    handle_with(msg, docs, out, shutdown, None)
}

/// `handle`, except that with `changed` given, an opened or edited document is only recorded there
/// (the server checks after a quiet period, see `serve`).
fn handle_with(msg: &Json, docs: &mut HashMap<String, String>, out: &mut dyn Write, shutdown: &mut bool, mut changed: Option<&mut Vec<String>>) -> bool {
    let mut after_edit = |out: &mut dyn Write, uri: &str, docs: &HashMap<String, String>| match changed.as_deref_mut() {
        Some(list) => {
            list.retain(|u| u != uri);
            list.push(uri.to_string());
        }
        None => publish_all(out, uri, docs),
    };
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
                ("capabilities", obj(vec![("textDocumentSync", num(1)), ("documentFormattingProvider", Json::Bool(true)), ("hoverProvider", Json::Bool(true)), ("completionProvider", obj(vec![("triggerCharacters", Json::Arr(vec![Json::Str(".".into())]))])), ("signatureHelpProvider", obj(vec![("triggerCharacters", Json::Arr(vec![Json::Str("(".into()), Json::Str(",".into())]))]))])),
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
                after_edit(out, uri, docs);
            }
        }
        "textDocument/didChange" => {
            let uri = params.get("textDocument").get("uri").str().unwrap_or("").to_string();
            if let Json::Arr(changes) = params.get("contentChanges") {
                if let Some(text) = changes.last().and_then(|c| c.get("text").str()) {
                    docs.insert(uri.clone(), text.to_string());
                    register_neighbours(&uri, text);
                    after_edit(out, &uri, docs);
                }
            }
        }
        "textDocument/didClose" => {
            if let Some(uri) = params.get("textDocument").get("uri").str() {
                docs.remove(uri);
                diagnostics::forget(uri);
                if let Some(list) = changed.as_mut() {
                    list.retain(|u| u != uri);
                }
                publish(out, uri, Json::Arr(Vec::new()));
            }
        }
        "textDocument/hover" => {
            let uri = params.get("textDocument").get("uri").str().unwrap_or("");
            let at = params.get("position");
            let (Json::Num(line), Json::Num(character)) = (at.get("line"), at.get("character")) else { return reply_null(out, &id) };
            reply(out, hover(uri, docs, *line as i64, *character as i64));
        }
        "textDocument/completion" => {
            let uri = params.get("textDocument").get("uri").str().unwrap_or("");
            let at = params.get("position");
            let (Json::Num(line), Json::Num(character)) = (at.get("line"), at.get("character")) else { return reply_null(out, &id) };
            reply(out, completion(uri, docs, *line as i64, *character as i64));
        }
        "textDocument/signatureHelp" => {
            let uri = params.get("textDocument").get("uri").str().unwrap_or("");
            let at = params.get("position");
            let (Json::Num(line), Json::Num(character)) = (at.get("line"), at.get("character")) else { return reply_null(out, &id) };
            reply(out, signature_help(uri, docs, *line as i64, *character as i64));
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

fn reply_null(out: &mut dyn Write, id: &Json) -> bool {
    send(out, &obj(vec![("jsonrpc", Json::Str("2.0".into())), ("id", id.clone()), ("result", Json::Null)]));
    true
}

/// How long the server waits after the last edit before it checks.
const QUIET: Duration = Duration::from_millis(200);

/// True for a message that changes what a check would say.
fn is_edit(msg: &Json) -> bool {
    matches!(msg.get("method").str(), Some("textDocument/didOpen" | "textDocument/didChange" | "textDocument/didClose"))
}

/// The messages that have arrived, so that the server can see whether a newer edit is waiting
/// while it works on an older one.
struct Inbox {
    rx: Receiver<Json>,
    queue: VecDeque<Json>,
}

impl Inbox {
    /// Moves what has arrived into the queue.
    fn fill(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            self.queue.push_back(m);
        }
    }

    fn edit_waiting(&mut self) -> bool {
        self.fill();
        self.queue.iter().any(is_edit)
    }

    /// The next message, waiting at most `wait` (for ever when `None`); `Err(true)` at the end of
    /// the input, `Err(false)` when the time ran out.
    fn next(&mut self, wait: Option<Duration>) -> Result<Json, bool> {
        if let Some(m) = self.queue.pop_front() {
            return Ok(m);
        }
        match wait {
            None => self.rx.recv().map_err(|_| true),
            Some(w) => self.rx.recv_timeout(w).map_err(|e| e == RecvTimeoutError::Disconnected),
        }
    }
}

/// Checks the open documents and publishes, unless a newer edit has arrived meanwhile: its answer
/// replaces this one, so this one is dropped. Returns whether it published.
fn flush(inbox: &mut Inbox, last: &str, docs: &HashMap<String, String>, out: &mut dyn Write) -> bool {
    if inbox.edit_waiting() {
        return false;
    }
    let mut uris: Vec<&String> = docs.keys().filter(|u| u.as_str() != last).collect();
    uris.sort();
    if docs.contains_key(last) {
        uris.insert(0, docs.get_key_value(last).unwrap().0);
    }
    for u in uris {
        let diags = diagnostics::diagnostics(u, docs);
        if inbox.edit_waiting() {
            return false;
        }
        publish(out, u, diags);
    }
    true
}

/// Serves the messages that arrive on `rx` until `exit` or the end of the input; returns the exit
/// status. Diagnostics are not computed per edit: they follow `quiet` after the last one, and a
/// check that a newer edit overtakes is not published.
pub fn serve(rx: Receiver<Json>, out: &mut dyn Write, quiet: Duration) -> i32 {
    let mut inbox = Inbox { rx, queue: VecDeque::new() };
    let mut docs = HashMap::new();
    let mut shutdown = false;
    let mut changed: Vec<String> = Vec::new();
    loop {
        let wait = if changed.is_empty() { None } else { Some(quiet) };
        match inbox.next(wait) {
            Ok(msg) => {
                if !handle_with(&msg, &mut docs, out, &mut shutdown, Some(&mut changed)) {
                    break;
                }
            }
            Err(true) => break,
            Err(false) => {
                // quiet: check, unless an edit is already on its way
                let last = changed.last().cloned().unwrap_or_default();
                if flush(&mut inbox, &last, &docs, out) {
                    changed.clear();
                }
            }
        }
    }
    if shutdown {
        0
    } else {
        1
    }
}

/// Runs the server on standard input and output until `exit` or the end of the input; returns the
/// process exit status.
pub fn run() -> i32 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        while let Some(msg) = read_message(&mut input) {
            if tx.send(msg).is_err() {
                break;
            }
        }
    });
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    serve(rx, &mut out, QUIET)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit_message(uri: &str, text: &str) -> Json {
        let mut j = String::new();
        Json::Str(text.into()).write(&mut j);
        parse_json(&format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{}"}},"contentChanges":[{{"text":{}}}]}}}}"#, uri, j)).unwrap()
    }

    #[test]
    fn a_check_that_a_newer_edit_overtakes_is_not_published() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut inbox = Inbox { rx, queue: VecDeque::new() };
        let mut docs = HashMap::new();
        docs.insert("file:///a.zila".to_string(), "package main\n\nfunc main()\nend\n".to_string());
        let mut out: Vec<u8> = Vec::new();
        // nothing is waiting: the check is published
        assert!(flush(&mut inbox, "file:///a.zila", &docs, &mut out));
        assert!(!out.is_empty());
        // an edit is waiting: this check is dropped, the edit stays for the loop to take
        out.clear();
        tx.send(edit_message("file:///a.zila", "package main\n")).unwrap();
        assert!(!flush(&mut inbox, "file:///a.zila", &docs, &mut out));
        assert!(out.is_empty());
        assert!(inbox.next(Some(Duration::from_millis(1))).is_ok());
        // a request that is not an edit does not stop a check
        tx.send(parse_json(r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/formatting","params":{}}"#).unwrap()).unwrap();
        assert!(flush(&mut inbox, "file:///a.zila", &docs, &mut out));
    }
}
