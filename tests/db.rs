//! The `db` library (spec `db-library`): parameters, typed rows decoded by comptime reflection,
//! transactions, errors and URLs, against an in-memory fake driver written in Brasa. The real
//! drivers (`postgres`, `sqlserver`) have their own sections below and, when the Docker
//! environment of `tests/docker` is running, tests against real servers.

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("brasa-db-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        brasa::comptime::set_cache_dir(&dir);
    });
}

fn build(src: &str) -> Result<std::path::PathBuf, String> {
    setup();
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-db-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap()).map(|_| exe)
}

fn run(src: &str) -> String {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "")
}

fn compile_error(src: &str) -> String {
    match build(src) {
        Ok(exe) => {
            let _ = std::fs::remove_file(exe);
            panic!("expected a compile error")
        }
        Err(e) => e,
    }
}

/// A fake driver: `select customers` returns two customers, `select scalar` one column,
/// `select empty` nothing, `select wide` a column no field wants, `update ...` three rows
/// affected, `fail ...` an AlreadyExists error. Everything it is asked is appended to the log.
const FAKE: &str = r#"
use errors
use text

#[shared]
type Log = record
    lines: []text
end

func arg_text(a: db.Arg) -> text
    match a
        Int(n) -> return "int:{n}"
        Float(x) -> return "float:{x}"
        Bool(b) -> return "bool:{b}"
        Text(s) -> return "text:{s}"
        Bytes(s) -> return "bytes:{s.len}"
        Vector(v) -> return "vector:{v.len}"
        Null -> return "null"
    end
    return ""
end

func fake(log: Log) -> db.Conn
    return db.connection(
        func(sql, args)
            var shown = sql
            for a in args
                shown = shown + " " + arg_text(a)
            end
            log.lines.add(shown)
            if text.starts_with(sql, "fail")
                return error(errors.AlreadyExists, "duplicate key (23505)")
            end
            if text.starts_with(sql, "select customers")
                var rs = db.rows(["ID", "full_name", "extra", "email", "score"], ["int", "text", "text", "text", "int"])
                rs.add("1", false)
                rs.add("Ana", false)
                rs.add("x", false)
                rs.add("", true)
                rs.add("300", false)
                rs.add("2", false)
                rs.add("Bia", false)
                rs.add("y", false)
                rs.add("b@x", false)
                rs.add("5", false)
                return rs
            end
            if text.starts_with(sql, "select scalar")
                var rs = db.rows(["n"], ["int"])
                rs.add("41", false)
                rs.add("42", false)
                return rs
            end
            if text.starts_with(sql, "select nullname")
                var rs = db.rows(["id", "full_name"], ["int", "text"])
                rs.add("1", false)
                rs.add("", true)
                return rs
            end
            if text.starts_with(sql, "select noname")
                var rs = db.rows(["id"], ["int"])
                rs.add("1", false)
                return rs
            end
            if text.starts_with(sql, "select empty")
                return db.rows(["id"], ["int"])
            end
            var rs = db.rows([], [])
            if text.starts_with(sql, "update")
                rs.affected = 3
            end
            return rs
        end,
        func()
            log.lines.add("BEGIN")
            return
        end,
        func()
            log.lines.add("COMMIT")
            return
        end,
        func()
            log.lines.add("ROLLBACK")
            return
        end,
        func()
            log.lines.add("CLOSE")
        end
    )
end

type Customer = record
    id: int
    #[db("full_name")] name: text
    email: text?
    #[db(skip)] note: text
end

type Small = record
    id: i8
end

func show(log: Log)
    for l in log.lines
        print("log: " + l)
    end
end
"#;

fn program(body: &str) -> String {
    format!("package main\n\nuse db\n{}\nfunc main()\n    let log = Log {{ lines: [] }}\n    let conn = fake(log)\n{}\nend\n", FAKE, body)
}

#[test]
fn exec_returns_rows_affected_and_arguments_travel_as_parameters() {
    let body = r#"
    match conn.exec("update t set a = $1 and b = $2 and c = $3 and d = $4 and e = $5 and f = $6", db.int(7), db.float(1.5), db.bool(true), db.str("it's"), db.null(), db.vector([1.0, 2.0]))
        ok(n) -> print("affected", n)
        err(e) -> print(e.message)
    end
    match conn.exec("insert x", db.bytes("ab"))
        ok(n) -> print("affected", n)
        err(e) -> print(e.message)
    end
    show(log)
"#;
    let expected = "affected 3\naffected 0\nlog: update t set a = $1 and b = $2 and c = $3 and d = $4 and e = $5 and f = $6 int:7 float:1.5 bool:true text:it's null vector:2\nlog: insert x bytes:2\n";
    assert_eq!(run(&program(body)), expected);
}

#[test]
fn a_value_that_is_not_an_arg_is_a_compile_error_naming_the_type() {
    let body = "    let n = conn.exec(\"update t\", 5)\n";
    let e = compile_error(&program(body));
    assert!(e.contains("Arg") && e.contains("int"), "{}", e);
}

