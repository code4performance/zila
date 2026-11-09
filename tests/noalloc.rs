use zila::codegen_x64::generate_asm;
use zila::parse_source;

fn check_err(src: &str, want: &str) {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let err = generate_asm(&file).unwrap_err();
    assert!(
        err.contains(want),
        "expected error containing {:?}, got {:?}",
        want,
        err
    );
}

fn check_ok(src: &str) {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    generate_asm(&file).unwrap_or_else(|e| panic!("unexpected compile error: {}", e));
}

#[test]
fn attribute_on_type_is_rejected() {
    let src = "package main

#[no_alloc]
type Point = record
    x: int
    y: int
end

func main()
end
";
    check_err(src, "#[no_alloc] only applies to functions ('Point')");
}

#[test]
fn attribute_on_var_is_rejected() {
    let src = "package main

#[no_alloc]
var global_counter = 0

func main()
end
";
    check_err(src, "#[no_alloc] only applies to functions ('global_counter')");
}

#[test]
fn unknown_allocator_role_is_rejected() {
    let src = "package main

#[allocator(bump)]
func my_bump(size: u64, align: u64) -> *u8
    return nil
end

func main()
end
";
    check_err(src, "unknown allocator role 'bump' (expected 'alloc', 'free' or 'realloc')");
}

#[test]
fn allocator_missing_argument_is_rejected() {
    let src = "package main

#[allocator]
func my_alloc(size: u64, align: u64) -> *u8
    return nil
end

func main()
end
";
    check_err(src, "#[allocator] requires one argument: 'alloc', 'free' or 'realloc'");
}

#[test]
fn non_allocating_function_compiles() {
    let src = "package main

#[no_alloc]
func add(a: int, b: int) -> int
    let c = a + b
    return c
end

func main()
    let x = add(2, 3)
end
";
    check_ok(src);
}

#[test]
fn allocating_interpolation_is_rejected() {
    let src = "package main

#[no_alloc]
func greet(name: text) -> text
    return \"hello {name}\"
end

func main()
end
";
    check_err(src, "error: 'greet' is #[no_alloc] but allocates");
    check_err(src, "greet -> text interpolation at main.zila:5");
}

#[test]
fn constant_text_and_non_capturing_closure_accepted() {
    let src = "package main

func helper(f: func(int))
end

#[no_alloc]
func ok_func() -> text
    let f = func(x: int)
        let y = x * 2
    end
    helper(f)
    return \"ready\"
end

func main()
end
";
    check_ok(src);
}

#[test]
fn capturing_closure_is_rejected() {
    let src = "package main

#[no_alloc]
func make_adder(n: int)
    let f = func(x: int)
        let y = x + n
    end
end

func main()
end
";
    check_err(src, "closure that captures 'n'");
}

#[test]
fn transitive_call_chain_in_message() {
    let src = "package main

func c() -> []int
    return [1, 2, 3]
end

func b() -> []int
    return c()
end

#[no_alloc]
func a() -> []int
    return b()
end

func main()
end
";
    check_err(src, "error: 'a' is #[no_alloc] but allocates");
    check_err(src, "a -> b -> c -> list literal at main.zila:4");
}

#[test]
fn recursion_without_allocation_is_accepted() {
    let src = "package main

#[no_alloc]
func fib(n: int) -> int
    if n <= 1
        return n
    end
    return fib(n - 1) + fib(n - 2)
end

func main()
end
";
    check_ok(src);
}

#[test]
fn list_literal_chain_names_the_literal() {
    let src = "package main

func c() -> []int
    return [1]
end

func b() -> []int
    return c()
end

#[no_alloc]
func a() -> []int
    return b()
end

func main()
end
";
    check_err(src, "a -> b -> c -> list literal at main.zila:4");
}

