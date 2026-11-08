//! The `postgres` driver (spec `postgres-driver`). A fake PostgreSQL server written in Brasa, in
//! the same program as the client, covers the failure cases (refused TLS, wrong passwords, forged
//! signatures, errors in the middle, row limits, a server that goes away). Tests against the real
//! server of `tests/docker` run when `BRASA_TEST_POSTGRES` is set and print a skip line otherwise.

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("brasa-pg-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        brasa::comptime::set_cache_dir(&dir);
    });
}

fn build(src: &str) -> Result<std::path::PathBuf, String> {
    setup();
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-pg-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap()).map(|_| exe)
}

fn run(src: &str) -> String {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "")
}

/// The fake server. `mode` picks the login: `trust`, `md5`, `cleartext`, `scram`, `scram-forged`,
/// `gss`, `no-tls` (answers N to an SSLRequest), `close-after-first` (trust, then hangs up after
/// the first statement). Statements: `select 1`, `echo` (the bound parameters joined by `|`),
/// `boom` (an error), `many` (ten rows), `update` (three rows affected), `sleepy` (answers after
/// 300 ms), anything else an empty result.
const FAKE: &str = r##"
use net
use crypto
use text
use task

#[shared]
type Fake = record
    mode: text
    password: text
    log: []text
end

#[shared]
type Wire = record
    conn: net.Conn
    buf: text
    eof: bool
end

func be32(n: long) -> text
    return crypto.from_bytes([(n >> 24) bitand 255, (n >> 16) bitand 255, (n >> 8) bitand 255, n bitand 255])
end

func be16(n: long) -> text
    return crypto.from_bytes([(n >> 8) bitand 255, n bitand 255])
end

func nul() -> text
    return crypto.from_bytes([0])
end

func get32(s: text, at: int) -> long
    var v: long = (long(s.byte(at)) << 24) bitor (long(s.byte(at + 1)) << 16) bitor (long(s.byte(at + 2)) << 8) bitor long(s.byte(at + 3))
    if v >= 2147483648
        v = v - 4294967296
    end
    return v
end

func frame(kind: int, body: text) -> text
    return crypto.from_bytes([long(kind)]) + be32(long(body.len) + 4) + body
end

func want(w: Wire, n: int) -> bool
    while w.buf.len < n
        let more = match await w.conn.read(4096)
            ok(m) -> m
            err(e) -> ""
        end
        if more.len == 0
            w.eof = true
            return false
        end
        w.buf = w.buf + more
    end
    return true
end

func grab(w: Wire, n: int) -> text
    let out = w.buf.__raw_slice(0, n)
    w.buf = w.buf.__raw_slice(n, w.buf.len)
    return out
end

func send(w: Wire, data: text)
    let r = await w.conn.write(data)
end

func fail(w: Wire, code: text, message: text)
    send(w, frame(69, "S" + "ERROR" + nul() + "C" + code + nul() + "M" + message + nul() + nul()))
end

func ready(w: Wire)
    send(w, frame(90, "I"))
end

// One password-type message ('p'): its body.
func read_p(w: Wire) -> text
    if not want(w, 5)
        return ""
    end
    let head = grab(w, 5)
    let len = int(get32(head, 1))
    if not want(w, len - 4)
        return ""
    end
    return grab(w, len - 4)
end

func cstring_end(s: text, from: int) -> int
    var z = from
    while z < s.len and s.byte(z) != 0
        z += 1
    end
    return z
end

