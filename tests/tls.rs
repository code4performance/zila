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

// ---------------------------------------------------------------------------
// Upgrading an open connection, ALPN and a private CA. They need the TLS servers of
// `tests/docker` (`docker compose -f tests/docker/compose.yml up -d --wait`, then source
// `tests/docker/env.ps1`); without `BRASA_TEST_CA` they print a skip line.
// ---------------------------------------------------------------------------

fn ca_path() -> Option<String> {
    match std::env::var("BRASA_TEST_CA") {
        Ok(p) if !p.is_empty() => Some(p.replace('\\', "/")),
        _ => {
            eprintln!("skipped: set BRASA_TEST_CA (see tests/docker/README.md) to run this test");
            None
        }
    }
}

const SSL_REQUEST: &str = "crypto.from_bytes([0, 0, 0, 8, 4, 210, 22, 47])";

fn postgres_program(upgrade: &str) -> String {
    format!(
        r#"package main

use net
use tls
use crypto

func main() -> error
    let raw = try await net.connect("127.0.0.1", 55432)
    try await raw.write({ssl})
    let answer = try await raw.read(1)
    print(answer)
{upgrade}
    return
end
"#,
        ssl = SSL_REQUEST,
        upgrade = upgrade
    )
}

#[test]
fn upgrade_starts_tls_after_a_plain_greeting() {
    let Some(ca) = ca_path() else { return };
    // after the upgrade, a Startup message gets an authentication request ('R') back, encrypted
    let tail = r#"    let opts = tls.Options{ alpn: [], require_alpn: false, ca_file: "@CA@" }
    let conn = try await tls.upgrade_with(raw, "localhost", opts)
    let startup = crypto.from_bytes([0, 0, 0, 32, 0, 3, 0, 0]) + "user" + crypto.from_bytes([0]) + "app" + crypto.from_bytes([0]) + "database" + crypto.from_bytes([0]) + "shop" + crypto.from_bytes([0, 0])
    try await conn.write(startup)
    let reply = try await conn.read(1)
    print(reply)
    print(conn.protocol() == "")"#
        .replace("@CA@", &ca);
    let Some(out) = run(&postgres_program(&tail)) else { return };
    assert_eq!(out.replace('\r', ""), "S\nR\ntrue\n", "{}", out);
}

#[test]
fn upgrade_fails_for_an_untrusted_or_wrongly_named_certificate_and_closes() {
    let Some(ca) = ca_path() else { return };
    // no CA file: the development CA is not trusted by the system
    let untrusted = "    match await tls.upgrade(raw, \"localhost\")\n        ok(c) -> print(\"unexpected success\")\n        err(e) -> print(\"refused\")\n    end\n    let after = await raw.read(1)\n    print(\"closed\")";
    let Some(out) = run(&postgres_program(untrusted)) else { return };
    assert!(out.starts_with("S\nrefused\n"), "{}", out);
    // the CA is given but the name is wrong
    let wrong = r#"    let opts = tls.Options{ alpn: [], require_alpn: false, ca_file: "@CA@" }
    match await tls.upgrade_with(raw, "wrong.example", opts)
        ok(c) -> print("unexpected success")
        err(e) -> print(e.message)
    end"#
        .replace("@CA@", &ca);
    let Some(out) = run(&postgres_program(&wrong)) else { return };
    assert!(out.starts_with("S\n") && out.contains("does not match the host name") || out.contains("not match"), "{}", out);
    assert!(!out.contains("unexpected success"), "{}", out);
}

#[test]
fn a_missing_ca_file_is_an_error_naming_the_path() {
    let Some(_ca) = ca_path() else { return };
    let tail = r#"    let opts = tls.Options{ alpn: [], require_alpn: false, ca_file: "C:/no/such/ca.pem" }
    match await tls.upgrade_with(raw, "localhost", opts)
        ok(c) -> print("unexpected success")
        err(e) -> print(e.message)
    end"#;
    let Some(out) = run(&postgres_program(tail)) else { return };
    assert!(out.contains("no/such/ca.pem"), "{}", out);
}

fn alpn_program(port: u16, alpn: &str, require: bool, ca: &str) -> String {
    format!(
        r#"package main

use tls

func main() -> error
    let opts = tls.Options{{ alpn: {alpn}, require_alpn: {require}, ca_file: "{ca}" }}
    match await tls.connect_with("localhost", {port}, opts)
        ok(conn) ->
            print("protocol=" + conn.protocol())
            try await conn.write("GET / HTTP/1.0\r\n\r\n")
            let head = try await conn.read(12)
            print(head)
        err(e) -> print(e.message)
    end
    return
end
"#
    )
}

#[test]
fn alpn_reports_the_protocol_the_server_selected() {
    let Some(ca) = ca_path() else { return };
    let Some(out) = run(&alpn_program(55444, "[\"tds/8.0\", \"http/1.1\"]", false, &ca)) else { return };
    assert!(out.starts_with("protocol=http/1.1\nHTTP/1.0 200"), "{}", out);
}

#[test]
fn alpn_required_and_absent_fails_naming_the_offered_protocols() {
    let Some(ca) = ca_path() else { return };
    // a server that ignores ALPN: connects with an empty protocol unless it is required
    let Some(out) = run(&alpn_program(55443, "[\"tds/8.0\"]", false, &ca)) else { return };
    assert!(out.starts_with("protocol=\n"), "{}", out);
    let Some(out) = run(&alpn_program(55443, "[\"tds/8.0\", \"http/1.1\"]", true, &ca)) else { return };
    assert!(out.contains("tds/8.0, http/1.1") && out.contains("none of the offered protocols"), "{}", out);
}

#[test]
fn a_private_ca_is_trusted_only_when_given_and_connect_is_unchanged() {
    let Some(ca) = ca_path() else { return };
    let with_ca = alpn_program(55443, "[]", false, &ca);
    let Some(out) = run(&with_ca) else { return };
    assert!(out.starts_with("protocol=\nHTTP/1.0 200"), "{}", out);
    let without = alpn_program(55443, "[]", false, "");
    let Some(out) = run(&without) else { return };
    assert!(out.contains("failed") && !out.contains("protocol="), "{}", out);
}