#[test]
fn rows_become_records_by_column_name_with_rename_skip_and_optionals() {
    let body = r#"
    match db.query(Customer, conn, "select customers where id > $1", db.int(0))
        ok(rows) ->
            for r in rows
                print(r.id, r.name, r.email ?? "none", r.note == "")
            end
        err(e) -> print("error: " + e.message)
    end
    show(log)
"#;
    let expected = "1 Ana none true\n2 Bia b@x true\nlog: select customers where id > $1 int:0\n";
    assert_eq!(run(&program(body)), expected);
}

#[test]
fn one_expects_exactly_one_row_and_scalars_read_a_single_column() {
    let body = r#"
    match db.one(Customer, conn, "select customers")
        ok(r) -> print("one", r.id)
        err(e) -> print(e.kind.name + ": " + e.message)
    end
    match db.one(Customer, conn, "select empty")
        ok(r) -> print("one", r.id)
        err(e) -> print(e.kind.name + ": " + e.message)
    end
    match db.query(int, conn, "select scalar")
        ok(rows) -> print(rows.len, rows[0] + rows[1])
        err(e) -> print(e.message)
    end
    match db.query(int, conn, "select customers")
        ok(rows) -> print("accepted")
        err(e) -> print(e.message)
    end
"#;
    let expected = "std.InvalidInput: the query returned 2 rows where one was expected\nstd.NotFound: the query returned no row\n2 83\nexpected a result with one column, got 5\n";
    assert_eq!(run(&program(body)), expected);
}

#[test]
fn errors_name_the_field_the_column_and_the_row() {
    let body = r#"
    match db.query(Customer, conn, "select nullname")
        ok(rows) -> print("accepted")
        err(e) -> print(e.message)
    end
    match db.query(Customer, conn, "select noname")
        ok(rows) -> print("accepted")
        err(e) -> print(e.message)
    end
    match db.query(Small, conn, "select customers")
        ok(rows) -> print("accepted")
        err(e) -> print(e.message)
    end
    match conn.exec("fail now")
        ok(n) -> print("accepted")
        err(e) -> print(e.kind.name + ": " + e.message)
    end
"#;
    // `Small` matches the `ID` column ignoring case, so it reads both rows
    let expected = "row 1, field name (column full_name): NULL in a field that is not optional
no column \"full_name\" for field name
accepted
std.AlreadyExists: duplicate key (23505)
";
    assert_eq!(run(&program(body)), expected);
}

#[test]
fn an_integer_out_of_range_is_an_error_naming_the_field_and_row() {
    let body = r#"
    match db.query(Small, conn, "select customers")
        ok(rows) -> print(rows.len)
        err(e) -> print(e.message)
    end
    match db.query(Big, conn, "select customers")
        ok(rows) -> print("accepted")
        err(e) -> print(e.message)
    end
"#;
    let src = program(body).replace("type Small = record", "type Big = record\n    #[db(\"score\")] id: i8\nend\n\ntype Small = record");
    assert_eq!(run(&src), "2\nrow 1, field id (column score): 300 does not fit in i8\n");
}

#[test]
fn bad_db_attributes_and_duplicate_columns_are_compile_errors() {
    let src = program("    print(1)\n").replace("type Small = record", "type Twice = record\n    a: int\n    #[db(\"a\")] b: int\nend\n\nfunc use_twice(c: db.Conn)\n    let r = db.query(Twice, c, \"x\")\nend\n\ntype Small = record");
    let e = compile_error(&src);
    assert!(e.contains("both read the column"), "{}", e);
    let src = program("    print(1)\n").replace("type Small = record", "type Bad = record\n    #[db(5)] a: int\nend\n\nfunc use_bad(c: db.Conn)\n    let r = db.query(Bad, c, \"x\")\nend\n\ntype Small = record");
    let e = compile_error(&src);
    assert!(e.contains("db attribute"), "{}", e);
}

#[test]
fn transactions_commit_roll_back_and_roll_back_when_dropped() {
    let body = r#"
    let tx = try conn.begin()
    match conn.exec("update outside")
        ok(n) -> print("accepted")
        err(e) -> print(e.message)
    end
    match conn.begin()
        ok(t) -> print("accepted")
        err(e) -> print(e.message)
    end
    match tx.conn.begin()
        ok(t) -> print("accepted")
        err(e) -> print(e.message)
    end
    match tx.conn.exec("update inside")
        ok(n) -> print("inside", n)
        err(e) -> print(e.message)
    end
    match tx.commit()
        ok -> print("committed")
        err(e) -> print(e.message)
    end
    match tx.commit()
        ok -> print("accepted")
        err(e) -> print(e.message)
    end
    match tx.conn.exec("update stale")
        ok(n) -> print("accepted")
        err(e) -> print(e.message)
    end
    match conn.exec("update after")
        ok(n) -> print("after", n)
        err(e) -> print(e.message)
    end
    let t2 = try conn.begin()
    match t2.rollback()
        ok -> print("rolled back")
        err(e) -> print(e.message)
    end
    try drop_tx(conn)
    match conn.exec("update next")
        ok(n) -> print("next", n)
        err(e) -> print(e.message)
    end
    show(log)
    return
"#;
    let src = program(body)
        .replace("func main()", "func main() -> error")
        .replace(
            "func show(log: Log)",
            "func drop_tx(conn: db.Conn) -> error\n    let t = try conn.begin()\n    let r = t.conn.exec(\"update dropped\")\n    return\nend\n\nfunc show(log: Log)",
        );
    let expected = "\
a transaction is open on this connection: use its `conn` or end it first
a transaction is open on this connection: use its `conn` or end it first
a transaction is already open on this connection
inside 3
committed
this transaction has ended
this transaction has ended
after 3
rolled back
next 3
log: BEGIN
log: update inside
log: COMMIT
log: update after
log: BEGIN
log: ROLLBACK
log: BEGIN
log: update dropped
log: ROLLBACK
log: update next
";
    assert_eq!(run(&src), expected);
}

