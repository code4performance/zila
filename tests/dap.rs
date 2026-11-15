//! The debug adapter: scripted sessions over the protocol (programs only print).

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zila::lsp::{parse_json, Json};

static N: AtomicUsize = AtomicUsize::new(0);

struct Out(Arc<Mutex<Vec<u8>>>);

impl Write for Out {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A client talking to an adapter that runs on its own thread.
struct Client {
    tx: Sender<Json>,
    buf: Arc<Mutex<Vec<u8>>>,
    /// Messages read so far.
    seen: Vec<Json>,
    cursor: usize,
    seq: i64,
    handle: Option<std::thread::JoinHandle<i32>>,
}

impl Client {
    fn start() -> Client {
        let (tx, rx) = std::sync::mpsc::channel();
        let buf = Arc::new(Mutex::new(Vec::new()));
        let out = buf.clone();
        let handle = std::thread::spawn(move || zila::dap::serve(rx, &mut Out(out)));
        Client { tx, buf, seen: Vec::new(), cursor: 0, seq: 0, handle: Some(handle) }
    }

    /// The messages the adapter has written that were not read yet.
    fn pump(&mut self) {
        let data = self.buf.lock().unwrap().clone();
        let text = String::from_utf8_lossy(&data[self.cursor..]).to_string();
        let mut used = 0;
        for part in text.split("Content-Length: ").skip(1) {
            let Some((head, body)) = part.split_once("\r\n\r\n") else { break };
            let len: usize = head.trim().parse().unwrap();
            if body.len() < len {
                break;
            }
            self.seen.push(parse_json(&body[..len]).unwrap());
            used += "Content-Length: ".len() + head.len() + 4 + len;
        }
        self.cursor += used;
    }