func login(w: Wire, f: Fake, user: text) -> bool
    let mode = f.mode
    if mode == "md5"
        let salt = "abcd"
        send(w, frame(82, be32(5) + salt))
        let p = read_p(w)
        let want_hash = "md5" + crypto.hex(crypto.md5(crypto.hex(crypto.md5(f.password + user)) + salt))
        if p.slice(0, cstring_end(p, 0)) != want_hash
            fail(w, "28P01", "password authentication failed for user")
            return false
        end
        f.log.add("md5 ok")
        send(w, frame(82, be32(0)))
        return true
    end
    if mode == "cleartext"
        send(w, frame(82, be32(3)))
        let p = read_p(w)
        if p == ""
            return false
        end
        f.log.add("password received")
        send(w, frame(82, be32(0)))
        return true
    end
    if mode == "gss"
        send(w, frame(82, be32(7)))
        return false
    end
    if mode == "scram" or mode == "scram-forged"
        send(w, frame(82, be32(10) + "SCRAM-SHA-256" + nul() + nul()))
        let p = read_p(w)
        if p == ""
            return false
        end
        let z = cstring_end(p, 0)
        let len = int(get32(p, z + 1))
        let first = p.slice(z + 5, z + 5 + len)
        // first = "n,,n=,r=<nonce>"
        let bare = first.slice(3, first.len)
        let cnonce = bare.slice(bare.byte(0) == 110 and 5 or 0, bare.len)
        let nonce = cnonce + "srvpart"
        let salt = "saltsaltsalt"
        let server_first = "r=" + nonce + ",s=" + crypto.base64_encode(salt) + ",i=4096"
        send(w, frame(82, be32(11) + server_first))
        let q = read_p(w)
        if q == ""
            return false
        end
        let final = q
        let pp = text.index_of(final, ",p=")
        let without = final.slice(0, pp)
        let proof = match crypto.base64_decode(final.slice(pp + 3, final.len))
            ok(b) -> b
            err(e) -> ""
        end
        let salted = match crypto.pbkdf2_sha256(f.password, salt, 4096, 32)
            ok(k) -> k
            err(e) -> ""
        end
        let client_key = crypto.hmac_sha256(salted, "Client Key")
        let stored = crypto.sha256(client_key)
        let auth = bare + "," + server_first + "," + without
        let sig = crypto.hmac_sha256(stored, auth)
        var recovered: []long = []
        for k in 0..32
            recovered.add(long(proof.byte(k)) bitxor long(sig.byte(k)))
        end
        if not crypto.equal(crypto.sha256(crypto.from_bytes(recovered)), stored)
            fail(w, "28P01", "password authentication failed for user")
            return false
        end
        let server_key = crypto.hmac_sha256(salted, "Server Key")
        var server_sig = crypto.hmac_sha256(server_key, auth)
        if mode == "scram-forged"
            server_sig = crypto.hmac_sha256(server_key, auth + "x")
        end
        send(w, frame(82, be32(12) + "v=" + crypto.base64_encode(server_sig)))
        send(w, frame(82, be32(0)))
        return true
    end
    send(w, frame(82, be32(0)))
    return true
end

func row_desc(names: []text) -> text
    var body = be16(long(names.len))
    for n in names
        body = body + n + nul() + be32(0) + be16(0) + be32(25) + be16(-1) + be32(-1) + be16(0)
    end
    return frame(84, body)
end

func data_row(cells: []text) -> text
    var body = be16(long(cells.len))
    for c in cells
        body = body + be32(long(c.len)) + c
    end
    return frame(68, body)
end

// Answers one statement (a Parse, Bind, Describe, Execute, Sync batch).
let REPLAY: Map<text, text> = @REPLAY@

func answer(w: Wire, f: Fake, sql: text, params: []text, count: int)
    if f.mode == "replay"
        var key = "boom"
        if text.contains(sql, "from customers")
            key = "customers"
        else if text.contains(sql, "true as b")
            key = "types"
        end
        match crypto.unhex(REPLAY[key])
            ok(b) -> send(w, b)
            err(e) -> f.log.add("bad replay")
        end
        return
    end
    send(w, frame(49, ""))
    send(w, frame(50, ""))
    if sql == "boom"
        fail(w, "23505", "duplicate key value violates unique constraint")
        ready(w)
        return
    end
    if sql == "select 1"
        send(w, row_desc(["n"]) + data_row(["1"]) + frame(67, "SELECT 1" + nul()))
    else if sql == "echo"
        send(w, row_desc(["args"]) + data_row([text.join(params, "|")]) + frame(67, "SELECT 1" + nul()))
    else if sql == "many"
        send(w, row_desc(["n"]))
        for k in 0..10
            send(w, data_row(["{k}"]))
        end
        send(w, frame(67, "SELECT 10" + nul()))
    else if sql == "update"
        send(w, frame(110, "") + frame(67, "UPDATE 3" + nul()))
    else if sql == "sleepy"
        await sleep(300)
        send(w, row_desc(["n"]) + data_row(["1"]) + frame(67, "SELECT 1" + nul()))
    else
        send(w, frame(110, "") + frame(67, sql + nul()))
    end
    ready(w)
