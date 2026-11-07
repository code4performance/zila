use brasa::codegen_x64::{generate_asm, generate_asm_for, CompileOptions, Target};
use brasa::parse_source;

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
    check_err(src, "greet -> text interpolation at main.br:5");
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
    check_err(src, "a -> b -> c -> list literal at main.br:4");
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
