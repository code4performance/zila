use brasa::ast::*;
use brasa::parse_source;
use std::fs;

#[test]
fn all_examples_parse() {
    let mut n = 0;
    for entry in fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map_or(false, |e| e == "br") {
            let src = fs::read_to_string(&path).unwrap();
            if let Err(d) = parse_source(&src) {
                panic!("{}: {}", path.display(), d);
            }
            n += 1;
        }
    }
    assert!(n >= 8, "expected the example files, found {}", n);
}

/// Parse `let x = <expr>` inside a function and return the expression.
fn expr(src: &str) -> Expr {
    let code = format!("package t\nfunc f()\n    let x = {}\nend\n", src);
    let file = parse_source(&code).unwrap_or_else(|d| panic!("{}: {}", src, d));
    match &file.decls[0].kind {
        DeclKind::Func(f) => match &f.body[0].kind {
            StmtKind::Var(v) => v.value.clone().unwrap(),
            _ => panic!("not a let"),
        },
        _ => panic!("not a func"),
    }
}

fn shape(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Ident(n) => n.clone(),
        ExprKind::Binary { op, lhs, rhs } => format!("({} {} {})", op, shape(lhs), shape(rhs)),
        ExprKind::Unary { op, expr } => format!("({} {})", op, shape(expr)),
        ExprKind::Field { expr, name, safe } => {
            format!("({}{} {})", if *safe { "?." } else { "." }, shape(expr), name)
        }
        ExprKind::Call { callee, args } => {
            let a: Vec<String> = args.iter().map(|a| shape(&a.value)).collect();
            format!("(call {} [{}])", shape(callee), a.join(" "))
        }
        ExprKind::Propagate(e) => format!("(? {})", shape(e)),
        ExprKind::Await(e) => format!("(await {})", shape(e)),
        ExprKind::Deref(e) => format!("(^ {})", shape(e)),
        _ => format!("{:?}", e.kind),
    }
}

#[test]
fn arithmetic_precedence() {
    assert_eq!(shape(&expr("a + b * c")), "(+ a (* b c))");
    assert_eq!(shape(&expr("a - b - c")), "(- (- a b) c)");
    assert_eq!(shape(&expr("(a + b) * c")), "(* (+ a b) c)");
    assert_eq!(shape(&expr("-a * b")), "(* (- a) b)");
}

#[test]
fn bitwise_words_bind_like_add_and_mul() {
    // bitand is multiplicative, bitor/bitxor additive
    assert_eq!(shape(&expr("a bitor b bitand c")), "(bitor a (bitand b c))");
    assert_eq!(shape(&expr("a bitxor b bitor c")), "(bitor (bitxor a b) c)");
    assert_eq!(shape(&expr("bitnot a")), "(bitnot a)");
}

#[test]
fn logic_and_coalesce_precedence() {
    assert_eq!(shape(&expr("a or b and c")), "(or a (and b c))");
    assert_eq!(shape(&expr("a ?? b or c")), "(?? a (or b c))");
    assert_eq!(shape(&expr("not a == b")), "(== (not a) b)");
}

#[test]
fn postfix_forms() {
    assert_eq!(shape(&expr("a.b.c")), "(.(.a b) c)");
    assert_eq!(shape(&expr("a?.b")), "(?.a b)");
    assert_eq!(shape(&expr("try f(a, b)")), "(? (call f [a b]))");
    assert_eq!(shape(&expr("p^")), "(^ p)");
    assert_eq!(shape(&expr("&p")), "(& p)");
}

#[test]
fn method_chain_across_lines() {
    let e = expr("xs\n        .map(f)\n        .filter(g)");
    assert_eq!(shape(&e), "(call (.(call (.xs map) [f]) filter) [g])");
}

#[test]
fn range_and_comparison() {
    assert_eq!(shape(&expr("1..n + 1")), "(.. 1 (+ n 1))");
    assert_eq!(shape(&expr("a < b")), "(< a b)");
}

#[test]
fn text_interpolation_is_parsed() {
    let e = expr("\"hi {a + 1} and {f(\"x\")}\"");
    match e.kind {
        ExprKind::Text(parts) => {
            assert_eq!(parts.len(), 4);
            assert!(matches!(&parts[0], TextPart::Lit(s) if s == "hi "));
            assert!(matches!(&parts[1], TextPart::Expr(_)));
            assert!(matches!(&parts[3], TextPart::Expr(_)));
        }
        k => panic!("{:?}", k),
    }
}

#[test]
fn nested_generics_split_shift_token() {
    let src = "package t\nlet x: Map<text, List<i32>> = nil\n";
    assert!(parse_source(src).is_ok());
}

#[test]
fn else_if_chain_has_one_end() {
    let e = expr("if a\n 1\n else if b\n 2\n else\n 3\n end");
    match e.kind {
        ExprKind::If { branches, else_body } => {
            assert_eq!(branches.len(), 2);
            assert!(else_body.is_some());
        }
        k => panic!("{:?}", k),
    }
}