#[test]
fn callee_without_attribute_that_does_not_allocate_is_accepted() {
    check_ok("package main

func add(a: int, b: int) -> int
    return a + b
end

#[no_alloc]
func total(a: int, b: int) -> int
    return add(a, b) + add(b, a)
end

func main()
end
");
}

#[test]
fn a_standard_library_function_that_allocates_ends_the_chain() {
    let src = "package main

use text

#[no_alloc]
func shout(s: text) -> text
    return text.upper(s)
end

func main()
end
";
    let file = parse_source(src).unwrap();
    let e = generate_asm(&file).unwrap_err();
    assert!(e.contains("'shout' is #[no_alloc] but allocates") && e.contains("shout -> upper"), "{}", e);
}

#[test]
fn a_standard_library_function_that_cannot_allocate_is_accepted() {
    check_ok("package main

use text

#[no_alloc]
func has(s: text) -> bool
    return text.contains(s, \"x\")
end

func main()
end
");
}

#[test]
fn spawn_new_defer_and_channel_are_rejected() {
    for (body, op) in [
        ("spawn work()", "spawn"),
        ("let c = channel(1)", "channel"),
        ("defer work()", "defer"),
    ] {
        let src = format!("package main

func work()
end

#[no_alloc]
func f()
    {}
end

func main()
end
", body);
        check_err(&src, &format!("f -> {} at main.zila:", op));
    }
    check_err(
        "package main

type P = record
    x: int
end

#[no_alloc]
func f()
    let p = new P{ x: 1 }
end

func main()
end
",
        "new P",
    );
}

#[test]
fn collection_creation_is_rejected() {
    check_err("package main

#[no_alloc]
func f()
    let m = Map.new()
end

func main()
end
", "Map.new");
    check_err("package main

#[no_alloc]
func f()
    let xs: []int = []
end

func main()
end
", "list literal");
}

#[test]
fn call_through_function_value_is_rejected() {
    let src = "package main

#[no_alloc]
func run(cb: func(int) -> int) -> int
    return cb(1)
end

func main()
end
";
    check_err(src, "call through function value 'cb'");
}

#[test]
fn interface_bounded_calls_are_static_and_analysed() {
    // interfaces only bound type parameters in Zila Lang, so a call on `T: Shape` is resolved to the
    // methods of that name (static dispatch) and analysed like a direct call
    let src = "package main

type Shape = interface
    func area(self) -> int
end

type Sq = record
    n: int
end

func (s: Sq) area() -> int
    return [s.n][0]
end

#[no_alloc]
func measure<T: Shape>(shape: T) -> int
    return shape.area()
end

func main()
end
";
    check_err(src, "measure -> area -> list literal");
    check_ok(&src.replace("return [s.n][0]", "return s.n"));
}

#[test]
fn method_on_a_known_type_is_analysed_as_a_direct_call() {
    let ok = "package main

type Box = record
    n: int
end

func (b: Box) twice() -> int
    return b.n + b.n
end

#[no_alloc]
func measure(b: Box) -> int
    return b.twice()
end

func main()
end
";
    check_ok(ok);
    let bad = ok.replace("return b.n + b.n", "return [b.n][0]");
    check_err(&bad, "measure -> twice -> list literal");
}

#[test]
fn extern_is_trusted_unless_it_allocates() {
    let base = "package main

#[link(\"kernel32\")]
#EXTERN
extern func GetTickCount() -> u32

#[no_alloc]
func now() -> u32
    unsafe
        return GetTickCount()
    end
end

func main()
end
";
    check_ok(&base.replace("#EXTERN
", ""));
    check_err(&base.replace("#EXTERN", "#[allocates]"), "extern 'GetTickCount' declared #[allocates]");
}

#[test]
fn calling_an_async_function_is_rejected() {
    let src = "package main

async func fetch() -> int
    return 1
end

#[no_alloc]
func f()
    let x = fetch()
end

func main()
end
";
    check_err(src, "async function 'fetch' (creates a future)");
}

#[test]
fn generic_functions_are_accepted_or_rejected_by_their_body() {
    let ok = "package main

func first<T>(items: []T) -> T
    return items[0]
end

#[no_alloc]
func f(xs: []int) -> int
    return first(xs)
end

func main()
end
";
    check_ok(ok);
    let bad = "package main

func label<T>(x: T) -> text
    return \"v = {x}\"
end

#[no_alloc]
func f() -> text
    return label(1)
end

func main()
end
";
    check_err(bad, "f -> label -> text interpolation");
}

#[test]
fn a_write_that_may_copy_is_rejected() {
    let src = "package main

#[no_alloc]
func set(xs: []int)
    xs[0] = 1
end

func main()
end
";
    check_err(src, "this write may copy");
}

#[test]
fn every_standard_module_still_compiles_with_its_no_alloc_promises() {
    // the promises in std/ are checked whenever the module is imported
    for m in ["text", "math", "errors", "float", "list", "set", "sorted"] {
        let src = format!("package main\n\nuse {}\n\nfunc main()\nend\n", m);
        let file = parse_source(&src).unwrap();
        generate_asm(&file).unwrap_or_else(|e| panic!("use {}: {}", m, e));
    }
}

#[test]
fn a_std_function_marked_no_alloc_that_allocates_fails_the_library() {
    // the same check on a library module written the way std/ is: the chain ends inside it
    let main = parse_source("package main\n\nfunc main()\nend\n").unwrap();
    let lib = parse_source("package lib\n\n#[no_alloc]\nfunc broken(n: int) -> []int\n    return [n]\nend\n").unwrap();
    let modules = vec![("main".to_string(), main), ("lib".to_string(), lib)];
    let e = zila::noalloc::check_program(&modules, &std::collections::HashSet::new()).unwrap_err();
    assert!(e.contains("lib.zila:4") && e.contains("broken -> list literal"), "{}", e);
}

#[test]
fn the_no_alloc_example_runs() {
    let src = std::fs::read_to_string("examples/no_alloc.zila").unwrap();
    let file = parse_source(&src).unwrap();
    let exe = std::env::temp_dir().join(format!("zila-noalloc-example-{}.exe", std::process::id()));
    match zila::codegen_x64::compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") || e.contains("vcvars64") => return,
        Err(e) => panic!("{}", e),
    }
    let out = std::process::Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    // 12345 -> (((49*31+50)*31+51)*31+52)*31+53 modulo 65521
    let mut sum: i64 = 0;
    for b in "12345".bytes() {
        sum = (sum * 31 + b as i64) % 65521;
    }
    assert_eq!(String::from_utf8_lossy(&out.stdout).replace('\r', ""), format!("checksum: {}\nparsed: 2024 and -1\n", sum));
}

#[test]
fn writes_through_a_ref_parameter_are_accepted() {
    let src = "package main

#[no_alloc]
func set(ref xs: []int)
    xs[0] = 1
end

func main()
end
";
    check_ok(src);
}
