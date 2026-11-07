//! Compile-time reflection: `comptime func`, `#[derive]`, `#[generated]` functions and block forms,
//! the comptime subset, limits, the cache and diagnostics (spec `comptime-reflection`). Programs are
//! compiled to native executables and run (skipped when the build cannot run them).

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

/// A cache directory of this test process, so that runs of the test suite do not see each other.
fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("brasa-comptime-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        brasa::comptime::set_cache_dir(&dir);
    });
}

fn build(src: &str) -> Result<std::path::PathBuf, String> {
    setup();
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-comptime-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap()).map(|_| exe)
}

/// Builds and runs a program; returns its output.
fn run(src: &str) -> String {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "")
}

/// The compile error of a program that must not compile.
fn compile_error(src: &str) -> String {
    match build(src) {
        Ok(exe) => {
            let _ = std::fs::remove_file(exe);
            panic!("expected a compile error")
        }
        Err(e) => e,
    }
}

/// How many times the generator `name` was run (not served from the cache) in this process.
fn launches(name: &str) -> usize {
    let prefix = format!("{}(", name);
    brasa::comptime::launch_log().iter().filter(|l| l.starts_with(&prefix)).count()
}

const DESCRIBE: &str = r#"
#[derive]
comptime func describe(t: Type) -> text
    var inner = t.name()
    for f in t.fields()
        inner = inner + " {f.name}=\{v.{f.name}\}"
    end
    return "func (v: {t.name()}) describe() -> text\n    return \"{inner}\"\nend\n"
end
"#;

#[test]
fn a_derive_generates_a_method() {
    let src = format!(
        r#"package main
use reflect

#[derive(describe)]
type Point = record
    x: int
    y: int
end
{}
func main()
    let p = Point {{ x: 1, y: 2 }}
    print(p.describe())
end
"#,
        DESCRIBE
    );
    assert_eq!(run(&src).trim(), "Point x=1 y=2");
}

fn line_of(src: &str, needle: &str) -> usize {
    src.lines().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("no line with {}", needle)) + 1
}

// ---- reflection data ----

#[test]
fn reflection_data_round_trips_through_the_wire_format() {
    use brasa::comptime::reflect::{parse_wire, serialize_call, ModFile, Reflector, Universe, WireCall};
    let src = r#"package main

type Address = record
    street: text
    zip: int
end

type Shape = enum
    #[label("round")]
    Circle(r: float)
    Dot
    Pair(int, text)
end

#[derive(json)]
type User = record
    id: int
    #[json("full_name")]
    name: text
    homes: []Address
    nickname: text?
    scores: Map<text, int>
    shape: Shape
end
"#;
    let file = parse_source(src).unwrap();
    let uni = Universe { modules: vec![ModFile { name: "main".into(), file }] };
    let mut r = Reflector::new(&uni);
    let root = r.declared("main", "User").unwrap();
    let table = r.table;
    let user = &table.types[root];
    assert_eq!((user.name.as_str(), user.kind.as_str()), ("User", "record"));
    assert_eq!(user.attrs[0].name, "derive");
    assert_eq!(user.attrs[0].args, vec!["json".to_string()]);
    assert_eq!(user.fields.len(), 6);
    assert_eq!(user.fields[0].name, "id");
    assert_eq!(table.types[user.fields[0].ty].kind, "int");
    assert!(user.fields[0].attrs.is_empty());
    assert_eq!(user.fields[1].attrs[0].name, "json");
    assert_eq!(user.fields[1].attrs[0].args, vec!["full_name".to_string()]);
    let homes = &table.types[user.fields[2].ty];
    assert_eq!((homes.kind.as_str(), homes.name.as_str()), ("list", "[]Address"));
    let address = &table.types[homes.elem as usize];
    assert_eq!(address.fields[1].name, "zip");
    let nick = &table.types[user.fields[3].ty];
    assert_eq!(nick.kind, "optional");
    assert_eq!(table.types[nick.inner as usize].kind, "text");
    let scores = &table.types[user.fields[4].ty];
    assert_eq!(scores.kind, "map");
    assert_eq!(table.types[scores.key as usize].kind, "text");
    assert_eq!(table.types[scores.value as usize].kind, "int");
    let shape = &table.types[user.fields[5].ty];
    assert_eq!(shape.kind, "enum");
    assert_eq!(shape.variants[0].name, "Circle");
    assert_eq!(shape.variants[0].fields[0].name, "r");
    assert_eq!(shape.variants[0].attrs[0].args, vec!["round".to_string()]);
    assert!(shape.variants[1].fields.is_empty());
    assert_eq!(shape.variants[2].fields[0].name, "_0");
    // the text of a call and back
    let call = WireCall { id: 3, generator: "json.json".into(), arg_kind: "derive".into(), table: table.clone(), root, raw: Some("a b\nc\\d".into()) };
    let text = serialize_call(&call);
    assert!(text.starts_with("call 3 json.json derive\n"));
    let back = parse_wire(&text).unwrap();
    assert_eq!(back, vec![call]);
    // a later version may add lines
    let more = text.replace("end\n", "future 1 2 3\nend\n");
    assert_eq!(parse_wire(&more).unwrap().len(), 1);
}