#[test]
fn multiline_match_arm_blocks() {
    let src = "package t\nfunc f(n: i32)\n    match n\n        1 | 2 ->\n            let a = 1\n            print(a)\n        _ -> print(0)\n    end\nend\n";
    let file = parse_source(src).unwrap();
    if let DeclKind::Func(f) = &file.decls[0].kind {
        if let StmtKind::Expr(e) = &f.body[0].kind {
            if let ExprKind::Match { arms, .. } = &e.kind {
                assert_eq!(arms.len(), 2);
                assert_eq!(arms[0].pattern.len(), 2);
                assert_eq!(arms[0].body.len(), 2);
                assert!(matches!(arms[1].pattern[0], PatAtom::Wildcard));
                return;
            }
        }
    }
    panic!("unexpected AST");
}

#[test]
fn keyword_method_names_after_dot() {
    assert!(parse_source("package t\nfunc f()\n    app.use(m)\n    x.new()\nend\n").is_ok());
}

fn err_at(src: &str) -> (u32, String) {
    let d = parse_source(src).unwrap_err();
    (d.line, d.msg)
}

#[test]
fn error_missing_end() {
    let (line, msg) = err_at("package t\nfunc f()\n    print(1)\n");
    assert!(msg.contains("expected 'end'"), "{}", msg);
    assert!(line >= 3);
}

#[test]
fn error_two_statements_on_one_line() {
    let (line, msg) = err_at("package t\nfunc f()\n    let a = 1 let b = 2\nend\n");
    assert_eq!(line, 3);
    assert!(msg.contains("end of line"), "{}", msg);
}

#[test]
fn error_let_without_value() {
    let (_, msg) = err_at("package t\nfunc f()\n    let a\nend\n");
    assert!(msg.contains("'let' requires"), "{}", msg);
}

#[test]
fn error_chained_comparison() {
    let (_, msg) = err_at("package t\nfunc f()\n    let a = 1 < 2 < 3\nend\n");
    assert!(msg.contains("chained"), "{}", msg);
}

#[test]
fn error_bad_assignment_target() {
    let (_, msg) = err_at("package t\nfunc f()\n    f() = 3\nend\n");
    assert!(msg.contains("assignment target"), "{}", msg);
}

#[test]
fn error_unterminated_text() {
    let (line, msg) = err_at("package t\nlet s = \"abc\n");
    assert_eq!(line, 2);
    assert!(msg.contains("unterminated"), "{}", msg);
}

#[test]
fn error_missing_package() {
    let (_, msg) = err_at("func f()\nend\n");
    assert!(msg.contains("package"), "{}", msg);
}

#[test]
fn error_inside_interpolation_reports_line() {
    let (line, _) = err_at("package t\nfunc f()\n\n    let s = \"a {1 +} b\"\nend\n");
    assert_eq!(line, 4);
}

// ---------- FFI: C and assembly ----------

fn first_decl(src: &str) -> Decl {
    parse_source(src).unwrap_or_else(|d| panic!("{}", d)).decls.remove(0)
}

#[test]
fn extern_c_with_abi_and_default() {
    match first_decl("package t\nextern \"stdcall\" func f(a: i32) -> i32\n").kind {
        DeclKind::Extern(e) => assert_eq!((e.abi.as_str(), e.c_variadic), ("stdcall", false)),
        k => panic!("{:?}", k),
    }
    match first_decl("package t\nextern func g()\n").kind {
        DeclKind::Extern(e) => assert_eq!(e.abi, "C"),
        k => panic!("{:?}", k),
    }
}

#[test]
fn extern_c_variadic() {
    match first_decl("package t\nextern \"C\" func printf(fmt: *c.char, ...) -> c.int\n").kind {
        DeclKind::Extern(e) => {
            assert!(e.c_variadic);
            assert_eq!(e.params.len(), 1);
        }
        k => panic!("{:?}", k),
    }
}

#[test]
fn extern_callback_type() {
    let src = "package t\nextern \"C\" func qsort(base: *c.void, cmp: extern \"C\" func(*c.void, *c.void) -> c.int)\n";
    match first_decl(src).kind {
        DeclKind::Extern(e) => match &e.params[1].ty {
            Some(Type::Func { abi, params, .. }) => {
                assert_eq!(abi.as_deref(), Some("C"));
                assert_eq!(params.len(), 2);
            }
            t => panic!("{:?}", t),
        },
        k => panic!("{:?}", k),
    }
}

#[test]
fn stacked_attributes() {
    let d = first_decl("package t\n#[naked]\n#[cfg(\"x86_64\")]\nfunc halt()\nend\n");
    assert_eq!(d.attrs.len(), 2);
    assert_eq!(d.attrs[1].name, "cfg");
}

#[test]
fn asm_block_structure() {
    let src = "package t\nfunc f()\n    unsafe\n        asm\n            \"add @r, @b\"\n            \"nop\"\n            inout r = x\n            in b = y + 1\n            out eax = z\n            clobber \"cc\", \"memory\"\n        end\n    end\nend\n";
    let file = parse_source(src).unwrap();
    let DeclKind::Func(f) = &file.decls[0].kind else { panic!() };
    let StmtKind::Unsafe(b) = &f.body[0].kind else { panic!() };
    let StmtKind::Asm(a) = &b[0].kind else { panic!() };
    assert_eq!(a.lines, vec!["add @r, @b", "nop"]);
    assert_eq!(a.operands.len(), 3);
    assert_eq!(a.operands[0].dir, AsmDir::InOut);
    assert_eq!(a.operands[1].dir, AsmDir::In);
    assert_eq!(a.operands[2].name, "eax");
    assert_eq!(a.clobbers, vec!["cc", "memory"]);
}

