//! TLS client (`use tls`). A real HTTPS request is made to a public host; the test is skipped when
//! there is no network or no native toolchain. Bad certificates are checked against badssl.com.

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn run(src: &str) -> Option<String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-tls-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") || e.contains("vcvars64") => {
            eprintln!("no native toolchain: skipping");
            return None;
        }
        Err(e) => panic!("compile error: {}", e),
    }
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    Some(String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', ""))
}

fn online() -> bool {
    use std::net::ToSocketAddrs;
    "example.com:443".to_socket_addrs().map(|mut a| a.next().is_some()).unwrap_or(false)
}

#[test]
fn https_request() {
    if !online() {
        eprintln!("offline: skipping");
        return;
    }
    let src = r#"package main

use tls

func main() -> error
    let conn = try await tls.connect("example.com", 443)
    try await conn.write("GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
    var all = ""
    while true
        let part = try await conn.read(4096)
        if part.len == 0
            break
        end
        all = all + part
    end
    print(all.slice(0, 15))
    print(all.len > 500)
    return
end
"#;
    let Some(out) = run(src) else { return };
    assert!(out.starts_with("HTTP/1.1 200 OK") || out.starts_with("HTTP/1.1 "), "{}", out);
    assert!(out.contains("true"), "{}", out);
}

#[test]
fn bad_certificates_are_refused() {
    if !online() {
        return;
    }
    let src = r#"package main

use tls

func main() -> error
    match await tls.connect("expired.badssl.com", 443)
        ok(c) -> print("unexpected success")
        err(e) -> print("refused")
    end
    return
end
"#;
    let Some(out) = run(src) else { return };
    assert_eq!(out.trim(), "refused");
}
