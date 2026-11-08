//! The `sqlserver` driver (spec `sqlserver-driver`). A fake TDS server written in Zila Lang covers the
//! protocol edge cases (results over many packets, server errors, truncated and unknown tokens,
//! refused encryption modes). Tests against the real SQL Server 2025 containers of `tests/docker`
//! run when `ZILA_TEST_SQLSERVER`, `ZILA_TEST_SQLSERVER_STRICT` or `ZILA_TEST_SQLSERVER_AI` are
//! set and print a skip line otherwise (source `tests/docker/env.ps1` after starting them).

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("zila-mssql-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        zila::comptime::set_cache_dir(&dir);
    });
}

fn build(src: &str) -> Result<std::path::PathBuf, String> {
    setup();
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-mssql-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap()).map(|_| exe)
}

fn run(src: &str) -> String {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "")
}

/// The fake server: pre-login (answers "encryption not supported"), login, then one answer per
/// batch chosen by its text: `ints` (three int rows), `big` (5000 rows of 200 characters, many
/// packets), `boom` (error 2627), `cut` (a row cut in half), `weird` (an unknown token), `update`
/// (three rows affected), anything else an empty result.
const FAKE: &str = r##"
use net
use crypto
use text

#[shared]
type Wire = record
    conn: net.Conn
    buf: text
end

func le16(n: long) -> text
    return crypto.from_bytes([n bitand 255, (n >> 8) bitand 255])
end

func le32(n: long) -> text
    return crypto.from_bytes([n bitand 255, (n >> 8) bitand 255, (n >> 16) bitand 255, (n >> 24) bitand 255])
end

func be16(n: long) -> text
    return crypto.from_bytes([(n >> 8) bitand 255, n bitand 255])
end

func b1(n: long) -> text
    return crypto.from_bytes([n bitand 255])
end

func utf16(s: text) -> text
    var out: []long = []
    for c in s
        let cp = c.code
        out.add(long(cp bitand 255))
        out.add(long((cp >> 8) bitand 255))
    end
    return crypto.from_bytes(out)
end

func from_utf16(b: text) -> text
    var cs: []char = []
    var i = 0
    while i + 1 < b.len
        cs.add(char(int(b.byte(i)) + int(b.byte(i + 1)) * 256))
        i += 2
    end
    return text.from_chars(cs)
end

func want(w: Wire, n: int) -> bool
    while w.buf.len < n
        let more = match await w.conn.read(8192)
            ok(m) -> m
            err(e) -> ""
        end
        if more.len == 0
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

// One whole request message: its packet type and payload; kind -1 at the end of the input.
type Req = record
    kind: int
    payload: text
end

func read_req(w: Wire) -> Req
    var kind = -1
    var parts: []text = []
    while true
        if not want(w, 8)
            return Req { kind: -1, payload: "" }
        end
        let head = grab(w, 8)
        kind = int(head.byte(0))
        let len = int(head.byte(2)) * 256 + int(head.byte(3))
        if not want(w, len - 8)
            return Req { kind: -1, payload: "" }
        end
        parts.add(grab(w, len - 8))
        if int(head.byte(1)) bitand 1 != 0
            break
        end
    end
    return Req { kind: kind, payload: text.join(parts, "") }
end

// Sends a reply message split into packets of 4096 bytes.
func reply(w: Wire, payload: text)
    var at = 0
    var id = 1
    var all: []text = []
    while true
        var n = payload.len - at
        var status = 1
        if n > 4088
            n = 4088
            status = 0
        end
        all.add(crypto.from_bytes([4, long(status)]) + be16(long(n + 8)) + be16(0) + crypto.from_bytes([long(id bitand 255), 0]) + payload.__raw_slice(at, at + n))
        at += n
        id += 1
        if status == 1
            break
        end
    end
    let r = await w.conn.write(text.join(all, ""))
end

func done(count: long, status: int) -> text
    return b1(253) + le16(long(status)) + le16(0) + le32(count) + le32(0)
end

func collation() -> text
    return crypto.from_bytes([9, 4, 208, 0, 52])