end

func serve(conn: net.Conn, f: Fake)
    let w = Wire { conn: conn, buf: "", eof: false }
    if not want(w, 8)
        return
    end
    if get32(w.buf, 4) == 80877103
        grab(w, 8)
        f.log.add("ssl request")
        send(w, "N")
        if f.mode == "no-tls"
            if want(w, 8)
                f.log.add("client continued after N")
            end
            return
        end
        if not want(w, 8)
            return
        end
    end
    let len = int(get32(w.buf, 0))
    if not want(w, len)
        return
    end
    let startup = grab(w, len)
    var user = ""
    var at = 8
    while at < startup.len and startup.byte(at) != 0
        let k_end = cstring_end(startup, at)
        let key = startup.slice(at, k_end)
        let v_end = cstring_end(startup, k_end + 1)
        if key == "user"
            user = startup.slice(k_end + 1, v_end)
        end
        at = v_end + 1
    end
    if not login(w, f, user)
        return
    end
    send(w, frame(83, "server_version" + nul() + "19" + nul()) + frame(75, be32(1) + be32(2)))
    ready(w)
    var sql = ""
    var params: []text = []
    var served = 0
    while true
        if not want(w, 5)
            return
        end
        let head = grab(w, 5)
        let kind = int(head.byte(0))
        let body_len = int(get32(head, 1)) - 4
        if not want(w, body_len)
            return
        end
        let body = grab(w, body_len)
        if kind == 88
            return
        end
        if kind == 80
            let z = cstring_end(body, 0)
            let z2 = cstring_end(body, z + 1)
            sql = body.slice(z + 1, z2)
        end
        if kind == 66
            params = []
            let z = cstring_end(body, 0)
            let z2 = cstring_end(body, z + 1)
            var p = z2 + 1
            let nfmt = int(body.byte(p)) * 256 + int(body.byte(p + 1))
            p += 2 + nfmt * 2
            let n = int(body.byte(p)) * 256 + int(body.byte(p + 1))
            p += 2
            for k in 0..n
                let l = int(get32(body, p))
                p += 4
                if l < 0
                    params.add("<null>")
                else
                    params.add(body.__raw_slice(p, p + l))
                    p += l
                end
            end
        end
        if kind == 83
            served += 1
            answer(w, f, sql, params, served)
            if f.mode == "close-after-first" and served == 1
                await conn.close()
                return
            end
        end
    end
end

func accept_loop(listener: net.Listener, f: Fake)
    while true
        match await listener.accept()
            ok(c) -> spawn serve(c, f)
            err(e) -> return
        end
    end
end
"##;

fn replay_map() -> String {
    let data = std::fs::read_to_string("tests/data/postgres-exchange.txt").expect("run tests/docker/record_postgres.py");
    let items: Vec<String> = data
        .lines()
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            format!("\"{}\": \"{}\"", k, v)
        })
        .collect();
    format!("{{ {} }}", items.join(", "))
}

fn program(mode: &str, password: &str, body: &str) -> String {
    let fake = FAKE.replace("@REPLAY@", &replay_map());
    format!(
        "package main\n\nuse db\nuse postgres\n{fake}\nfunc main() -> error\n    let f = Fake {{ mode: \"{mode}\", password: \"{password}\", log: [] }}\n    let listener = try await net.listen(0)\n    spawn accept_loop(listener, f)\n    let port = listener.port\n{body}\n    return\nend\n"
    )
}

fn url(extra: &str) -> String {
    format!("    let base = \"postgres://app:secret@127.0.0.1:{{port}}/shop?sslmode=disable{extra}\"\n")
}