#[test]
fn a_type_with_attributes_and_an_enum_reach_the_generator() {
    let src = r#"package main
use reflect

#[derive(inspect)]
type User = record
    id: int
    #[json("full_name")]
    name: text
end

#[derive(inspect)]
type Shape = enum
    Circle(r: float)
    Dot
end

#[derive]
comptime func inspect(t: Type) -> text
    var s = ""
    if t.kind() == "record"
        for f in t.fields()
            var a = ""
            for at in f.attrs
                a = a + " #{at.name}(" + at.args[0] + ")"
            end
            s = s + "{f.name}:{f.ty.kind()}{a};"
        end
    end
    if t.kind() == "enum"
        for v in t.variants()
            s = s + v.name + "/" + "{v.fields.len}"
            for f in v.fields
                s = s + "[{f.name}:{f.ty.kind()}]"
            end
            s = s + ";"
        end
    end
    return "func (v: {t.name()}) inspect() -> text\n    return \"{s}\"\nend\n"
end

func main()
    let u = User { id: 1, name: "n" }
    print(u.inspect())
    print(Shape.Dot.inspect())
end
"#;
    assert_eq!(run(src).trim(), "id:int;name:text #json(full_name);\nCircle/1[r:float];Dot/0;");
}

// ---- derive ----

const POINT: &str = r##"
type Point = record
    x: int
    y: int
end
"##;

#[test]
fn two_derives_on_one_type_in_either_order() {
    for order in ["describe, size_hint", "size_hint, describe"] {
        let src = format!(
            r##"package main
use reflect

#[derive({})]
type Point = record
    x: int
    y: int
end

#[derive]
comptime func describe(t: Type) -> text
    return "func (v: {{t.name()}}) describe() -> text\n    return \"{{t.name()}}\"\nend\n"
end

#[derive]
comptime func size_hint(t: Type) -> text
    return "func (v: {{t.name()}}) size_hint() -> int\n    return {{t.fields().len}}\nend\n"
end

func main()
    let p = Point {{ x: 1, y: 2 }}
    print(p.describe(), p.size_hint())
end
"##,
            order
        );
        assert_eq!(run(&src).trim(), "Point 2", "{}", order);
    }
}

#[test]
fn an_unknown_derive_is_reported_at_the_attribute() {
    let src = format!("package main\n\n#[derive(nothing)]{}\nfunc main()\nend\n", POINT);
    let e = compile_error(&src);
    assert!(e.contains("no derive named `nothing` was found in the imported packages"), "{}", e);
    assert!(e.contains(&format!("main.br:{}:", line_of(&src, "type Point"))), "{}", e);
}