end

func b_varchar(s: text) -> text
    return b1(long(s.len)) + utf16(s)
end

func int_col(name: text) -> text
    return b1(129) + le16(1) + le32(0) + le16(0) + b1(56) + b_varchar(name)
end

func answer(w: Wire, sql: text)
    if sql == "ints"
        reply(w, int_col("n") + b1(209) + le32(1) + b1(209) + le32(2) + b1(209) + le32(300) + done(3, 16))
    else if sql == "big"
        var parts: []text = [b1(129) + le16(1) + le32(0) + le16(0) + b1(167) + le16(200) + collation() + b_varchar("s")]
        let value = text.repeat("x", 200)
        for k in 0..5000
            parts.add(b1(209) + le16(200) + value)
        end
        parts.add(done(5000, 16))
        reply(w, text.join(parts, ""))
    else if sql == "boom"
        let msg = "Violation of PRIMARY KEY constraint"
        let body = le32(2627) + b1(1) + b1(14) + le16(long(msg.len)) + utf16(msg) + b_varchar("srv") + b_varchar("") + le32(1)
        reply(w, b1(170) + le16(long(body.len)) + body + done(0, 2))
    else if sql == "cut"
        reply(w, int_col("n") + b1(209) + crypto.from_bytes([1, 0]))
    else if sql == "weird"
        reply(w, b1(1) + b1(2) + done(0, 0))
    else if sql == "update"
        reply(w, done(3, 16))
    else
        reply(w, done(0, 0))
    end
end

func serve(conn: net.Conn)
    let w = Wire { conn: conn, buf: "" }
    let pre = read_req(w)
    if pre.kind != 18
        return
    end
    reply(w, crypto.from_bytes([1, 0, 6, 0, 1, 255, 2]))
    let login = read_req(w)
    if login.kind != 16
        return
    end
    let ack = crypto.from_bytes([1, 4, 0, 0, 116]) + b_varchar("fake") + crypto.from_bytes([1, 0, 0, 0])
    reply(w, b1(173) + le16(long(ack.len)) + ack + done(0, 0))
    while true
        let req = read_req(w)
        if req.kind == -1
            return
        end
        if req.kind == 1
            answer(w, from_utf16(req.payload.__raw_slice(22, req.payload.len)))
        else
            reply(w, done(0, 0))
        end
    end
end

func accept_loop(listener: net.Listener)
    while true
        match await listener.accept()
            ok(c) -> spawn serve(c)
            err(e) -> return
        end
    end
end

// Replays recorded answers: one per request message, whatever it asks.
let REPLAY: []text = @REPLAY@

func serve_replay(conn: net.Conn)
    let w = Wire { conn: conn, buf: "" }
    var k = 0
    while k < REPLAY.len
        let req = read_req(w)
        if req.kind == -1
            return
        end
        match crypto.unhex(REPLAY[k])
            ok(b) ->
                let r = await conn.write(b)
            err(e) -> return
        end
        k += 1
    end
end

func accept_replay(listener: net.Listener)
    while true
        match await listener.accept()
            ok(c) -> spawn serve_replay(c)
            err(e) -> return
        end
    end
end
"##;

fn replay_list() -> String {
    let data = std::fs::read_to_string("tests/data/sqlserver-exchange.txt").expect("run tests/docker/record_sqlserver.py");
    let items: Vec<String> = data.lines().map(|l| format!("\"{}\"", l)).collect();
    format!("[{}]", items.join(", "))
}

fn program(body: &str) -> String {
    let fake = FAKE.replace("@REPLAY@", "[]");
    format!(
        "package main\n\nuse db\nuse sqlserver\n{fake}\nfunc main() -> error\n    let listener = try await net.listen(0)\n    spawn accept_loop(listener)\n    let port = listener.port\n    let url = \"sqlserver://sa:s3cret@127.0.0.1:{{port}}/x?encrypt=off\"\n{body}\n    return\nend\n"
    )
}

