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
        ExprKind::Call { callee, args, .. } => {
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
fn error_asm_bad_clobber() {
    let (line, msg) = err_at("package t\nfunc f()\n    asm\n        nop\n        clobber 5\n    end\nend\n");
    assert_eq!(line, 5);
    assert!(msg.contains("clobbered register"), "{}", msg);
}

#[test]
fn asm_lines_are_written_without_quotes() {
    let src = "package t\nfunc f()\n    asm\n        // a Brasa comment\n        start:\n            in al, dx\n            out dx, al\n            or al, 2\n            mov ax, [bx] ; load\n\n            msg: db \"http://x\", 13, 0   // trailing\n        end:\n            \"quoted\"\n            in rdx = port\n            out rax = v\n            clobber cc, memory\n    end\nend\n";
    let file = parse_source(src).unwrap();
    let DeclKind::Func(f) = &file.decls[0].kind else { panic!() };
    let StmtKind::Asm(a) = &f.body[0].kind else { panic!() };
    assert_eq!(
        a.lines,
        vec!["start:", "in al, dx", "out dx, al", "or al, 2", "mov ax, [bx] ; load", "msg: db \"http://x\", 13, 0", "end:", "quoted"]
    );
    assert_eq!(a.operands.len(), 2);
    assert_eq!(a.operands[0].dir, AsmDir::In);
    assert_eq!(a.operands[0].name, "rdx");
    assert_eq!(a.operands[1].dir, AsmDir::Out);
    assert_eq!(a.clobbers, vec!["cc", "memory"]);
    // statements after the block are read normally again
    let src = "package t\nfunc f()\n    asm\n        cli\n        hlt\n    end\n    let x = 1\nend\n";
    let file = parse_source(src).unwrap();
    let DeclKind::Func(f) = &file.decls[0].kind else { panic!() };
    assert_eq!(f.body.len(), 2);
    let StmtKind::Asm(a) = &f.body[0].kind else { panic!() };
    assert_eq!(a.lines, vec!["cli", "hlt"]);
}

#[test]
fn error_asm_unterminated_raw_block() {
    let (_, msg) = err_at("package t\nfunc f()\n    asm\n        nop\n");
    assert!(msg.contains("asm"), "{}", msg);
}

#[test]
fn error_asm_interpolated_line() {
    let (_, msg) = err_at("package t\nfunc f()\n    asm\n        \"mov {x}\"\n    end\nend\n");
    assert!(msg.contains("plain text"), "{}", msg);
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

// ---- comptime, attributes and block forms (spec comptime-reflection) ----

fn func_of(src: &str, name: &str) -> FuncDecl {
    let file = parse_source(src).unwrap_or_else(|d| panic!("{}", d));
    file.decls
        .iter()
        .find_map(|d| match &d.kind {
            DeclKind::Func(f) if f.name == name => Some(f.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no func {}", name))
}

#[test]
fn comptime_func_and_comptime_parameters_parse() {
    let f = func_of("package t\ncomptime func gen(t: Type) -> text\n    return \"\"\nend\n", "gen");
    assert!(f.comptime && !f.params[0].comptime);
    let g = func_of("package t\n#[generated(gen)]\nfunc parse(comptime t: Type, raw: text) -> t\n", "parse");
    assert!(!g.comptime && g.params[0].comptime && !g.params[1].comptime && g.body.is_empty());
    assert!(brasa::lexer::KEYWORDS.contains(&"comptime"));
}

#[test]
fn comptime_only_applies_to_functions() {
    let e = parse_source("package t\ncomptime type X = int\n").unwrap_err().to_string();
    assert!(e.contains("'comptime' applies only to functions"), "{}", e);
    let e = parse_source("package t\nfunc f()\n    let g = func(comptime x: int) end\nend\n").unwrap_err().to_string();
    assert!(e.contains("only allowed in function declarations"), "{}", e);
}

#[test]
fn derive_generated_and_block_attributes_parse() {
    let file = parse_source(
        "package t\n#[derive(describe, json.json)]\ntype P = record\n    x: int\nend\n#[derive]\ncomptime func describe(t: Type) -> text\n    return \"\"\nend\n#[block]\ncomptime func shout(raw: text) -> text\n    return raw\nend\n",
    )
    .unwrap();
    assert_eq!(file.decls[0].attrs[0].name, "derive");
    assert_eq!(file.decls[0].attrs[0].args.len(), 2);
    assert_eq!(file.decls[1].attrs[0].name, "derive");
    assert_eq!(file.decls[2].attrs[0].name, "block");
}

#[test]
fn generated_needs_a_generator_name() {
    let e = parse_source("package t\n#[generated]\nfunc parse(comptime t: Type) -> t\n").unwrap_err().to_string();
    assert!(e.contains("needs the name of a comptime function"), "{}", e);
}

fn blocks(names: &[&str]) -> std::collections::HashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_block_form_is_an_expression_or_a_statement() {
    let src = "package t\nfunc f()\n    let s = shout\n        hello { not code\n    end\n    shout\n  more\n    end\nend\n";
    let file = brasa::parser::parse_source_with(src, blocks(&["shout"])).unwrap();
    let DeclKind::Func(f) = &file.decls[0].kind else { panic!() };
    let StmtKind::Var(v) = &f.body[0].kind else { panic!("{:?}", f.body[0]) };
    match &v.value.as_ref().unwrap().kind {
        ExprKind::Block { name, raw } => {
            assert_eq!(name, "shout");
            assert_eq!(raw, "        hello { not code");
        }
        other => panic!("{:?}", other),
    }
    match &f.body[1].kind {
        StmtKind::Block { name, raw } => assert_eq!((name.as_str(), raw.as_str()), ("shout", "  more")),
        other => panic!("{:?}", other),
    }
}

#[test]
fn an_unknown_name_alone_on_a_line_is_an_ordinary_identifier() {
    let src = "package t\nfunc f()\n    let s = shout\n    shout\nend\n";
    let file = parse_source(src).unwrap();
    let DeclKind::Func(f) = &file.decls[0].kind else { panic!() };
    assert!(matches!(&f.body[1].kind, StmtKind::Expr(Expr { kind: ExprKind::Ident(_), .. })));
}

#[test]
fn block_names_come_from_the_file_and_registered_packages() {
    brasa::comptime::register_package("blockpkg_parse", "package blockpkg_parse\n#[block]\ncomptime func twice(raw: text) -> text\n    return raw\nend\n");
    let src = "package t\nuse blockpkg_parse\nfunc f()\n    let s = twice\n        a < b ? {\n    end\nend\n";
    let file = parse_source(src).unwrap();
    let DeclKind::Func(f) = &file.decls[0].kind else { panic!() };
    let StmtKind::Var(v) = &f.body[0].kind else { panic!() };
    assert!(matches!(v.value.as_ref().unwrap().kind, ExprKind::Block { .. }));
    // a package that cannot be found: the name is an ordinary identifier
    let src2 = "package t\nuse nowhere_pkg\nfunc f()\n    let s = twice\nend\n";
    assert!(parse_source(src2).is_ok());
}

#[test]
fn an_unclosed_block_is_an_error() {
    let e = brasa::parser::parse_source_with("package t\nfunc f()\n    let s = shout\n        hi\n", blocks(&["shout"])).unwrap_err().to_string();
    assert!(e.contains("not closed with 'end'"), "{}", e);
}

#[test]
fn the_grammar_has_no_html_expression() {
    let g = std::fs::read_to_string("brasa.ebnf").unwrap();
    assert!(!g.contains("html_expr"));
    assert!(g.contains("block_expr") && g.contains("comptime"));
}

#[test]
fn shared_and_spawn_thread_forms_parse() {
    let src = "package t\n\
type Job = record\n    items: shared []int\nend\n\
func run(job: shared Job, n: int)\nend\n\
func f()\n\
    let a = shared [1, 2, 3]\n\
    let b: shared []int = a\n\
    let c = shared Job{ items: a }\n\
    spawn_thread run(c, 1)\n\
    let t = spawn_thread run(c, 2)\n\
    t.join()\n\
end\n";
    let file = parse_source(src).unwrap();
    let DeclKind::Func(f) = &file.decls[2].kind else { panic!() };
    let StmtKind::Var(v) = &f.body[0].kind else { panic!() };
    assert!(matches!(v.value.as_ref().unwrap().kind, ExprKind::Shared(_)));
    let StmtKind::Var(v) = &f.body[1].kind else { panic!() };
    assert!(matches!(v.ty, Some(Type::Shared(_))));
    let StmtKind::Expr(e) = &f.body[3].kind else { panic!() };
    assert!(matches!(e.kind, ExprKind::SpawnThread(_)));
    let StmtKind::Var(v) = &f.body[4].kind else { panic!() };
    assert!(matches!(v.value.as_ref().unwrap().kind, ExprKind::SpawnThread(_)));
}

#[test]
fn spawn_thread_without_a_call_is_a_syntax_error() {
    for body in ["    spawn_thread 5\n", "    let t = spawn_thread work\n"] {
        let src = format!("package t\nfunc f()\n{}end\n", body);
        let e = parse_source(&src).unwrap_err().to_string();
        assert!(e.contains("spawn_thread") && e.contains("function call"), "{}", e);
    }
}

#[test]
fn shared_attribute_name_still_parses() {
    let src = "package t\n#[shared]\ntype Cell = record\n    v: int\nend\n";
    assert!(parse_source(src).is_ok());
}