#[test]
fn a_derive_name_in_two_packages_is_ambiguous_until_qualified() {
    for (pkg, word) in [("cta_one", "one"), ("cta_two", "two")] {
        brasa::comptime::register_package(
            pkg,
            &format!(
                "package {}\nuse reflect\n#[derive]\ncomptime func tag(t: Type) -> text\n    return \"func (v: {{t.name()}}) tag() -> text\\n    return \\\"{}\\\"\\nend\\n\"\nend\n",
                pkg, word
            ),
        );
    }
    let ambiguous = format!("package main\nuse reflect\nuse cta_one\nuse cta_two\n\n#[derive(tag)]{}\nfunc main()\nend\n", POINT);
    let e = compile_error(&ambiguous);
    assert!(e.contains("derive `tag`") && e.contains("ambiguous") && e.contains("`cta_one.tag` or `cta_two.tag`"), "{}", e);
    let qualified = format!(
        "package main\nuse reflect\nuse cta_one\nuse cta_two\n\n#[derive(cta_two.tag)]{}\nfunc main()\n    let p = Point {{ x: 1, y: 2 }}\n    print(p.tag())\nend\n",
        POINT
    );
    assert_eq!(run(&qualified).trim(), "two");
}

#[test]
fn a_user_method_and_a_derived_one_collide() {
    let src = format!(
        r##"package main
use reflect

#[derive(describe)]{}
func (p: Point) describe() -> text
    return "mine"
end
{}
func main()
end
"##,
        POINT, DESCRIBE
    );
    let e = compile_error(&src);
    assert!(e.contains("`describe` generates the method `describe` for `Point`"), "{}", e);
    assert!(e.contains(&format!("already declared for `Point` at line {}", line_of(&src, "func (p: Point) describe"))), "{}", e);
}

// ---- hygiene ----

#[test]
fn a_generated_helper_cannot_capture_or_collide_with_a_user_name() {
    let src = format!(
        r##"package main
use reflect

#[derive(tagged)]{}
func escape(s: text) -> text
    return "<{{s}}>"
end

#[derive]
comptime func tagged(t: Type) -> text
    return "func escape(s: text) -> text\n    return \"[\{{s\}}]\"\nend\nfunc (v: {{t.name()}}) tagged() -> text\n    return escape(\"{{t.name()}}\")\nend\n"
end

func main()
    let p = Point {{ x: 1, y: 2 }}
    print(escape("a"), p.tagged())
end
"##,
        POINT
    );
    assert_eq!(run(&src).trim(), "<a> [Point]");
}

#[test]
fn fresh_names_are_unique_and_the_double_underscore_prefix_is_reserved() {
    let src = format!(
        r##"package main
use reflect

#[derive(sum)]{}
#[derive]
comptime func sum(t: Type) -> text
    let a = reflect.fresh("tmp")
    let b = reflect.fresh("tmp")
    return "func (v: {{t.name()}}) sum() -> int\n    let {{a}} = v.x\n    let {{b}} = v.y\n    return {{a}} + {{b}}\nend\n"
end

func main()
    let p = Point {{ x: 20, y: 22 }}
    print(p.sum())
end
"##,
        POINT
    );
    assert_eq!(run(&src).trim(), "42");
    let e = compile_error("package main\nfunc __helper()\nend\nfunc main()\nend\n");
    assert!(e.contains("names that start with `__` are reserved for generated code"), "{}", e);
}

// ---- types as arguments ----

const ZERO: &str = r##"
#[generated(zero_gen)]
func zero(comptime t: Type) -> t

comptime func zero_gen(t: Type) -> text
    var fields = ""
    for f in t.fields()
        var v = "0"
        if f.ty.kind() == "text"
            v = "\"\""
        end
        if fields != ""
            fields = fields + ", "
        end
        fields = fields + "{f.name}: {v}"
    end
    return "return t \{ {fields} \}"
end
"##;

const ZERO_TYPES: &str = r##"
type Point = record
    x: int
    y: int
end

type Config = record
    port: int
    name: text
end
"##;