#[test]
fn an_unsupported_encryption_mode_is_refused_before_connecting_and_the_password_stays_out() {
    let body = "    for m in [\"on\", \"true\", \"maybe\"]\n        match sqlserver.open(\"sqlserver://sa:s3cret@127.0.0.1:1/x?encrypt=\" + m)\n            ok(c) -> print(\"unexpected success\")\n            err(e) -> print(e.kind.name + \": \" + e.message)\n        end\n    end\n    for bad in [\"sqlserver://sa:s3cret@h:x/db\", \"postgres://sa:s3cret@h/db\", \"sqlserver://:s3cret@h/db\"]\n        match sqlserver.open(bad)\n            ok(c) -> print(\"unexpected success\")\n            err(e) -> print(e.message)\n        end\n    end\n";
    let out = run(&program(body));
    assert!(!out.contains("s3cret"), "{}", out);
    assert!(out.contains("std.Unsupported: encrypt=on (TLS inside the TDS pre-login) is not supported: use encrypt=strict"), "{}", out);
    assert!(out.contains("std.Unsupported: encrypt=true") && out.contains("std.InvalidInput: unsupported encrypt \"maybe\""), "{}", out);
    assert!(out.contains("bad port") && out.contains("not a SQL Server URL") && out.contains("no user"), "{}", out);
}

#[test]
fn rows_come_back_through_the_fake_login_and_a_big_result_spans_many_packets() {
    let body = "    let conn = try sqlserver.open(url)\n    let ints = try db.query(int, conn, \"ints\")\n    print(ints.len, ints[0] + ints[1] + ints[2])\n    let big = try db.query(text, conn, \"big\")\n    print(big.len, big[0].len, big[4999] == big[0])\n    let n = try conn.exec(\"update\")\n    print(n)\n";
    let src = program(body).replace("{port}", "{port}");
    assert_eq!(run(&src), "3 303\n5000 200 true\n3\n");
}

#[test]
fn a_server_error_maps_its_number_and_the_connection_stays_usable() {
    let body = "    let conn = try sqlserver.open(url)\n    match conn.exec(\"boom\")\n        ok(n) -> print(\"unexpected success\")\n        err(e) -> print(e.kind.name + \": \" + e.message)\n    end\n    let ints = try db.query(int, conn, \"ints\")\n    print(ints.len)\n";
    assert_eq!(run(&program(body)), "std.AlreadyExists: Violation of PRIMARY KEY constraint (2627, severity 14, state 1)\n3\n");
}

#[test]
fn a_cut_or_unknown_token_stream_is_an_error_not_a_crash() {
    let body = "    let conn = try sqlserver.open(url)\n    match db.query(int, conn, \"cut\")\n        ok(r) -> print(\"unexpected success\")\n        err(e) -> print(e.message)\n    end\n    match db.query(int, conn, \"ints\")\n        ok(r) -> print(\"unexpected success\")\n        err(e) -> print(e.kind.name)\n    end\n";
    assert_eq!(run(&program(body)), "the database server's answer is cut off or malformed\nstd.Unavailable\n");
    let body = "    let conn = try sqlserver.open(url)\n    match db.query(int, conn, \"weird\")\n        ok(r) -> print(\"unexpected success\")\n        err(e) -> print(e.message)\n    end\n";
    let out = run(&program(body));
    assert!(out.starts_with("the database server sent a token this driver does not know (0x01)"), "{}", out);
}

// ---------------------------------------------------------------------------
// The real servers of tests/docker
// ---------------------------------------------------------------------------

fn env_url(name: &str) -> Option<String> {
    match std::env::var(name) {
        // backslashes (`host\INSTANCE`) must be escaped to sit in a Zila Lang text literal
        Ok(u) if !u.is_empty() => Some(u.replace('\\', "\\\\")),
        _ => {
            eprintln!("skipped: set {} (see tests/docker/README.md) to run this test", name);
            None
        }
    }
}