#[test]
fn plain_open_runs_a_query_with_trust_login() {
    let body = format!(
        "{}    let conn = try postgres.open(base)\n    let rows = try db.query(int, conn, \"select 1\")\n    print(rows[0])\n    print(f.log.len)\n",
        url("").replace("{port}", "{port}")
    );
    assert_eq!(run(&program("trust", "x", &body)), "1\n0\n");
}

#[test]
fn require_against_a_server_that_refuses_tls_fails_before_any_password() {
    let body = "    match postgres.open(\"postgres://app:s3cret@127.0.0.1:{port}/shop\")\n        ok(c) -> print(\"unexpected success\")\n        err(e) -> print(e.message.contains(\"does not accept TLS\"))\n    end\n    await sleep(100)\n    for l in f.log\n        print(l)\n    end\n";
    let out = run(&program("no-tls", "s3cret", body).replace("e.message.contains(\"does not accept TLS\")", "text.contains(e.message, \"does not accept TLS\")"));
    assert_eq!(out, "true\nssl request\n", "{}", out);
}

#[test]
fn a_malformed_url_is_an_error_without_the_password() {
    let body = "    for bad in [\"postgres://app:s3cret@h:x/shop\", \"mysql://app:s3cret@h/shop\", \"postgres://:s3cret@h/shop\", \"postgres://app:s3cret@h/shop?sslmode=prefer\"]\n        match postgres.open(bad)\n            ok(c) -> print(\"unexpected success\")\n            err(e) -> print(e.message)\n        end\n    end\n";
    let out = run(&program("trust", "x", body));
    assert!(!out.contains("s3cret"), "{}", out);
    assert!(out.contains("bad port") && out.contains("not a PostgreSQL URL") && out.contains("no user") && out.contains("unsupported sslmode"), "{}", out);
}

fn login_program(mode: &str, password_used: &str, extra: &str) -> String {
    let body = format!(
        "    match postgres.open(\"postgres://app:{password_used}@127.0.0.1:{{port}}/shop?sslmode=disable{extra}\")\n        ok(c) ->\n            let rows = try db.query(int, c, \"select 1\")\n            print(\"ok\", rows[0])\n        err(e) -> print(e.kind.name + \": \" + e.message)\n    end\n    await sleep(50)\n    for l in f.log\n        print(l)\n    end\n"
    );
    program(mode, "secret", &body)
}

#[test]
fn scram_sha_256_accepts_the_right_password_and_refuses_the_wrong_one() {
    assert_eq!(run(&login_program("scram", "secret", "")), "ok 1\n");
    let out = run(&login_program("scram", "wrong", ""));
    assert!(out.starts_with("std.Unauthenticated: password authentication failed"), "{}", out);
    assert!(!out.contains("wrong\n") || !out.contains("s3cret"), "{}", out);
}

#[test]
fn a_forged_scram_server_signature_is_refused() {
    let out = run(&login_program("scram-forged", "secret", ""));
    assert!(out.starts_with("std.Unauthenticated: the server's SCRAM signature is wrong"), "{}", out);
}

#[test]
fn md5_login_works() {
    assert_eq!(run(&login_program("md5", "secret", "")), "ok 1\nmd5 ok\n");
}

#[test]
fn a_cleartext_password_is_refused_without_tls_and_unknown_methods_are_named() {
    let out = run(&login_program("cleartext", "secret", ""));
    assert!(out.starts_with("std.Unsupported: the server asked for a cleartext password on a connection without TLS"), "{}", out);
    assert!(!out.contains("password received"), "{}", out);
    let out = run(&login_program("gss", "secret", ""));
    assert!(out.contains("authentication method this driver does not support (code 7)"), "{}", out);
}