#[test]
fn a_generated_function_runs_its_generator_once_per_type() {
    let zero = ZERO.replace("zero_gen", "zero_gen_once");
    let src = format!(
        "package main\nuse reflect\n{}{}\nfunc main()\n    let a = zero(Point)\n    let b = zero(Point)\n    let c = zero(Config)\n    print(a.x, b.y, c.port, c.name.len)\nend\n",
        ZERO_TYPES, zero
    );
    assert_eq!(run(&src).trim(), "0 0 0 0");
    assert_eq!(launches("zero_gen_once"), 2);
}

#[test]
fn a_generated_function_accepts_an_explicit_type_argument() {
    let src = format!("package main\nuse reflect\n{}{}\nfunc main()\n    let a = zero<Config>()\n    print(a.port)\nend\n", ZERO_TYPES, ZERO);
    assert_eq!(run(&src).trim(), "0");
}

#[test]
fn a_runtime_value_is_not_a_type() {
    let src = format!("package main\nuse reflect\n{}{}\nfunc main()\n    let k = 3\n    let a = zero(k)\nend\n", ZERO_TYPES, ZERO);
    let e = compile_error(&src);
    assert!(e.contains("must be a type known at compile time"), "{}", e);
}

#[test]
fn specializations_leak_nothing() {
    let generated = format!(
        "package main\nuse reflect\n{}{}\nfunc main()\n    let a = zero(Point)\n    print(a.x, live_objects())\nend\n",
        ZERO_TYPES, ZERO
    );
    let by_hand = format!(
        "package main\nuse reflect\n{}\nfunc zero_by_hand() -> Point\n    return Point {{ x: 0, y: 0 }}\nend\nfunc main()\n    let a = zero_by_hand()\n    print(a.x, live_objects())\nend\n",
        ZERO_TYPES
    );
    assert_eq!(run(&generated), run(&by_hand));
}

// ---- blocks ----

const BLOCKS: &str = r##"
#[block]
comptime func shout(raw: text) -> text
    return "\"{text.upper(raw)}\""
end

#[block]
comptime func show(raw: text) -> text
    return "\"hi \{{text.trim(raw)}\}\""
end

#[block]
comptime func twice(raw: text) -> text
    return raw + "\n" + raw
end
"##;

#[test]
fn a_block_is_an_expression_a_template_or_statements() {
    let src = format!(
        "package main\nuse reflect\nuse text\n{}\nfunc main()\n    let s = shout\nhello\nend\n    print(s)\n    let name = \"Ana\"\n    let t = show\n        name\n    end\n    print(t)\n    twice\n        print(\"again\")\n    end\nend\n",
        BLOCKS
    );
    assert_eq!(run(&src).trim(), "HELLO\nhi Ana\nagain\nagain");
}

#[test]
fn a_block_content_need_not_be_brasa() {
    let src = format!("package main\nuse text\n{}\nfunc main()\n    let s = shout\n{}\nend\n    print(s)\nend\n", BLOCKS, "this is ] not <Brasa> at all ((");
    assert_eq!(run(&src).trim(), "THIS IS ] NOT <BRASA> AT ALL ((");
}

#[test]
fn an_unknown_block_name_is_an_ordinary_identifier() {
    let e = compile_error("package main\nfunc main()\n    let s = nothing_here\nend\n");
    assert!(e.contains("nothing_here"), "{}", e);
}

// ---- Type and comptime functions outside comptime ----

#[test]
fn type_exists_only_at_compile_time() {
    let e = compile_error("package main\nfunc f(t: Type)\nend\nfunc main()\nend\n");
    assert!(e.contains("`Type` exists only at compile time"), "{}", e);
}

#[test]
fn comptime_functions_are_not_callable_from_runtime_code() {
    let src = "package main\ncomptime func gen(raw: text) -> text\n    return raw\nend\nfunc main()\n    print(gen(\"x\"))\nend\n";
    let e = compile_error(src);
    assert!(e.contains("comptime function `gen` runs only at build time"), "{}", e);
}

// ---- the subset ----