#[test]
fn error_asm_unterminated() {
    let (_, msg) = err_at("package t\nfunc f()\n    asm\n        \"nop\"\n");
    assert!(msg.contains("asm"), "{}", msg);
}

#[test]
fn error_asm_bad_line() {
    let (line, msg) = err_at("package t\nfunc f()\n    asm\n        nop\n    end\nend\n");
    assert_eq!(line, 4);
    assert!(msg.contains("assembly line"), "{}", msg);
}

#[test]
fn error_asm_interpolated_line() {
    let (_, msg) = err_at("package t\nfunc f()\n    asm\n        \"mov {x}\"\n    end\nend\n");
    assert!(msg.contains("plain text"), "{}", msg);
}

#[test]
fn interpreter_runs_hello() {
    let src = std::fs::read_to_string("examples/hello.br").unwrap();
    let file = parse_source(&src).unwrap();
    let mut interp = brasa::Interpreter::new();
    interp.load_file(&file).unwrap();
    interp.run_main().unwrap();
}

fn parses(src: &str) -> Result<brasa::ast::File, String> {
    brasa::parse_source(src).map_err(|d| d.msg)
}

#[test]
fn enum_holds_plain_and_data_variants() {
    let f = parses("package t
type Shape = enum
    Circle(int)
    Rect(int, int)
    Empty
end
type C = enum
    Red, Green
end
").unwrap();
    let brasa::ast::DeclKind::Type(t) = &f.decls[0].kind else { panic!() };
    let brasa::ast::TypeBody::Enum(v) = &t.body else { panic!() };
    assert_eq!(v.len(), 3);
    assert_eq!(v[1].fields.len(), 2);
    let brasa::ast::DeclKind::Type(t) = &f.decls[1].kind else { panic!() };
    let brasa::ast::TypeBody::Enum(v) = &t.body else { panic!() };
    assert_eq!(v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Red", "Green"]);
}

#[test]
fn union_keyword_is_gone() {
    assert!(parses("package t
type S = union
    A
end
").is_err());
}

#[test]
fn or_error_result_types() {
    let f = parses("package t
func a() -> int or error
    return 1
end
func b() -> error
    return
end
").unwrap();
    let brasa::ast::DeclKind::Func(a) = &f.decls[0].kind else { panic!() };
    let r = a.ret.as_ref().unwrap();
    assert!(r.fallible && r.ty.is_some());
    let brasa::ast::DeclKind::Func(b) = &f.decls[1].kind else { panic!() };
    let r = b.ret.as_ref().unwrap();
    assert!(r.fallible && r.ty.is_none());
}

#[test]
fn old_error_syntax_is_rejected_with_a_hint() {
    let e = parses("package t
func f()
    let x = g()?
end
").unwrap_err();
    assert!(e.contains("try"), "{}", e);
}

#[test]
fn lambdas_have_no_arrow() {
    assert!(parses("package t
func f()
    let g = func(x) x * 2 end
end
").is_ok());
    let e = parses("package t
func f()
    let g = func(x) -> x * 2 end
end
").unwrap_err();
    assert!(e.contains("lambdas do not use '->'"), "{}", e);
}

#[test]
fn async_functions_and_await_parse() {
    let f = parses("package t
async func f(n: int) -> int or error
    return n
end
func main() -> error
    let a = f(1)
    let b = try await a
    return
end
").unwrap();
    let brasa::ast::DeclKind::Func(a) = &f.decls[0].kind else { panic!() };
    assert!(a.is_async);
    let brasa::ast::DeclKind::Func(m) = &f.decls[1].kind else { panic!() };
    assert!(!m.is_async);
}

#[test]
fn await_binds_like_try() {
    assert_eq!(shape(&expr("await f(a)")), "(await (call f [a]))");
    assert_eq!(shape(&expr("try await f(a)")), "(? (await (call f [a])))");
    assert_eq!(shape(&expr("await a + await b")), "(+ (await a) (await b))");
    assert_eq!(shape(&expr("f(await g())")), "(call f [(await (call g []))])");
}

#[test]
fn async_lambdas_and_bare_async_are_rejected() {
    let e = parses("package t
func f()
    let g = async func() end
end
").unwrap_err();
    assert!(e.contains("lambdas cannot be async"), "{}", e);
    let e = parses("package t
async type X = int
").unwrap_err();
    assert!(e.contains("after 'async'"), "{}", e);
}

#[test]
fn async_and_await_are_reserved_words() {
    assert!(brasa::lexer::KEYWORDS.contains(&"async"));
    assert!(brasa::lexer::KEYWORDS.contains(&"await"));
    assert!(parses("package t
func f()
    let async = 1
end
").is_err());
}