fn real_program(url: &str, types: &str, body: &str) -> String {
    format!(
        "package main\n\nuse db\nuse sqlserver\nuse crypto\n\ntype Customer = record\n    id: int\n    name: text\n    email: text?\n    city: text?\nend\n{types}\nfunc main() -> error\n    let conn = try sqlserver.open(\"{url}\")\n    try conn.exec(\"create table #customers (id int primary key, name nvarchar(200) not null, email nvarchar(200), city nvarchar(200))\")\n    try conn.exec(\"insert into #customers values (@p1, @p2, @p3, @p4), (@p5, @p6, @p7, @p8), (@p9, @p10, @p11, @p12), (@p13, @p14, @p15, @p16)\", db.int(1), db.str(\"Ana Souza\"), db.str(\"ana@example.com\"), db.str(\"Recife\"), db.int(2), db.str(\"João D'Arc\"), db.null(), db.str(\"São Paulo\"), db.int(3), db.str(\"Zoë 日本\"), db.null(), db.null(), db.int(4), db.str(\"Владимир Петров\"), db.str(\"vp@example.com\"), db.str(\"Москва\"))\n{body}\n    return\nend\n"
    )
}

const ROW_TYPES: &str = "\ntype Row = record\n    b: bool\n    n: long\n    f: float\n    d: text\n    s: text\n    u: text\n    g: text\n    y: text\n    z: text?\n    day: text\n    ts: text\n    m: text\nend\n";

const TYPES_BODY: &str = "    let rows = try db.query(Customer, conn, \"select id, name, email, city from #customers where id < @p1 order by id\", db.int(5))\n    for r in rows\n        print(r.id, r.name, r.email ?? \"-\", r.city ?? \"-\")\n    end\n    let t = try db.one(Row, conn, \"select cast(1 as bit) as b, cast(42 as bigint) as n, cast(1.5 as float) as f, cast(12.50 as decimal(10,2)) as d, N'Zoë 日本' as s, cast('x' as varchar(10)) as u, cast('6F9619FF-8B86-D011-B42D-00C04FC964FF' as uniqueidentifier) as g, cast(0xDEAD as varbinary(10)) as y, cast(null as nvarchar(10)) as z, cast('2026-11-08' as date) as day, cast('2026-11-08 13:45:10.123' as datetime2(3)) as ts, cast(12.3456 as money) as m\")\n    print(t.b, t.n, t.f, t.d, t.s, t.u, t.g, crypto.hex(t.y), t.z == nil, t.day, t.ts, t.m)\n    let echoed = try db.one(text, conn, \"select @p1 + N'|' + cast(@p2 as nvarchar(20)) + N'|' + cast(@p3 as nvarchar(5)) + N'|' + isnull(@p4, N'<null>')\", db.str(\"it's Zoë\"), db.float(2.5), db.bool(true), db.null())\n    print(echoed)\n    match db.one(Customer, conn, \"select id, name, email, city from #customers where id = @p1\", db.int(999))\n        ok(r) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name)\n    end\n    match conn.exec(\"insert into #customers(id, name) values (@p1, @p2)\", db.int(1), db.str(\"dup\"))\n        ok(n) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name, text.contains(e.message, \"2627\"))\n    end\n";

const TYPES_EXPECTED: &str = "1 Ana Souza ana@example.com Recife\n2 João D'Arc - São Paulo\n3 Zoë 日本 - -\n4 Владимир Петров vp@example.com Москва\ntrue 42 1.5 12.50 Zoë 日本 x 6f9619ff-8b86-d011-b42d-00c04fc964ff dead true 2026-11-08 2026-11-08 13:45:10.123 12.3456\nit's Zoë|2.5|1|<null>\nstd.NotFound\nstd.AlreadyExists true\n";

fn typed_body() -> String {
    TYPES_BODY.to_string()
}

fn typed_program(url: &str) -> String {
    real_program(url, ROW_TYPES, &typed_body()).replace("use crypto\n", "use crypto\nuse text\n")
}

#[test]
fn strict_encryption_tds_8_reads_typed_rows_parameters_and_maps_unique_violations() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER_STRICT") else { return };
    let out = run(&typed_program(&u));
    assert_eq!(out, TYPES_EXPECTED.replace("true\n", "true\n").replace("std.AlreadyExists true", "std.AlreadyExists true"), "{}", out);
}