fn subset_error(decls: &str, body: &str) -> String {
    compile_error(&format!(
        "package main\nuse fs\nuse reflect\n{}\ncomptime func g(raw: text) -> text\n{}\n    return raw\nend\nfunc main()\nend\n",
        decls, body
    ))
}

#[test]
fn the_subset_rejects_extern_asm_spawn_async_await_channels_pointers_unsafe_and_globals() {
    let e = subset_error("extern func GetTickCount() -> u32", "    unsafe\n        let x = GetTickCount()\n    end");
    assert!(e.contains("`extern` is not allowed in comptime code: g"), "{}", e);
    let e = subset_error("", "    unsafe\n        asm\n            nop\n        end\n    end");
    assert!(e.contains("`asm` is not allowed in comptime code: g"), "{}", e);
    let e = subset_error("func work()\nend", "    spawn work()");
    assert!(e.contains("`spawn` is not allowed in comptime code: g"), "{}", e);
    let e = subset_error("async func work() -> int\n    return 1\nend\nfunc helper() -> int\n    let f = work()\n    return 1\nend", "    let x = helper()");
    assert!(e.contains("`async` is not allowed in comptime code: g -> helper -> work"), "{}", e);
    let e = subset_error("", "    let x = await g(raw)");
    assert!(e.contains("`await` is not allowed in comptime code"), "{}", e);
    let e = subset_error("", "    let c = channel(int)");
    assert!(e.contains("`channel` is not allowed in comptime code"), "{}", e);
    let e = subset_error("func peek(p: *int) -> int\n    return 0\nend", "    let x = peek(nil)");
    assert!(e.contains("`raw pointer` is not allowed in comptime code: g -> peek"), "{}", e);
    let e = subset_error("", "    unsafe\n        let y = 1\n    end");
    assert!(e.contains("`unsafe` is not allowed in comptime code"), "{}", e);
    let e = subset_error("var counter = 0", "    counter += 1");
    assert!(e.contains("global `var` is not allowed in comptime code"), "{}", e);
}

#[test]
fn the_subset_follows_calls_through_helpers_closures_and_generic_instances() {
    let helper = "func helper() -> int\n    let r = fs.read(\"x\")\n    return 1\nend";
    let e = subset_error(helper, "    let x = helper()");
    assert!(e.contains("is not allowed in comptime code: g -> helper -> fs.read"), "{}", e);
    let e = subset_error(helper, "    let f = func()\n        helper()\n    end\n    f()");
    assert!(e.contains("g -> helper -> fs.read"), "{}", e);
    let e = subset_error("func id<T>(x: T) -> T\n    let r = fs.read(\"x\")\n    return x\nend", "    let y = id(1)");
    assert!(e.contains("g -> id -> fs.read"), "{}", e);
}

#[test]
fn text_list_format_and_math_helpers_are_in_the_subset() {
    let src = r##"package main
use reflect
use text
use list
use format
use math

#[derive(helpers)]
type Point = record
    x: int
end

#[derive]
comptime func helpers(t: Type) -> text
    let words = text.split("a,b,c", ",")
    let up = list.map(words, func(w) text.upper(w) end)
    let joined = text.join(up, "-")
    let padded = format.pad_left(joined, 8, ".")
    let n = math.checked_add(1, 2) ?? 0
    let m = math.abs(2.5)
    let sorted = list.sort([3, 1, 2])
    let big = format.thousands(1234567)
    return "func (v: {t.name()}) helpers() -> text\n    return \"{padded} {n} {sorted.len} {big}\"\nend\n"
end

func main()
    let p = Point { x: 1 }
    print(p.helpers())
end
"##;
    assert_eq!(run(src).trim(), "...A-B-C 3 3 1,234,567");
}

// ---- diagnostics ----

#[test]
fn a_generator_refuses_a_field_at_its_line() {
    let src = r##"package main
use reflect

#[derive(strict)]
type Bad = record
    ok: int
    cb: func(int) -> int
end

#[derive]
comptime func strict(t: Type) -> text
    for f in t.fields()
        if f.ty.kind() == "func"
            reflect.fail("fields of kind func cannot be encoded", f)
        end
    end
    return ""
end

func main()
end
"##;
    let e = compile_error(src);
    assert!(
        e.contains(&format!("main.br:{}: error from comptime function `strict`: fields of kind func cannot be encoded", line_of(src, "cb: func"))),
        "{}",
        e
    );
}