#[test]
fn a_closed_connection_reports_closed_and_close_is_idempotent() {
    let body = r#"
    conn.close()
    conn.close()
    match conn.exec("update x")
        ok(n) -> print("accepted")
        err(e) -> print(e.kind.name + ": " + e.message)
    end
    show(log)
"#;
    assert_eq!(run(&program(body)), "std.Closed: the database connection is closed\nlog: CLOSE\n");
}

#[test]
fn urls_are_taken_apart_and_the_password_never_appears_in_messages() {
    let body = r#"
    match db.parse_url("postgres://app:p%40ss%3Aw@db.example.com:5433/shop%20db?sslmode=require&x=a%2Fb&flag")
        ok(u) ->
            print(u.scheme, u.user, u.password, u.host, u.port, u.database)
            print(u.params["sslmode"], u.params["x"], u.params["flag"] == "")
        err(e) -> print(e.message)
    end
    match db.parse_url("sqlserver://sa@localhost")
        ok(u) -> print(u.user, u.password == "", u.host, u.port, u.database == "")
        err(e) -> print(e.message)
    end
    for bad in ["postgres://app:s3cret@localhost:99999/shop", "postgres://app:s3cret@/shop", "postgres://app:s3cret@h:x/shop", "nothing", "postgres://u:%zz@h/db"]
        match db.parse_url(bad)
            ok(u) -> print("accepted")
            err(e) -> print(e.message)
        end
    end
    print(db.redact("postgres://app:s3cret@localhost/shop"))
    print(db.redact("postgres://app@localhost/shop"))
    print(db.redact("sqlserver://sa:a:b@c@h:1/x"))
"#;
    let expected = "\
postgres app p@ss:w db.example.com 5433 shop db
require a/b true
sa true localhost 0 true
bad port in the database URL postgres://app:***@localhost:99999/shop
the database URL has no host: postgres://app:***@/shop
bad port in the database URL postgres://app:***@h:x/shop
not a database URL: nothing
bad percent escape in the URL
postgres://app:***@localhost/shop
postgres://app@localhost/shop
sqlserver://sa:***@h:1/x
";
    let out = run(&program(body));
    assert!(!out.contains("s3cret"), "{}", out);
    assert_eq!(out, expected);
}

// ---------------------------------------------------------------------------
// The examples
// ---------------------------------------------------------------------------

#[test]
fn the_database_examples_build_and_run_against_the_docker_servers_when_they_are_up() {
    for name in ["examples/db.br", "examples/users_crud.br"] {
        let src = std::fs::read_to_string(name).unwrap();
        let exe = build(&src).unwrap_or_else(|e| panic!("{}: {}", name, e));
        // db.br needs both servers, users_crud.br the SQL Server one with the njai database
        let wanted: &[&str] = if name.ends_with("db.br") { &["BRASA_TEST_POSTGRES", "BRASA_TEST_SQLSERVER_STRICT"] } else { &["BRASA_TEST_SQLSERVER_AI"] };
        if wanted.iter().any(|v| std::env::var(v).map_or(true, |x| x.is_empty())) {
            eprintln!("skipped running {}: set {} (see tests/docker/README.md)", name, wanted.join(" and "));
            let _ = std::fs::remove_file(exe);
            continue;
        }
        let out = Command::new(&exe).output().unwrap();
        let _ = std::fs::remove_file(&exe);
        let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
        if name.ends_with("db.br") {
            let rows = "1 Ana Souza ana@example.com Recife\n2 João D'Arc - São Paulo\n3 Zoë 日本 - -\n4 Владимир Петров vp@example.com Москва\ninside the transaction 1 after the rollback 0\n";
            assert_eq!(text, format!("PostgreSQL\n{rows}SQL Server\n{rows}"), "{}", String::from_utf8_lossy(&out.stderr));
        } else {
            assert!(text.contains("duplicate refused: std.AlreadyExists") && text.ends_with("deleted 2 left 0\n"), "{}\n{}", text, String::from_utf8_lossy(&out.stderr));
        }
    }
}