#[test]
fn unencrypted_development_mode_gives_the_same_results() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER") else { return };
    let out = run(&typed_program(&u));
    assert_eq!(out, TYPES_EXPECTED, "{}", out);
}

#[test]
fn a_wrong_login_is_unauthenticated() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER") else { return };
    // the password is the text between the first `:` after the user and the `@`
    let start = u.find("://").unwrap() + 3;
    let at = u.find('@').unwrap();
    let colon = u[start..at].find(':').map(|c| c + start).unwrap();
    let bad = format!("{}:nope{}", &u[..colon], &u[at..]);
    let src = format!("package main\n\nuse sqlserver\n\nfunc main()\n    match sqlserver.open(\"{bad}\")\n        ok(c) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name)\n    end\nend\n");
    assert_eq!(run(&src), "std.Unauthenticated\n");
}

#[test]
fn a_server_that_forces_encryption_is_refused_in_off_mode() {
    // needs the server of tests/docker that forces encryption (ZILA_TEST_SQLSERVER_STRICT)
    let Some(u) = env_url("ZILA_TEST_SQLSERVER_STRICT") else { return };
    let off = u.replace("encrypt=strict", "encrypt=off");
    let src = format!("package main\n\nuse sqlserver\n\nfunc main()\n    match sqlserver.open(\"{off}\")\n        ok(c) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name + \": \" + e.message)\n    end\nend\n");
    assert_eq!(run(&src), "std.Unsupported: the server requires encryption: use encrypt=strict\n");
}

#[test]
fn transactions_commit_and_roll_back_on_the_real_server() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER") else { return };
    let body = "    let t = try conn.begin()\n    try t.conn.exec(\"insert into #customers(id, name) values (@p1, @p2)\", db.int(9001), db.str(\"tx\"))\n    let inside = try db.one(int, t.conn, \"select count(*) from #customers where id = 9001\")\n    try t.rollback()\n    let after = try db.one(int, conn, \"select count(*) from #customers where id = 9001\")\n    print(inside, after)\n    let t2 = try conn.begin()\n    try t2.conn.exec(\"insert into #customers(id, name) values (@p1, @p2)\", db.int(9002), db.str(\"kept\"))\n    try t2.commit()\n    print(try db.one(int, conn, \"select count(*) from #customers where id = 9002\"))\n    try conn.exec(\"delete from #customers where id = 9002\")\n    let dropped = try conn.begin()\n    try dropped.conn.exec(\"insert into #customers(id, name) values (@p1, @p2)\", db.int(9003), db.str(\"gone\"))\n";
    let src = real_program(&u, "", body).replace("    let dropped = try conn.begin()", "    try forget(conn)\n    let dropped = try conn.begin()").replace(
        "func main()",
        "func forget(conn: db.Conn) -> error\n    let d = try conn.begin()\n    try d.conn.exec(\"insert into #customers(id, name) values (@p1, @p2)\", db.int(9004), db.str(\"gone\"))\n    return\nend\n\nfunc main()",
    );
    let src = src.replace("    let dropped = try conn.begin()\n    try dropped.conn.exec(\"insert into #customers(id, name) values (@p1, @p2)\", db.int(9003), db.str(\"gone\"))\n", "    print(try db.one(int, conn, \"select count(*) from #customers where id in (9003, 9004)\"))\n");
    assert_eq!(run(&src), "1 0\n1\n0\n");
}

#[test]
fn a_slow_statement_does_not_stall_other_tasks_on_the_real_server() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER") else { return };
    let body = "    let ticks = Ticks { n: 0 }\n    spawn tick(ticks)\n    let r = try db.one(int, conn, \"waitfor delay '00:00:00.600'; select 7\")\n    print(r, ticks.n >= 4)\n";
    let src = real_program(&u, "", body).replace(
        "func main()",
        "#[shared]\ntype Ticks = record\n    n: int\nend\n\nfunc tick(t: Ticks)\n    for k in 0..8\n        await sleep(100)\n        t.n += 1\n    end\nend\n\nfunc main()",
    );
    assert_eq!(run(&src), "7 true\n");
}