#[test]
fn an_error_in_generated_code_names_the_generator_the_use_site_and_the_line() {
    let src = r##"package main
use reflect

#[derive(bad_gen)]
type X = record
    a: int
end

#[derive]
comptime func bad_gen(t: Type) -> text
    return "func (v: {t.name()}) f() -> int\n    let a = 1\n    return \"text\"\nend\n"
end

func main()
end
"##;
    let e = compile_error(src);
    assert!(e.contains(&format!("main.br:{}: error in code generated by `bad_gen` for `X` (derive attribute)", line_of(src, "type X"))), "{}", e);
    assert!(e.contains("generated line 3:     return \"text\""), "{}", e);
}

#[test]
fn a_parse_error_in_generated_text_is_reported_the_same_way() {
    let src = r##"package main
use reflect

#[derive(parse_gen)]
type X = record
    a: int
end

#[derive]
comptime func parse_gen(t: Type) -> text
    return "func (v: {t.name()}) f(\n"
end

func main()
end
"##;
    let e = compile_error(src);
    assert!(e.contains("error in code generated by `parse_gen` for `X`"), "{}", e);
    assert!(e.contains("generated line"), "{}", e);
}

#[test]
fn an_expansion_that_never_settles_stops_after_eight_rounds() {
    let src = r##"package main
use reflect

#[derive(again)]
type T0 = record
    x: int
end

#[derive]
comptime func again(t: Type) -> text
    return "#[derive(again)]\ntype {t.name()}X = record\n    x: int\nend\n"
end

func main()
end
"##;
    let e = compile_error(src);
    assert!(e.contains("did not settle after 8 rounds: again -> again"), "{}", e);
    assert!(e.contains(&format!("main.br:{}:", line_of(src, "type T0"))), "{}", e);
}

// ---- limits ----

#[test]
fn an_endless_generator_is_stopped_by_the_time_limit() {
    let src = r##"package main
use reflect

#[derive(endless_gen)]
type X = record
    a: int
end

#[derive]
comptime func endless_gen(t: Type) -> text
    var n = 0
    while true
        n += 1
    end
    return "{n}"
end

func main()
end
"##;
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    brasa::comptime::set_timeout_secs(1);
    let e = compile_error(src);
    brasa::comptime::set_timeout_secs(5);
    assert!(e.contains("comptime function `endless_gen` did not finish within 1 seconds"), "{}", e);
    assert!(e.contains(&format!("main.br:{}:", line_of(src, "type X"))), "{}", e);
}

#[test]
fn an_oversized_output_and_a_crash_are_errors() {
    let big = r##"package main
use reflect
use text

#[derive(big_gen)]
type X = record
    a: int
end

#[derive]
comptime func big_gen(t: Type) -> text
    return text.repeat("x", 2000000)
end

func main()
end
"##;
    let e = compile_error(big);
    assert!(e.contains("comptime function `big_gen` produced more than 1 MiB of output"), "{}", e);
    let crash = big.replace("big_gen", "crash_gen").replace("return text.repeat(\"x\", 2000000)", "abort(\"boom\")\n    return \"\"");
    let e = compile_error(&crash);
    assert!(e.contains("comptime function `crash_gen` crashed (exit code 101)") && e.contains("boom"), "{}", e);
}

// ---- the cache ----

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn cached_program(gen: &str, body: &str, helper: &str, extra_field: &str) -> String {
    format!(
        r##"package main
use reflect

#[derive({gen})]
type X = record
    a: int
{extra_field}
end

func suffix() -> text
    return "{helper}"
end

#[derive]
comptime func {gen}(t: Type) -> text
    return "func (v: {{t.name()}}) tag() -> text\n    return \"{body}{{suffix()}}\"\nend\n"
end

func main()
end
"##,
        gen = gen,
        body = body,
        helper = helper,
        extra_field = extra_field
    )
}