#[test]
fn parameters_are_bound_not_spliced_and_every_arg_kind_has_a_wire_form() {
    let body = format!(
        "{}    let conn = try postgres.open(base)\n    let rows = try db.query(text, conn, \"echo\", db.int(-7), db.float(1.5), db.bool(true), db.bool(false), db.str(\"x'); drop table t; --\"), db.null(), db.vector([1.0, 2.5]), db.bytes(\"ab\"))\n    print(rows[0])\n",
        url("").replace("{port}", "{port}")
    );
    assert_eq!(run(&program("trust", "x", &body)), "-7|1.5|t|f|x'); drop table t; --|<null>|[1.0,2.5]|\\x6162\n");
}

#[test]
fn a_failing_statement_leaves_the_connection_usable_and_maps_the_sqlstate() {
    let body = format!(
        "{}    let conn = try postgres.open(base)\n    match conn.exec(\"boom\")\n        ok(n) -> print(\"unexpected success\")\n        err(e) -> print(e.kind.name + \": \" + e.message)\n    end\n    let rows = try db.query(int, conn, \"select 1\")\n    print(rows[0])\n    let n = try conn.exec(\"update\")\n    print(n)\n",
        url("").replace("{port}", "{port}")
    );
    assert_eq!(run(&program("trust", "x", &body)), "std.AlreadyExists: duplicate key value violates unique constraint (23505)\n1\n3\n");
}

#[test]
fn the_row_limit_gives_an_error_and_the_connection_stays_usable() {
    let body = format!(
        "{}    let conn = try postgres.open(base)\n    match db.query(int, conn, \"many\")\n        ok(r) -> print(\"unexpected success\")\n        err(e) -> print(e.kind.name + \": \" + e.message)\n    end\n    let rows = try db.query(int, conn, \"select 1\")\n    print(rows[0])\n",
        url("&max_rows=5").replace("{port}", "{port}")
    );
    assert_eq!(run(&program("trust", "x", &body)), "std.Overflow: the statement returned more than 5 rows (the limit of this connection)\n1\n");
}

#[test]
fn a_server_that_goes_away_gives_unavailable_on_the_next_statement() {
    let body = format!(
        "{}    let conn = try postgres.open(base)\n    let rows = try db.query(int, conn, \"select 1\")\n    print(rows[0])\n    match db.query(int, conn, \"select 1\")\n        ok(r) -> print(\"unexpected success\")\n        err(e) -> print(e.kind.name)\n    end\n    match db.query(int, conn, \"select 1\")\n        ok(r) -> print(\"unexpected success\")\n        err(e) -> print(e.kind.name)\n    end\n",
        url("").replace("{port}", "{port}")
    );
    assert_eq!(run(&program("close-after-first", "x", &body)), "1\nstd.Unavailable\nstd.Unavailable\n");
}

#[test]
fn a_slow_statement_does_not_stall_other_tasks() {
    let body = format!(
        "{}    let conn = try postgres.open(base)\n    let ticks = Fake {{ mode: \"\", password: \"\", log: [] }}\n    spawn ticker(ticks)\n    let rows = try db.query(int, conn, \"sleepy\")\n    print(rows[0], ticks.log.len >= 2)\n",
        url("").replace("{port}", "{port}")
    );
    let src = program("trust", "x", &body).replace(
        "func accept_loop(",
        "func ticker(t: Fake)\n    for k in 0..6\n        await sleep(60)\n        t.log.add(\"tick\")\n    end\nend\n\nfunc accept_loop(",
    );
    assert_eq!(run(&src), "1 true\n");
}

// ---------------------------------------------------------------------------
// A real PostgreSQL server (BRASA_TEST_POSTGRES)
//
// Any server will do, local or not, as long as the user may create TEMP tables: every test that
// writes uses a temporary table of its own session, which disappears when the connection closes,
// so nothing is left in the database. Example:
//   BRASA_TEST_POSTGRES=postgres://user:password@127.0.0.1:5432/database?sslmode=disable
// ---------------------------------------------------------------------------

fn real_url() -> Option<String> {
    match std::env::var("BRASA_TEST_POSTGRES") {
        Ok(u) if !u.is_empty() => Some(u.replace('\\', "/")),
        _ => {
            eprintln!("skipped: set BRASA_TEST_POSTGRES to the URL of a PostgreSQL server to run this test");
            None
        }
    }
}