// ---------------------------------------------------------------------------
// CRUD on a mirror of the njai application's Users table (login ai / ai)
// ---------------------------------------------------------------------------

#[test]
fn crud_on_the_users_table() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER_AI") else { return };
    let types = "\ntype User = record\n    id: text\n    username: text\n    #[db(\"display_name\")] display: text?\n    email: text?\n    department: text?\n    role: text\n    access_status: text\n    created: text\nend\n";
    let select = "select cast(Id as nvarchar(36)) as id, Username as username, DisplayName as display_name, Email as email, Department as department, Role as role, AccessStatus as access_status, convert(nvarchar(30), CreatedAtUtc, 126) as created from dbo.Users";
    let body = format!(
        "    try conn.exec(\"delete from dbo.Users where Username like N'zila-crud-%'\")\n    // Create\n    let n1 = try conn.exec(\"insert into dbo.Users (Id, Username, DisplayName, Email, Department, AuthSource, Role, AccessStatus, CreatedAtUtc) values (newid(), @p1, @p2, @p3, @p4, N'Local', N'User', N'Active', sysutcdatetime())\", db.str(\"zila-crud-ana\"), db.str(\"Ana Souza\"), db.str(\"ana@example.com\"), db.null())\n    let n2 = try conn.exec(\"insert into dbo.Users (Id, Username, DisplayName, Email, Department, AuthSource, Role, AccessStatus, CreatedAtUtc) values (newid(), @p1, @p2, @p3, @p4, N'Local', N'Admin', N'Active', sysutcdatetime())\", db.str(\"zila-crud-joão\"), db.str(\"João D'Arc 日本\"), db.null(), db.str(\"Engenharia\"))\n    print(\"created\", n1, n2)\n    // Read\n    let ana = try db.one(User, conn, \"{select} where Username = @p1\", db.str(\"zila-crud-ana\"))\n    print(ana.username, ana.display ?? \"-\", ana.email ?? \"-\", ana.department ?? \"-\", ana.role, ana.access_status, ana.id.len, ana.created.len > 10)\n    let all = try db.query(User, conn, \"{select} where Username like N'zila-crud-%' order by Username\")\n    for u in all\n        print(\"user\", u.username, u.display ?? \"-\", u.department ?? \"-\", u.role)\n    end\n    // Update\n    let up = try conn.exec(\"update dbo.Users set DisplayName = @p1, Department = @p2, AccessStatus = @p3 where Username = @p4\", db.str(\"Ana S.\"), db.str(\"Vendas\"), db.str(\"Suspended\"), db.str(\"zila-crud-ana\"))\n    let after = try db.one(User, conn, \"{select} where Username = @p1\", db.str(\"zila-crud-ana\"))\n    print(\"updated\", up, after.display ?? \"-\", after.department ?? \"-\", after.access_status)\n    let none = try conn.exec(\"update dbo.Users set Role = @p1 where Username = @p2\", db.str(\"X\"), db.str(\"nobody\"))\n    print(\"updated nobody\", none)\n    // a duplicate username is refused by the unique index\n    match conn.exec(\"insert into dbo.Users (Id, Username, AuthSource, Role, AccessStatus, CreatedAtUtc) values (newid(), @p1, N'Local', N'User', N'Active', sysutcdatetime())\", db.str(\"zila-crud-ana\"))\n        ok(n) -> print(\"unexpected success\")\n        err(e) -> print(e.kind.name)\n    end\n    // Delete, with a rolled-back attempt first\n    let t = try conn.begin()\n    let gone = try t.conn.exec(\"delete from dbo.Users where Username = @p1\", db.str(\"zila-crud-ana\"))\n    try t.rollback()\n    print(\"still there\", try db.one(int, conn, \"select count(*) from dbo.Users where Username = N'zila-crud-ana'\"), gone)\n    let d1 = try conn.exec(\"delete from dbo.Users where Username = @p1\", db.str(\"zila-crud-ana\"))\n    let d2 = try conn.exec(\"delete from dbo.Users where Username = @p1\", db.str(\"zila-crud-joão\"))\n    print(\"deleted\", d1, d2)\n    match db.one(User, conn, \"{select} where Username = @p1\", db.str(\"zila-crud-ana\"))\n        ok(r) -> print(\"unexpected\")\n        err(e) -> print(e.kind.name)\n    end\n    print(\"left\", try db.one(int, conn, \"select count(*) from dbo.Users where Username like N'zila-crud-%'\"))\n",
        select = select
    );
    let src = real_program(&u, types, &body);
    let expected = "created 1 1\nzila-crud-ana Ana Souza ana@example.com - User Active 36 true\nuser zila-crud-ana Ana Souza - User\nuser zila-crud-joão João D'Arc 日本 Engenharia Admin\nupdated 1 Ana S. Vendas Suspended\nupdated nobody 0\nstd.AlreadyExists\nstill there 1 1\ndeleted 1 1\nstd.NotFound\nleft 0\n";
    assert_eq!(run(&src), expected);
}