    fn send(&mut self, command: &str, args: &str) -> i64 {
        self.seq += 1;
        let msg = format!(r#"{{"seq":{},"type":"request","command":"{}","arguments":{}}}"#, self.seq, command, args);
        self.tx.send(parse_json(&msg).unwrap()).unwrap();
        self.seq
    }

    /// Sends a request and waits for its response.
    fn request(&mut self, command: &str, args: &str) -> Json {
        let id = self.send(command, args);
        self.wait(|m| m.get("type").str() == Some("response") && m.get("request_seq") == &Json::Num(id as f64), 60)
    }

    /// Waits for the first unread message that `want` accepts.
    fn wait(&mut self, want: impl Fn(&Json) -> bool, secs: u64) -> Json {
        let until = Instant::now() + Duration::from_secs(secs);
        let mut from = 0;
        loop {
            self.pump();
            if let Some(i) = self.seen[from..].iter().position(&want) {
                return self.seen.remove(from + i);
            }
            assert!(Instant::now() < until, "no such message in time; saw {:?}", self.seen);
            std::thread::sleep(Duration::from_millis(10));
            // keep what was not wanted for later waits
            from = 0;
        }
    }

    fn event(&mut self, name: &str) -> Json {
        let n = name.to_string();
        self.wait(move |m| m.get("type").str() == Some("event") && m.get("event").str() == Some(&n), 60)
    }

    /// The text the program wrote, from `output` events.
    fn program_output(&mut self) -> String {
        self.pump();
        self.seen.iter().filter(|m| m.get("event").str() == Some("output") && m.get("body").get("category").str() == Some("stdout")).map(|m| m.get("body").get("output").str().unwrap().to_string()).collect()
    }

    fn close(mut self) {
        let _ = self.request("disconnect", "{}");
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!("zila_dap_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const PROGRAM: &str = "package main\n\nfunc add(a: int, b: int) -> int\n    let s = a + b\n    return s\nend\n\nfunc main()\n    let x = add(3, 4)\n    let y = x * 2\n    print(\"sum {x} {y}\")\nend\n";

fn json_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "\\\\")
}

/// A started session on `src` (a file of its own), with the program loaded and not yet running;
/// `None` (with a note) when the system's virus scanner blocks the executable, which is not what
/// these tests check.
fn session(src: &str) -> Option<(Client, PathBuf, PathBuf)> {
    let dir = scratch();
    let file = dir.join("prog.zila");
    std::fs::write(&file, src).unwrap();
    let mut c = Client::start();
    let r = c.request("initialize", r#"{"adapterID":"zila"}"#);
    assert_eq!(r.get("success"), &Json::Bool(true));
    assert_eq!(r.get("body").get("supportsConfigurationDoneRequest"), &Json::Bool(true));
    let r = c.request("launch", &format!(r#"{{"program":"{}"}}"#, json_path(&file)));
    if r.get("message").str().is_some_and(|m| m.contains("225")) {
        eprintln!("skipped: {:?}", r.get("message"));
        c.close();
        return None;
    }
    assert_eq!(r.get("success"), &Json::Bool(true), "{:?}", r);
    c.event("initialized");
    let file = std::fs::canonicalize(&file).unwrap();
    Some((c, dir, zila::package::strip_verbatim(file)))
}

fn breakpoints(c: &mut Client, file: &std::path::Path, lines: &[u32]) -> Vec<Json> {
    let bps: Vec<String> = lines.iter().map(|l| format!(r#"{{"line":{}}}"#, l)).collect();
    let r = c.request("setBreakpoints", &format!(r#"{{"source":{{"path":"{}"}},"breakpoints":[{}]}}"#, json_path(file), bps.join(",")));
    assert_eq!(r.get("success"), &Json::Bool(true), "{:?}", r);
    let Json::Arr(v) = r.get("body").get("breakpoints") else { panic!() };
    v.clone()
}

/// (function name, line) of each frame, innermost first.
fn stack(c: &mut Client) -> Vec<(String, i64)> {
    let r = c.request("stackTrace", r#"{"threadId":1}"#);
    let Json::Arr(frames) = r.get("body").get("stackFrames") else { panic!("{:?}", r) };
    frames.iter().map(|f| (f.get("name").str().unwrap().to_string(), if let Json::Num(n) = f.get("line") { *n as i64 } else { -1 })).collect()
}

fn stopped(c: &mut Client, reason: &str) {
    let e = c.event("stopped");
    assert_eq!(e.get("body").get("reason").str(), Some(reason), "{:?}", e);
}

#[test]
fn a_breakpoint_stops_the_program_shows_the_stack_and_continue_runs_it_to_the_end() {
    let Some((mut c, dir, file)) = session(PROGRAM) else { return };
    let b = breakpoints(&mut c, &file, &[4]);
    assert_eq!((b[0].get("verified"), b[0].get("line")), (&Json::Bool(true), &Json::Num(4.0)));
    c.request("configurationDone", "{}");
    stopped(&mut c, "breakpoint");
    // the stack names the Zila functions with their lines: `add` at its breakpoint, `main` at the call
    let st = stack(&mut c);
    assert_eq!(st, [("add".to_string(), 4), ("main".to_string(), 9)]);
    let r = c.request("stackTrace", r#"{"threadId":1}"#);
    let Json::Arr(frames) = r.get("body").get("stackFrames") else { panic!() };
    assert_eq!(frames[0].get("source").get("path").str(), Some(file.to_string_lossy().as_ref()));
    // continue: the program ends and says so
    c.request("continue", r#"{"threadId":1}"#);
    let e = c.event("exited");
    assert_eq!(e.get("body").get("exitCode"), &Json::Num(0.0));
    c.event("terminated");
    assert_eq!(c.program_output().trim(), "sum 7 14");
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_breakpoint_on_a_line_without_code_moves_to_the_next_line_that_has_some() {
    let Some((mut c, dir, file)) = session(PROGRAM) else { return };
    // line 7 is blank, line 8 is `func main()`: the first statement is on line 9
    let b = breakpoints(&mut c, &file, &[7, 5]);
    assert_eq!((b[0].get("verified"), b[0].get("line")), (&Json::Bool(true), &Json::Num(9.0)));
    assert!(b[0].get("message").str().unwrap().contains("moved"), "{:?}", b[0]);
    assert_eq!(b[1].get("line"), &Json::Num(5.0));
    assert!(b[1].get("message") == &Json::Null, "a line that has code is not moved");
    // a line after the last statement has nowhere to move to
    let b = breakpoints(&mut c, &file, &[40]);
    assert_eq!(b[0].get("verified"), &Json::Bool(false));
    // the program stops at the moved breakpoint
    breakpoints(&mut c, &file, &[7]);
    c.request("configurationDone", "{}");
    stopped(&mut c, "breakpoint");
    assert_eq!(stack(&mut c)[0], ("main".to_string(), 9));
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_three_steps_visit_the_lines_in_order() {
    let Some((mut c, dir, file)) = session(PROGRAM) else { return };
    breakpoints(&mut c, &file, &[9]);
    c.request("configurationDone", "{}");
    stopped(&mut c, "breakpoint");
    assert_eq!(stack(&mut c)[0], ("main".to_string(), 9));
    // step into `add(3, 4)`: the first statement of `add`
    c.request("stepIn", r#"{"threadId":1}"#);
    stopped(&mut c, "step");
    assert_eq!(stack(&mut c), [("add".to_string(), 4), ("main".to_string(), 9)]);
    // next: the next statement of `add`
    c.request("next", r#"{"threadId":1}"#);
    stopped(&mut c, "step");
    assert_eq!(stack(&mut c)[0], ("add".to_string(), 5));
    // step out: back in `main`, in the statement that called
    c.request("stepOut", r#"{"threadId":1}"#);
    stopped(&mut c, "step");
    assert_eq!(stack(&mut c), [("main".to_string(), 9)]);
    // next steps over the rest of the call and goes on to the next lines
    c.request("next", r#"{"threadId":1}"#);
    stopped(&mut c, "step");
    assert_eq!(stack(&mut c)[0], ("main".to_string(), 10));
    c.request("next", r#"{"threadId":1}"#);
    stopped(&mut c, "step");
    assert_eq!(stack(&mut c)[0], ("main".to_string(), 11));
    // next over `print`, which is the runtime's: then the function ends and the program with it
    c.request("next", r#"{"threadId":1}"#);
    c.event("exited");
    c.event("terminated");
    assert_eq!(c.program_output().trim(), "sum 7 14");
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn next_steps_over_a_call_and_a_loop_runs_its_lines_each_time() {
    let src = "package main\n\nfunc twice(n: int) -> int\n    return n * 2\nend\n\nfunc main()\n    var total = 0\n    for i in 0..2\n        total += twice(i)\n    end\n    print(\"{total}\")\nend\n";
    let Some((mut c, dir, file)) = session(src) else { return };
    breakpoints(&mut c, &file, &[8]);
    c.request("configurationDone", "{}");
    stopped(&mut c, "breakpoint");
    let mut lines = Vec::new();
    for _ in 0..4 {
        c.request("next", r#"{"threadId":1}"#);
        stopped(&mut c, "step");
        let st = stack(&mut c);
        assert_eq!(st.len(), 1, "stepping over a call stays in `main`: {:?}", st);
        lines.push(st[0].1);
    }
    // `for` (9), the body (10) twice, since the loop runs twice, then `print` (12)
    assert_eq!(lines, [9, 10, 10, 12]);
    // the last statement is stepped over, `main` ends and so does the program
    c.request("next", r#"{"threadId":1}"#);
    c.event("exited");
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pause_stops_a_running_program() {
    let src = "package main\n\nfunc main()\n    var n = 0\n    while true\n        n += 1\n    end\nend\n";
    let Some((mut c, dir, _file)) = session(src) else { return };
    c.request("configurationDone", "{}");
    std::thread::sleep(Duration::from_millis(200));
    c.request("pause", r#"{"threadId":1}"#);
    stopped(&mut c, "pause");
    let st = stack(&mut c);
    assert_eq!(st[0].0, "main");
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_program_that_does_not_compile_fails_the_launch_with_the_error() {
    let dir = scratch();
    let file = dir.join("bad.zila");
    std::fs::write(&file, "package main\n\nfunc main()\n    let x: int = \"no\"\nend\n").unwrap();
    let mut c = Client::start();
    c.request("initialize", "{}");
    let r = c.request("launch", &format!(r#"{{"program":"{}"}}"#, json_path(&file)));
    assert_eq!(r.get("success"), &Json::Bool(false));
    assert!(r.get("message").str().unwrap().contains("expected int, found text"), "{:?}", r);
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- values ----

const KINDS: &str = "package main\n\ntype Point = record\n    x: int\n    y: int\nend\n\ntype Shape = enum\n    Circle(r: int)\n    Empty\nend\n\nfunc main()\n    let n = 42\n    let big: long = 5000000000\n    let f = 2.5\n    let b = true\n    let c = 'z'\n    let t = \"hello\"\n    let p = Point{x: 1, y: 2}\n    let xs = [10, 20, 30]\n    let m = {\"a\": 1, \"b\": 2}\n    let sh = Circle(7)\n    let e = Empty\n    print(\"done\")\nend\n";

/// (name, value, type, reference) of the variables a reference lists.
fn variables(c: &mut Client, reference: f64) -> Vec<(String, String, String, f64)> {
    let r = c.request("variables", &format!(r#"{{"variablesReference":{}}}"#, reference));
    assert_eq!(r.get("success"), &Json::Bool(true), "{:?}", r);
    let Json::Arr(v) = r.get("body").get("variables") else { panic!() };
    v.iter()
        .map(|x| {
            let Json::Num(n) = x.get("variablesReference") else { panic!() };
            (x.get("name").str().unwrap().to_string(), x.get("value").str().unwrap().to_string(), x.get("type").str().unwrap().to_string(), *n)
        })
        .collect()
}

fn frame_variables(c: &mut Client, frame: i64) -> Vec<(String, String, String, f64)> {
    let r = c.request("scopes", &format!(r#"{{"frameId":{}}}"#, frame));
    let Json::Arr(scopes) = r.get("body").get("scopes") else { panic!() };
    let Json::Num(reference) = scopes[0].get("variablesReference") else { panic!() };
    variables(c, *reference)
}

fn eval(c: &mut Client, expr: &str) -> Json {
    c.request("evaluate", &format!(r#"{{"expression":"{}","frameId":0,"context":"repl"}}"#, expr.replace('"', "\\\"")))
}

#[test]
fn every_kind_of_local_is_shown_with_its_type_and_value_and_expands_into_its_parts() {
    let Some((mut c, dir, file)) = session(KINDS) else { return };
    breakpoints(&mut c, &file, &[25]); // `print("done")`
    c.request("configurationDone", "{}");
    stopped(&mut c, "breakpoint");
    let vars = frame_variables(&mut c, 0);
    let get = |name: &str| vars.iter().find(|v| v.0 == name).unwrap_or_else(|| panic!("no variable {} in {:?}", name, vars)).clone();
    assert_eq!((get("n").1.as_str(), get("n").2.as_str()), ("42", "int"));
    assert_eq!((get("big").1.as_str(), get("big").2.as_str()), ("5000000000", "long"));
    assert_eq!((get("f").1.as_str(), get("f").2.as_str()), ("2.5", "float"));
    assert_eq!((get("b").1.as_str(), get("b").2.as_str()), ("true", "bool"));
    assert_eq!((get("c").1.as_str(), get("c").2.as_str()), ("'z'", "char"));
    assert_eq!((get("t").1.as_str(), get("t").2.as_str()), ("\"hello\"", "text"));
    assert_eq!(get("p").1, "Point{x: 1, y: 2}");
    assert_eq!(get("p").2, "Point");
    assert_eq!(get("xs").1, "[10, 20, 30]");
    assert_eq!(get("xs").2, "[]int");
    assert_eq!(get("m").1, "{2 entries}");
    assert_eq!(get("m").2, "Map<text, int>");
    assert_eq!((get("sh").1.as_str(), get("sh").2.as_str()), ("Circle(7)", "Shape"));
    assert_eq!(get("e").1, "Empty");
    // scalars have no parts, the others expand
    assert_eq!(get("n").3, 0.0);
    let parts = variables(&mut c, get("p").3);
    assert_eq!(parts.iter().map(|p| (p.0.as_str(), p.1.as_str())).collect::<Vec<_>>(), [("x", "1"), ("y", "2")]);
    let items = variables(&mut c, get("xs").3);
    assert_eq!(items.iter().map(|p| p.1.as_str()).collect::<Vec<_>>(), ["10", "20", "30"]);
    let entries = variables(&mut c, get("m").3);
    let mut e: Vec<(String, String)> = entries.iter().map(|p| (p.0.clone(), p.1.clone())).collect();
    e.sort();
    assert_eq!(e, [("\"a\"".to_string(), "1".to_string()), ("\"b\"".to_string(), "2".to_string())]);
    let payload = variables(&mut c, get("sh").3);
    assert_eq!((payload[0].1.as_str(), payload[0].2.as_str()), ("7", "int"));
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_debug_console_evaluates_names_fields_and_indexes() {
    let Some((mut c, dir, file)) = session(KINDS) else { return };
    breakpoints(&mut c, &file, &[25]);
    c.request("configurationDone", "{}");
    stopped(&mut c, "breakpoint");
    let result = |c: &mut Client, e: &str| {
        let r = eval(c, e);
        assert_eq!(r.get("success"), &Json::Bool(true), "{}: {:?}", e, r);
        r.get("body").get("result").str().unwrap().to_string()
    };
    assert_eq!(result(&mut c, "n"), "42");
    assert_eq!(result(&mut c, "p.x"), "1");
    assert_eq!(result(&mut c, "xs[1]"), "20");
    assert_eq!(result(&mut c, "xs.len"), "3");
    assert_eq!(result(&mut c, "t.len"), "5");
    assert_eq!(result(&mut c, "m[\"b\"]"), "2");
    assert_eq!(result(&mut c, "sh"), "Circle(7)");
    // what cannot be evaluated is an error with a message, not a wrong value
    let r = eval(&mut c, "nosuch");
    assert_eq!(r.get("success"), &Json::Bool(false));
    assert!(r.get("message").str().unwrap().contains("not a variable"), "{:?}", r);
    assert_eq!(eval(&mut c, "p.z").get("success"), &Json::Bool(false));
    assert_eq!(eval(&mut c, "xs[9]").get("success"), &Json::Bool(false));
    assert_eq!(eval(&mut c, "n + 1").get("success"), &Json::Bool(false));
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_local_the_optimizer_keeps_in_a_register_is_unavailable_in_the_frames_above() {
    let src = "package main\n\nfunc work(k: int) -> int\n    var total = 0\n    for i in 0..k\n        total += i\n    end\n    return total\nend\n\nfunc main()\n    var sum = 0\n    for j in 0..3\n        sum += work(j)\n    end\n    print(\"{sum}\")\nend\n";
    let Some((mut c, dir, file)) = session(src) else { return };
    breakpoints(&mut c, &file, &[6]); // `total += i`, inside `work`
    c.request("configurationDone", "{}");
    stopped(&mut c, "breakpoint");
    assert_eq!(stack(&mut c).len(), 2);
    let inner = frame_variables(&mut c, 0);
    let outer = frame_variables(&mut c, 1);
    // in the function stopped in every variable is readable
    assert!(inner.iter().all(|v| v.1 != "unavailable"), "{:?}", inner);
    assert!(inner.iter().any(|v| v.0 == "k"));
    // in the caller, a variable that lives in a register is unavailable, and none shows a wrong value
    assert!(outer.iter().any(|v| v.1 == "unavailable"), "{:?}", outer);
    for v in &outer {
        assert!(v.1 == "unavailable" || v.1.parse::<i64>().is_ok(), "{:?}", v);
    }
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_abort_stops_the_program_with_its_message_and_the_stack_of_the_call() {
    let src = "package main\n\nfunc check(n: int)\n    if n > 2\n        abort(\"n is too big\")\n    end\nend\n\nfunc main()\n    check(1)\n    check(3)\n    print(\"not reached\")\nend\n";
    let Some((mut c, dir, _file)) = session(src) else { return };
    c.request("configurationDone", "{}");
    let e = c.event("stopped");
    assert_eq!(e.get("body").get("reason").str(), Some("exception"), "{:?}", e);
    let text = e.get("body").get("description").str().unwrap().to_string();
    assert!(text.contains("abort") && text.contains("n is too big"), "{}", text);
    // the stack names the functions of the call: where it aborted, and who called
    let st = stack(&mut c);
    assert_eq!(st.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["check", "main"], "{:?}", st);
    assert_eq!(st[0].1, 5);
    assert_eq!(st[1].1, 11);
    // going on lets the program end the way it would: status 101
    c.request("continue", r#"{"threadId":1}"#);
    let e = c.event("exited");
    assert_eq!(e.get("body").get("exitCode"), &Json::Num(101.0));
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_stops_the_program_as_an_exception() {
    let src = "package main\n\nfunc main()\n    unsafe\n        let p: *int = 0\n        print(\"{p^}\")\n    end\nend\n";
    let Some((mut c, dir, _file)) = session(src) else { return };
    c.request("configurationDone", "{}");
    let e = c.event("stopped");
    assert_eq!(e.get("body").get("reason").str(), Some("exception"));
    assert!(e.get("body").get("description").str().unwrap().contains("access violation"), "{:?}", e);
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- platforms ----

/// A program of the other operating system, built here and not run.
fn foreign_program(dir: &std::path::Path) -> PathBuf {
    let file = parse_program();
    if cfg!(windows) {
        let out = dir.join("linux-program");
        zila::codegen_x64::compile_to_linux(&file, out.to_str().unwrap(), &zila::codegen_x64::CompileOptions { optimize: true, test_mode: false, debug: true }).unwrap();
        out
    } else {
        let out = dir.join("windows-program.exe");
        zila::codegen_x64::compile_to_native_with(&file, out.to_str().unwrap(), &zila::codegen_x64::CompileOptions { optimize: true, test_mode: false, debug: true }).unwrap();
        out
    }
}

fn parse_program() -> zila::ast::File {
    zila::parse_source(PROGRAM).unwrap()
}

#[test]
fn a_program_of_the_other_operating_system_fails_the_launch_with_a_message_that_says_so() {
    let dir = scratch();
    let program = foreign_program(&dir);
    let mut c = Client::start();
    c.request("initialize", "{}");
    let r = c.request("launch", &format!(r#"{{"program":"{}"}}"#, json_path(&program)));
    assert_eq!(r.get("success"), &Json::Bool(false), "{:?}", r);
    let message = r.get("message").str().unwrap().to_string();
    let (other, here) = if cfg!(windows) { ("Linux", "Windows") } else { ("Windows", "Linux") };
    assert!(message.contains(other) && message.contains(here) && message.contains("cannot be debugged"), "{}", message);
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn asking_for_the_other_target_fails_the_launch_too() {
    let dir = scratch();
    let file = dir.join("prog.zila");
    std::fs::write(&file, PROGRAM).unwrap();
    let mut c = Client::start();
    c.request("initialize", "{}");
    let target = if cfg!(windows) { "linux" } else { "windows" };
    let r = c.request("launch", &format!(r#"{{"program":"{}","target":"{}"}}"#, json_path(&file), target));
    assert_eq!(r.get("success"), &Json::Bool(false));
    assert!(r.get("message").str().unwrap().contains(target) && r.get("message").str().unwrap().contains("cannot be debugged"), "{:?}", r);
    c.close();
    let _ = std::fs::remove_dir_all(&dir);
}