/// The URL with a wrong password.
fn with_bad_password(url: &str) -> String {
    let start = url.find("://").unwrap() + 3;
    let at = url[start..].find('@').unwrap() + start;
    let colon = url[start..at].find(':').map(|c| c + start).unwrap_or(at);
    format!("{}:nope{}", &url[..colon], &url[at..])
}

fn real_program(url: &str, body: &str) -> String {
    format!(
        "package main\n\nuse db\nuse postgres\nuse text\n\ntype Customer = record\n    id: int\n    name: text\n    email: text?\n    city: text?\nend\n\n#[shared]\ntype Ticks = record\n    n: int\nend\n\nfunc tick(t: Ticks)\n    for k in 0..8\n        await sleep(100)\n        t.n += 1\n    end\nend\n\nfunc main() -> error\n    let conn = try postgres.open(\"{url}\")\n    try conn.exec(\"create temp table customers (id int primary key, name text not null, email text, city text)\")\n{body}\n    return\nend\n"
    )
}

const SEED: &str = "    try conn.exec(\"insert into customers values ($1, $2, $3, $4), ($5, $6, $7, $8), ($9, $10, $11, $12)\", db.int(1), db.str(\"Ana Souza\"), db.str(\"ana@example.com\"), db.str(\"Recife\"), db.int(2), db.str(\"João D'Arc\"), db.null(), db.str(\"São Paulo\"), db.int(3), db.str(\"Zoë 日本\"), db.null(), db.null())\n";

#[test]
fn the_real_server_reads_typed_rows_with_nulls_and_unicode() {
    let Some(u) = real_url() else { return };
    let body = format!("{SEED}    let rows = try db.query(Customer, conn, \"select id, name, email, city from customers where id < $1 order by id\", db.int(10))\n    for r in rows\n        print(r.id, r.name, r.email ?? \"-\", r.city ?? \"-\")\n    end\n    let one = try db.one(Customer, conn, \"select * from customers where id = $1\", db.int(3))\n    print(one.name)\n    match db.one(Customer, conn, \"select * from customers where id = $1\", db.int(999))\n        ok(r) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name)\n    end\n    let t = try db.one(Types, conn, \"select true as b, 42::int8 as n, 1.5::float8 as f, 12.50::numeric as d, 'x'::text as t, null::text as z, now()::date as day\")\n    print(t.b, t.n, t.f, t.d, t.t, t.z == nil, t.day.len)\n");
    let src = real_program(&u, &body).replace("func main()", "type Types = record\n    b: bool\n    n: long\n    f: float\n    d: text\n    t: text\n    z: text?\n    day: text\nend\n\nfunc main()");
    assert_eq!(run(&src), "1 Ana Souza ana@example.com Recife\n2 João D'Arc - São Paulo\n3 Zoë 日本 - -\nZoë 日本\nstd.NotFound\ntrue 42 1.5 12.50 x true 10\n");
}

#[test]
fn the_real_server_keeps_injection_text_as_data_maps_unique_violations_and_rolls_back() {
    let Some(u) = real_url() else { return };
    let body = format!("{SEED}    let evil = \"x'); drop table customers; --\"\n    let t = try conn.begin()\n    try t.conn.exec(\"insert into customers(id, name) values ($1, $2)\", db.int(9001), db.str(evil))\n    let back = try db.one(text, t.conn, \"select name from customers where id = $1\", db.int(9001))\n    print(back == evil)\n    match t.conn.exec(\"insert into customers(id, name) values ($1, $2)\", db.int(9001), db.str(\"dup\"))\n        ok(n) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name, text.contains(e.message, \"23505\"))\n    end\n    try t.rollback()\n    print(try db.one(int, conn, \"select count(*)::int from customers where id = 9001\"))\n    print(try db.one(int, conn, \"select count(*)::int from customers\"))\n    let t2 = try conn.begin()\n    try t2.conn.exec(\"update customers set city = $1 where id = $2\", db.str(\"Olinda\"), db.int(1))\n    try t2.commit()\n    print(try db.one(text, conn, \"select city from customers where id = 1\"))\n");
    assert_eq!(run(&real_program(&u, &body)), "true\nstd.AlreadyExists true\n0\n3\nOlinda\n");
}