#[test]
fn a_column_of_an_unsupported_type_is_an_error_naming_the_column_and_the_type() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER") else { return };
    let body = "    for q in [\"select cast(1 as sql_variant) as v\", \"select cast('x' as text) as t\", \"select 1 as ok, cast(1 as sql_variant) as payload\"]
        match db.query(text, conn, q)
            ok(r) -> print(\"unexpected success\")
            err(e) -> print(e.kind.name + \": \" + e.message)
        end
    end
    print(try db.one(int, conn, \"select 5\"))
";
    let out = run(&real_program(&u, "", body));
    assert_eq!(out, "std.Unsupported: column v: the type sql_variant is not supported by this driver
std.Unsupported: column t: the type text is not supported by this driver
std.Unsupported: column payload: the type sql_variant is not supported by this driver
5
");
}

#[test]
fn xml_and_single_byte_text_in_cyrillic_and_central_european_code_pages_are_converted() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER") else { return };
    let body = "    print(try db.one(text, conn, \"select cast('<a b=\\\"1\\\">Zoë</a>' as xml)\"))
    let t = try db.one(text, conn, \"select cast(N'Привет, мир' collate Cyrillic_General_CI_AS as varchar(40)) collate Cyrillic_General_CI_AS\")
    print(t)
    let pl = try db.one(text, conn, \"select cast(N'zażółć gęślą jaźń' collate Polish_CI_AS as varchar(40))\")
    print(pl)
    let west = try db.one(text, conn, \"select cast(N'ação café' collate Latin1_General_CI_AS as varchar(40))\")
    print(west)
    match db.one(text, conn, \"select cast(N'αβγ' collate Greek_CI_AS as varchar(10))\")
        ok(v) -> print(\"unexpected\", v)
        err(e) -> print(e.kind.name)
    end
";
    let out = run(&real_program(&u, "", body));
    assert_eq!(out, "<a b=\"1\">Zoë</a>\nПривет, мир\nzażółć gęślą jaźń\nação café\nstd.Unsupported\n");
}

#[test]
fn recorded_answers_of_the_real_server_decode_into_the_expected_values() {
    let client = std::fs::read_to_string("tests/data/sqlserver-client.br").unwrap();
    let rest = client
        .replace("package main

use db
use sqlserver
", "")
        .replace("func main() -> error", "func client_main(port: long) -> error")
        .replace("@PORT@", "{port}");
    let fake = FAKE.replace("@REPLAY@", &replay_list());
    let src = format!(
        "package main

use db
use sqlserver
{fake}
{rest}
func main() -> error
    let listener = try await net.listen(0)
    spawn accept_replay(listener)
    try client_main(listener.port)
    return
end
"
    );
    let out = run(&src);
    assert!(out.starts_with("1 Ana Souza
2 João D'Arc
3 Zoë 日本
true
std.AlreadyExists: Violation of PRIMARY KEY constraint"), "{}", out);
    assert!(out.contains("The duplicate key value is (1). (2627, severity 14, state 1)"), "{}", out);
}

#[test]
fn a_statement_that_times_out_is_cancelled_and_the_connection_stays_usable() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER") else { return };
    let url = format!("{u}{}timeout=400", if u.contains('?') { "&" } else { "?" });
    let body = "    let t0 = task.now()
    match db.one(int, conn, \"waitfor delay '00:00:05'; select 1\")
        ok(n) -> print(\"unexpected success\")
        err(e) -> print(e.kind.name + \": \" + e.message)
    end
    print(task.now() - t0 < 3000)
    print(try db.one(int, conn, \"select 5\"))
    let n = try db.one(int, conn, \"select count(*) from #customers\")
    print(n)
";
    let src = real_program(&url, "", body).replace("use crypto
", "use crypto
use task
");
    assert_eq!(run(&src), "std.Timeout: the statement timed out after 400 ms and was cancelled
true
5
4
");
}

#[test]
fn instance_and_pipe_urls_are_checked_before_anything_is_sent() {
    let body = r#"    for u in ["sqlserver://sa:s3cret@.\\SQL2025/x?encrypt=strict", "sqlserver://sa:s3cret@localhost/x?transport=carrier", "sqlserver://sa:s3cret@\\SQL2025/x", "sqlserver://sa:s3cret@.\\NOSUCHINSTANCE/x"]
        match sqlserver.open(u)
            ok(c) -> print("unexpected success")
            err(e) -> print(e.kind.name + ": " + e.message)
        end
    end
"#;
    let out = run(&program(body));
    assert!(!out.contains("s3cret"), "{}", out);
    assert!(out.contains("std.InvalidInput: a named pipe cannot carry TLS"), "{}", out);
    assert!(out.contains("std.InvalidInput: unsupported transport \"carrier\""), "{}", out);
    assert!(out.contains("std.InvalidInput: bad host or instance in the SQL Server URL"), "{}", out);
    assert!(out.contains("std.Unavailable: cannot open the named pipe"), "{}", out);
}

// ---------------------------------------------------------------------------
// `sql tsql` blocks on a real server
// ---------------------------------------------------------------------------

const SQL_BLOCKS_BODY: &str = r#"    let evil = "x'); drop table #customers; --"
    let ins = sql tsql
        insert into #customers (id, name, email, city) values ({7}, {evil}, {nil}, {"Lisboa"});
    end
    print(try conn.exec_sql(ins))
    let q = sql tsql
        select id, name, email, city from #customers where id = {7} and name = {evil}
    end
    let c = try db.one_sql(Customer, conn, q)
    print(c.id, c.name, c.email ?? "-", c.city ?? "-")
    let b = sql tsql
        declare @total int = 0;
        select @total = count(*) from #customers where id < {5};
        select [total] = @total, [a{{b}}] = N'it''s {not a hole}'
    end
    let total = try db.one_sql(Batch, conn, b)
    print(total.total)
    let tx = try conn.begin()
    let del = sql tsql
        delete from #customers where id = {7}
    end
    print(try tx.conn.exec_sql(del))
    try tx.rollback()
    let cnt = sql tsql
        select count(*) from #customers where id = {7}
    end
    print(try db.one_sql(int, conn, cnt))
    let wrong = sql postgres
        select 1
    end
    match conn.exec_sql(wrong)
        ok(n) -> print("unexpected")
        err(e) -> print(e.kind.name, e.message)
    end
"#;

#[test]
fn the_real_server_runs_sql_blocks_with_parameters_variables_and_rollback() {
    let Some(u) = env_url("ZILA_TEST_SQLSERVER") else { return };
    let src = real_program(&u, "\ntype Batch = record\n    total: int\nend\n", SQL_BLOCKS_BODY).replace("use crypto", "use crypto\nuse postgres");
    assert_eq!(
        run(&src),
        "1\n7 x'); drop table #customers; -- - Lisboa\n4\n1\n1\nstd.InvalidInput this statement is written for postgres but the connection speaks tsql\n"
    );
}