fn compiles(src: &str) {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let _ = std::fs::remove_file(exe);
}

#[test]
fn an_identical_second_build_does_not_run_the_generator() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let src = cached_program("cache_same", "a", "b", "");
    compiles(&src);
    assert_eq!(launches("cache_same"), 1);
    compiles(&src);
    assert_eq!(launches("cache_same"), 1);
}

#[test]
fn an_edited_generator_runs_again() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    compiles(&cached_program("cache_body", "a", "b", ""));
    compiles(&cached_program("cache_body", "other", "b", ""));
    assert_eq!(launches("cache_body"), 2);
}

#[test]
fn the_cache_key_covers_the_helpers_a_generator_calls() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    compiles(&cached_program("cache_helper", "a", "b", ""));
    compiles(&cached_program("cache_helper", "a", "changed", ""));
    assert_eq!(launches("cache_helper"), 2);
    // moving code around (whitespace, comments) does not change the key
    compiles(&cached_program("cache_helper", "a", "changed", "").replace("func suffix", "// a comment\n\nfunc suffix"));
    assert_eq!(launches("cache_helper"), 2);
}

#[test]
fn a_changed_type_runs_the_generator_again() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    compiles(&cached_program("cache_type", "a", "b", ""));
    compiles(&cached_program("cache_type", "a", "b", "    extra: int"));
    assert_eq!(launches("cache_type"), 2);
}

#[test]
fn a_different_compiler_version_and_the_no_cache_flag_run_the_generator_again() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let src = cached_program("cache_version", "a", "b", "");
    compiles(&src);
    brasa::comptime::set_version_tag("another compiler");
    compiles(&src);
    brasa::comptime::set_version_tag("");
    assert_eq!(launches("cache_version"), 2);
    compiles(&src);
    assert_eq!(launches("cache_version"), 2);
    brasa::comptime::set_cache_enabled(false);
    compiles(&src);
    compiles(&src);
    brasa::comptime::set_cache_enabled(true);
    assert_eq!(launches("cache_version"), 4);
}

// ---- cross-compilation ----

const TARGETED: &str = r##"package main
use reflect

#[derive(targeted)]
type X = record
    a: int
end

#[derive]
comptime func targeted(t: Type) -> text
    return "func (v: {t.name()}) built_for_{reflect.target()}() -> text\n    return \"{reflect.target()}\"\nend\n"
end

func main()
    let x = X { a: 1 }
    print(x.built_for_windows())
end
"##;

#[test]
fn comptime_runs_on_the_host_whatever_the_target() {
    use brasa::codegen_x64::{compile_to_linux, generate_asm_for, CompileOptions, Target};
    setup();
    let file = parse_source(TARGETED).unwrap();
    let opts = CompileOptions::default();
    // a Windows target: the generated method is named for it, and the program runs
    assert_eq!(run(TARGETED).trim(), "windows");
    // a Linux target (built here, not run): the generator ran as a host program and was told
    // the target is linux
    let e = generate_asm_for(&file, &opts, Target::Linux).unwrap_err();
    assert!(e.contains("built_for_windows"), "{}", e);
    let linux_src = TARGETED.replace("x.built_for_windows()", "x.built_for_linux()");
    let file = parse_source(&linux_src).unwrap();
    let out = std::env::temp_dir().join(format!("brasa-comptime-test-linux-{}", std::process::id()));
    compile_to_linux(&file, out.to_str().unwrap(), &opts).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    assert_eq!(&bytes[..4], b"\x7fELF");
}

// ---- the example ----

#[test]
fn the_comptime_example_prints_what_the_docs_say() {
    let src = std::fs::read_to_string("examples/comptime.br").unwrap();
    assert_eq!(run(&src).trim(), "Pair(left=a, right=b)\nzero(Config): port 0, name length 0\nHELLO FROM A BLOCK\n7");
}