#[test]
fn the_real_server_finds_the_nearest_vector_when_pgvector_is_installed_and_a_wrong_password_is_refused() {
    let Some(u) = real_url() else { return };
    let body = "    let has = try db.one(int, conn, \"select count(*)::int from pg_available_extensions where name = 'vector'\")\n    if has == 0\n        print(\"no pgvector\")\n        return\n    end\n    try conn.exec(\"create extension if not exists vector\")\n    try conn.exec(\"create temp table items (id int primary key, embedding vector(3))\")\n    try conn.exec(\"insert into items values (1, '[1,0,0]'), (2, '[0,1,0]'), (3, '[0,0,1]')\")\n    let rows = try db.query(int, conn, \"select id from items order by embedding <=> $1 limit 1\", db.vector([0.1, 0.9, 0.0]))\n    print(rows[0])\n    let v = try db.one(Vec, conn, \"select id, embedding from items where id = 1\")\n    print(v.embedding.len, v.embedding[0])\n";
    let src = real_program(&u, body).replace("func main()", "type Vec = record\n    id: int\n    embedding: []float\nend\n\nfunc main()");
    let out = run(&src);
    assert!(out == "no pgvector\n" || out == "2\n3 1.0\n", "{}", out);
    let bad = with_bad_password(&u);
    let src = format!("package main\n\nuse postgres\n\nfunc main()\n    match postgres.open(\"{bad}\")\n        ok(c) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name)\n    end\nend\n");
    assert_eq!(run(&src), "std.Unauthenticated\n");
}

#[test]
fn the_real_server_does_not_stall_other_tasks_during_a_slow_query() {
    let Some(u) = real_url() else { return };
    let body = "    let ticks = Ticks { n: 0 }\n    spawn tick(ticks)\n    let r = try db.one(int, conn, \"select 7 from pg_sleep(0.6)\")\n    print(r, ticks.n >= 4)\n";
    assert_eq!(run(&real_program(&u, body)), "7 true\n");
}

#[test]
fn recorded_answers_of_the_real_server_decode_into_the_expected_values() {
    let body = format!(
        "{}    let conn = try postgres.open(base)\n    let rows = try db.query(Customer, conn, \"select id, name, email, city from customers where id <= $1 order by id\", db.int(4))\n    for r in rows\n        print(r.id, r.name, r.email ?? \"-\", r.city ?? \"-\")\n    end\n    let t = try db.one(Types, conn, \"select true as b, 42::int8 as n, 1.5::float8 as f, 12.50::numeric as d, 'x'::text as t, 'bytes'::bytea as y, null::text as z, now()::date as day\")\n    print(t.b, t.n, t.f, t.d, t.t, crypto.hex(t.y), t.z == nil, t.day.len)\n    match conn.exec(\"insert into customers(id, name) values (1, 'dup')\")\n        ok(n) -> print(\"unexpected success\")\n        err(e) -> print(e.kind.name + \": \" + e.message)\n    end\n",
        url("")
    );
    let types = "type Customer = record\n    id: int\n    name: text\n    email: text?\n    city: text?\nend\n\ntype Types = record\n    b: bool\n    n: long\n    f: float\n    d: text\n    t: text\n    y: text\n    z: text?\n    day: text\nend\n\nfunc main()";
    let src = program("replay", "x", &body).replace("func main()", types).replace("use postgres\n", "use postgres\nuse crypto\n");
    let out = run(&src);
    assert!(out.starts_with("1 Ana Souza ana@example.com Recife\n2 João D'Arc - São Paulo\n3 Zoë 日本 - -\n4 Владимир Петров vp@example.com Москва\ntrue 42 1.5 12.50 x "), "{}", out);
    assert!(out.contains("dead true 10\n") || out.contains(" true 10\n"), "{}", out);
    assert!(out.ends_with("std.AlreadyExists: duplicate key value violates unique constraint \"customers_pkey\" (23505)\n"), "{}", out);
}
