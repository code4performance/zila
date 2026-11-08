//! `sql <dialect>` blocks (spec `sql-blocks`): the value a block makes, holes as bound parameters,
//! the checks done at compile time, and running a `db.Sql` through a connection. The connections
//! here are fakes written in Brasa; the real servers are in `tests/postgres.rs` and
//! `tests/sqlserver.rs`.

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("brasa-sql-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        brasa::comptime::set_cache_dir(&dir);
    });
}

fn build(src: &str) -> Result<std::path::PathBuf, String> {
    setup();
    let file = parse_source(src).map_err(|d| format!("parse error: {}", d.msg))?;
    let exe = std::env::temp_dir().join(format!("brasa-sql-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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

const HEAD: &str = "package main\n\nuse db\nuse postgres\nuse sqlserver\n\n";

/// Shows an argument.
const SHOW: &str = r#"
func show(a: db.Arg) -> text
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

func dump(q: db.Sql)
    print("[{q.dialect}] {q.text}")
    for a in q.args
        print("  {show(a)}")
    end
end
"#;

fn program(body: &str) -> String {
    format!("{}{}\nfunc main()\n{}\nend\n", HEAD, SHOW, body)
}

#[test]
fn a_block_is_a_sql_value_with_the_placeholders_of_its_dialect() {
    let out = run(&program(
        "    let limit = 5\n    let city = \"Rio\"\n    let a = sql postgres\n        select id, name from customers\n        where id < {limit} and city = {city}\n    end\n    dump(a)\n    let b = sql tsql\n        select id, name from customers\n        where id < {limit} and city = {city}\n    end\n    dump(b)\n",
    ));
    assert_eq!(
        out,
        "[postgres] select id, name from customers\nwhere id < $1 and city = $2\n  int:5\n  text:Rio\n[tsql] select id, name from customers\nwhere id < @p1 and city = @p2\n  int:5\n  text:Rio\n"
    );
}

#[test]
fn a_block_without_holes_is_a_constant_statement() {
    let src = program("    let q = sql postgres\n        select 1; -- the answer\n    end\n    dump(q)\n");
    let out = run(&src);
    assert_eq!(out, "[postgres] select 1; -- the answer\n");
    // no argument list is built: the constant constructor is used, not the one with holes
    let asm = brasa::codegen_x64::generate_asm(&parse_source(&src).unwrap()).unwrap();
    assert!(asm.contains("B_db_sql_const proc") && !asm.contains("B_db_sql proc"));
}

#[test]
fn holes_are_converted_by_the_type_of_the_value() {
    let out = run(&program(
        "    let n: int = 7\n    let big: long = 5000000000\n    let x: float = 2.5\n    let y: f32 = 1.5\n    let t = true\n    let name = \"Ana\"\n    let none: text? = nil\n    let v: []float = [1.0, 2.0]\n    let arg = db.int(9)\n    let q = sql postgres\n        select {n}, {big}, {x}, {y}, {t}, {name}, {none}, {v}, {arg}, {nil}\n    end\n    dump(q)\n",
    ));
    assert_eq!(
        out,
        "[postgres] select $1, $2, $3, $4, $5, $6, $7, $8, $9, $10\n  int:7\n  int:5000000000\n  float:2.5\n  float:1.5\n  bool:true\n  text:Ana\n  null\n  vector:2\n  int:9\n  null\n"
    );
}

#[test]
fn a_hole_may_be_any_expression_and_runs_in_order() {
    let out = run(&program(
        "    var k = 0\n    let q = sql tsql\n        select {k + 1}, {\"a{k}\" + \"b\"}\n    end\n    dump(q)\n",
    ));
    assert_eq!(out, "[tsql] select @p1, @p2\n  int:1\n  text:a0b\n");
}

#[test]
fn injection_text_is_data_and_braces_in_strings_are_text() {
    let out = run(&program(
        "    let evil = \"x'); drop table customers; --\"\n    let q = sql postgres\n        select 'a{b}c', {{done}} from t where name = {evil}\n    end\n    dump(q)\n",
    ));
    assert_eq!(out, "[postgres] select 'a{b}c', {done} from t where name = $1\n  text:x'); drop table customers; --\n");
}

#[test]
fn dollar_quoting_and_bracket_identifiers_follow_the_dialect() {
    let out = run(&program(
        "    let n = 1\n    let a = sql postgres\n        select $$it's {not a hole}$$, {n}::int, a[1]\n    end\n    dump(a)\n    let b = sql tsql\n        select [order], {n} from [dbo].[t]\n    end\n    dump(b)\n",
    ));
    assert_eq!(out, "[postgres] select $$it's {not a hole}$$, $1::int, a[1]\n  int:1\n[tsql] select [order], @p1 from [dbo].[t]\n  int:1\n");
}

#[test]
fn several_statements_are_allowed_in_tsql_and_a_trailing_semicolon_is_removed() {
    let out = run(&program(
        "    let id = 3\n    let q = sql tsql\n        declare @total int = 0; select @total = count(*) from t where id < {id};\n    end\n    dump(q)\n",
    ));
    assert_eq!(out, "[tsql] declare @total int = 0; select @total = count(*) from t where id < @p1\n  int:3\n");
}

#[test]
fn sql_is_still_an_ordinary_identifier() {
    let out = run(&program("    let sql = \"x\"\n    print(sql)\n    print(db.sql_const(\"d\", \"t\").text)\n"));
    assert_eq!(out, "x\nt\n");
}

#[test]
fn mistakes_are_reported_at_compile_time_with_their_line() {
    let e = compile_error(&program("    let q = sql postgres\n        select 1\n        where name = 'abc\n    end\n"));
    assert!(e.contains("string that begins here is not closed") && e.contains(".br:"), "{}", e);
    let e = compile_error(&program("    let q = sql postgres\n        select 1; select 2\n    end\n"));
    assert!(e.contains("one statement per block"), "{}", e);
    let e = compile_error(&program("    let q = sql postgres\n        where id = $1\n    end\n"));
    assert!(e.contains("{expr}"), "{}", e);
    let e = compile_error(&program("    let q = sql tsql\n        select 1\n        GO\n    end\n"));
    assert!(e.contains("not T-SQL"), "{}", e);
    let e = compile_error(&program("    let q = sql postgres\n        select (1\n    end\n"));
    assert!(e.contains("never closed"), "{}", e);
    let e = compile_error(&program("    let q = sql postgres\n        select {main}\n    end\n"));
    assert!(e.contains("cannot be a parameter"), "{}", e);
    let e = compile_error(&program("    let q = sql postgres\n        select {1 +}\n    end\n"));
    assert!(e.contains("in the hole"), "{}", e);
}

#[test]
fn a_dialect_must_be_declared_by_an_imported_package() {
    let e = compile_error("package main\n\nuse db\n\nfunc main()\n    let q = sql tsql\n        select 1\n    end\nend\n");
    assert!(e.contains("'tsql' is provided by the package 'sqlserver'") && e.contains("use sqlserver"), "{}", e);
    let e = compile_error(&program("    let q = sql oracle\n        select 1\n    end\n"));
    assert!(e.contains("no imported package declares the sql dialect 'oracle'"), "{}", e);
    let e = compile_error("package main\n\nuse postgres\n\nfunc main()\n    let q = sql postgres\n        select 1\n    end\nend\n");
    assert!(e.contains("needs `use db`"), "{}", e);
}

#[test]
fn an_unterminated_block_names_where_it_began() {
    let e = compile_error("package main\n\nuse db\nuse postgres\n\nfunc main()\n    let q = sql postgres\n        select 1\n");
    assert!(e.contains("`sql` block that begins here is not closed"), "{}", e);
}

/// A fake connection that logs what reaches it and reads rows from two statements.
const FAKE: &str = r#"
#[shared]
type Log = record
    lines: []text
end

func fake(log: Log, dialect: text) -> db.Conn
    return db.connection(
        func(sql, args)
            var shown = sql
            for a in args
                shown = shown + " " + show(a)
            end
            log.lines.add(shown)
            if text.starts_with(sql, "select customers")
                var rs = db.rows(["id", "name"], ["int", "text"])
                rs.add("1", false)
                rs.add("Ana", false)
                rs.add("2", false)
                rs.add("Bia", false)
                return rs
            end
            var rs = db.rows([], [])
            rs.affected = 3
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
        func() end,
        dialect
    )
end

type Customer = record
    id: int
    name: text
end
"#;

fn with_fake(body: &str) -> String {
    format!("{}use errors\nuse text\n\n{}{}\nfunc main() -> error\n    let log = Log {{ lines: [] }}\n{}\n    return\nend\n", HEAD, SHOW, FAKE, body)
}

#[test]
fn a_statement_runs_through_a_connection_with_its_arguments() {
    let out = run(&with_fake(
        "    let conn = fake(log, \"postgres\")\n    let q = sql postgres\n        update customers set name = {\"Zed\"} where id = {4}\n    end\n    print(try conn.exec_sql(q))\n    for l in log.lines\n        print(l)\n    end\n",
    ));
    assert_eq!(out, "3\nupdate customers set name = $1 where id = $2 text:Zed int:4\n");
}

#[test]
fn typed_rows_come_from_a_block() {
    let out = run(&with_fake(
        "    let conn = fake(log, \"tsql\")\n    let q = sql tsql\n        select customers where id > {0}\n    end\n    let rows = try db.query_sql(Customer, conn, q)\n    for c in rows\n        print(\"{c.id} {c.name}\")\n    end\n    let one = try db.one_sql(Customer, conn, q)\n    print(one.name)\n",
    ));
    assert!(out.starts_with("1 Ana\n2 Bia\n"), "{}", out);
    assert!(out.contains("InvalidInput") || out.contains("2 rows"), "{}", out);
}

#[test]
fn a_statement_for_another_dialect_is_never_sent() {
    let out = run(&with_fake(
        "    let conn = fake(log, \"postgres\")\n    let q = sql tsql\n        select 1\n    end\n    match conn.exec_sql(q)\n        ok(n) -> print(\"ran {n}\")\n        err(e) -> print(e.message)\n    end\n    print(\"sent {log.lines.len}\")\n    let free = fake(log, \"\")\n    match free.exec_sql(q)\n        ok(n) -> print(\"ran {n}\")\n        err(e) -> print(e.message)\n    end\n    print(\"sent {log.lines.len}\")\n",
    ));
    assert_eq!(out, "this statement is written for tsql but the connection speaks postgres\nsent 0\nran 3\nsent 1\n");
}

#[test]
fn a_block_runs_inside_a_transaction() {
    let out = run(&with_fake(
        "    let conn = fake(log, \"postgres\")\n    let tx = try conn.begin()\n    let q = sql postgres\n        insert into t values ({1})\n    end\n    try tx.conn.exec_sql(q)\n    try tx.rollback()\n    for l in log.lines\n        print(l)\n    end\n",
    ));
    assert_eq!(out, "BEGIN\ninsert into t values ($1) int:1\nROLLBACK\n");
}

#[test]
fn a_package_can_declare_a_new_dialect_without_any_change_to_the_compiler() {
    let dir = std::env::temp_dir().join(format!("brasa-sql-dialect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lite")).unwrap();
    std::fs::write(dir.join("brasa.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(
        dir.join("lite").join("lite.br"),
        "package lite\n\n#[sql_dialect(\"sqlite\")]\n#[sql_placeholder(\"?\")]\n#[sql_string(\"'\")]\n#[sql_ident(\"[\", \"]\")]\n#[sql_ident(\"`\")]\n#[sql_line_comment(\"--\")]\npub type Dialect = record\nend\n",
    )
    .unwrap();
    let main = "package main\n\nuse db\nuse lite\n\nfunc main()\n    let id = 7\n    let q = sql sqlite\n        select * from [t] where a = {id} and `b` = {id + 1} -- {not a hole}\n    end\n    print(\"{q.dialect}|{q.text}|{q.args.len}\")\nend\n";
    std::fs::write(dir.join("main.br"), main).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_brasa-parse")).arg("main.br").arg("--run").current_dir(&dir).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "");
    // a hand-written `?` is refused by the profile
    std::fs::write(dir.join("main.br"), main.replace("{id + 1}", "?")).unwrap();
    let bad = Command::new(env!("CARGO_BIN_EXE_brasa-parse")).arg("main.br").arg("--asm").current_dir(&dir).output().unwrap();
    let bad_text = String::from_utf8_lossy(&bad.stderr).to_string();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(text, "sqlite|select * from [t] where a = ? and `b` = ? -- {not a hole}|2\n");
    assert!(!bad.status.success() && bad_text.contains("{expr}"), "{}", bad_text);
}
