use zila::ast::*;
use zila::parse_source;
use std::fs;

#[test]
fn all_examples_parse() {
    let mut n = 0;
    for entry in fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "zila") {
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
    let src = "package t\nfunc f()\n    asm\n        // a Zila Lang comment\n        start:\n            in al, dx\n            out dx, al\n            or al, 2\n            mov ax, [bx] ; load\n\n            msg: db \"http://x\", 13, 0   // trailing\n        end:\n            \"quoted\"\n            in rdx = port\n            out rax = v\n            clobber cc, memory\n    end\nend\n";
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

fn parses(src: &str) -> Result<zila::ast::File, String> {
    zila::parse_source(src).map_err(|d| d.msg)
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
    let zila::ast::DeclKind::Type(t) = &f.decls[0].kind else { panic!() };
    let zila::ast::TypeBody::Enum(v) = &t.body else { panic!() };
    assert_eq!(v.len(), 3);
    assert_eq!(v[1].fields.len(), 2);
    let zila::ast::DeclKind::Type(t) = &f.decls[1].kind else { panic!() };
    let zila::ast::TypeBody::Enum(v) = &t.body else { panic!() };
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
    let zila::ast::DeclKind::Func(a) = &f.decls[0].kind else { panic!() };
    let r = a.ret.as_ref().unwrap();
    assert!(r.fallible && r.ty.is_some());
    let zila::ast::DeclKind::Func(b) = &f.decls[1].kind else { panic!() };
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
    let zila::ast::DeclKind::Func(a) = &f.decls[0].kind else { panic!() };
    assert!(a.is_async);
    let zila::ast::DeclKind::Func(m) = &f.decls[1].kind else { panic!() };
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
    assert!(zila::lexer::KEYWORDS.contains(&"async"));
    assert!(zila::lexer::KEYWORDS.contains(&"await"));
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
    assert!(zila::lexer::KEYWORDS.contains(&"comptime"));
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
    let file = zila::parser::parse_source_with(src, blocks(&["shout"])).unwrap();
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
    zila::comptime::register_package("blockpkg_parse", "package blockpkg_parse\n#[block]\ncomptime func twice(raw: text) -> text\n    return raw\nend\n");
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
    let e = zila::parser::parse_source_with("package t\nfunc f()\n    let s = shout\n        hi\n", blocks(&["shout"])).unwrap_err().to_string();
    assert!(e.contains("not closed with 'end'"), "{}", e);
}

#[test]
fn the_grammar_has_no_html_expression() {
    let g = std::fs::read_to_string("docs/zila.ebnf").unwrap();
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

fn parse_err(src: &str) -> String {
    match parse_source(src) {
        Ok(_) => panic!("expected a parse error for {:?}", src),
        Err(d) => d.msg,
    }
}

#[test]
fn pub_is_accepted_on_declarations_and_fields() {
    let src = "package t\n\n#[export(\"x\")]\npub func a()\nend\npub async func b()\nend\npub extern func c()\npub type R = record\n    pub x: int\n    y: int\nend\npub type E = enum\n    One\n    Two\nend\npub let k = 1\npub var v = 2\npub func (r: R) m()\nend\nfunc plain()\nend\n";
    let file = parse_source(src).unwrap_or_else(|d| panic!("{}", d));
    let flags: Vec<bool> = file.decls.iter().map(|d| d.public).collect();
    assert_eq!(flags, vec![true, true, true, true, true, true, true, true, false]);
    let DeclKind::Type(td) = &file.decls[3].kind else { panic!() };
    let TypeBody::Record(fields) = &td.body else { panic!() };
    assert!(fields[0].public && !fields[1].public);
}

#[test]
fn pub_is_reserved_but_a_member_may_be_named_pub() {
    assert!(parse_source("package t\nfunc f()\n    let pub = 1\nend\n").is_err());
    assert!(parse_source("package t\nfunc f()\n    let x = a.pub\nend\n").is_ok());
}

#[test]
fn a_misplaced_pub_is_a_parse_error_that_names_what_it_cannot_apply_to() {
    assert!(parse_err("package t\npub embed \"x\"\n").contains("'pub' cannot be applied to 'embed'"));
    assert!(parse_err("package t\ntype E = enum\n    pub A\nend\n").contains("'pub' cannot be applied to an enum variant"));
    assert!(parse_err("package t\ntype I = interface\n    pub func f(self)\nend\n").contains("interface method signature"));
    assert!(parse_err("package t\nfunc f(pub x: int)\nend\n").contains("'pub' cannot be applied to a parameter"));
    assert!(parse_err("package t\npub use x\n").contains("'pub' cannot be applied to 'use'"));
}

fn sp(s: Span) -> (u32, u32, u32, u32) {
    (s.line, s.col, s.end_line, s.end_col)
}

/// Ranges of declarations, names, statements, calls and types: lines and columns count from 1 and
/// the end is just after the last character.
#[test]
fn nodes_carry_source_ranges() {
    let src = r#"package t

pub func add(a: int, b: int) -> int
    let sum = a + b
    return sum
end

type Point = record
    x: int
end

func main()
    let r = add(1,
        2)
    let n = r.value
end
"#;
    let file = parse_source(src).unwrap();

    // `pub func add ... end`
    let add = &file.decls[0];
    assert_eq!(sp(add.span), (3, 1, 6, 4));
    let DeclKind::Func(f) = &add.kind else { panic!("not a func") };
    assert_eq!(sp(f.name_span), (3, 10, 3, 13));
    assert_eq!((sp(f.params[0].name_span), sp(f.params[1].name_span)), ((3, 14, 3, 15), (3, 22, 3, 23)));
    let Some(Type::Named { span, .. }) = &f.params[0].ty else { panic!("not a named type") };
    assert_eq!(sp(*span), (3, 17, 3, 20));

    // `let sum = a + b`
    let StmtKind::Var(v) = &f.body[0].kind else { panic!("not a let") };
    assert_eq!(sp(f.body[0].span), (4, 5, 4, 20));
    assert_eq!(sp(v.name_span), (4, 9, 4, 12));
    let value = v.value.as_ref().unwrap();
    assert_eq!(sp(value.span), (4, 15, 4, 20));
    let ExprKind::Binary { lhs, rhs, .. } = &value.kind else { panic!("not binary") };
    assert_eq!((sp(lhs.span), sp(rhs.span)), ((4, 15, 4, 16), (4, 19, 4, 20)));

    // the record type and its field
    let DeclKind::Type(t) = &file.decls[1].kind else { panic!("not a type") };
    assert_eq!(sp(t.name_span), (8, 6, 8, 11));
    let TypeBody::Record(fields) = &t.body else { panic!("not a record") };
    assert_eq!(sp(fields[0].name_span), (9, 5, 9, 6));

    // a call that goes over two lines, and a member access
    let DeclKind::Func(m) = &file.decls[2].kind else { panic!("not a func") };
    let StmtKind::Var(r) = &m.body[0].kind else { panic!("not a let") };
    let call = r.value.as_ref().unwrap();
    assert_eq!(sp(call.span), (13, 13, 14, 11));
    let ExprKind::Call { callee, .. } = &call.kind else { panic!("not a call") };
    assert_eq!(sp(callee.span), (13, 13, 13, 16));
    let StmtKind::Var(n) = &m.body[1].kind else { panic!("not a let") };
    let field = n.value.as_ref().unwrap();
    assert_eq!(sp(field.span), (15, 13, 15, 20));
    assert!(field.span.contains(15, 19) && !field.span.contains(15, 20));
}

// ---- recovery (`parse_partial`) ----

use zila::parser::parse_partial;

fn func_body_len(file: &File, i: usize) -> usize {
    match &file.decls[i].kind {
        DeclKind::Func(f) => f.body.len(),
        _ => panic!("not a func"),
    }
}

#[test]
fn a_statement_that_does_not_parse_is_skipped_and_the_rest_is_kept() {
    let p = parse_partial("package t\n\nfunc f()\n    let a = 1\n    let = 2\n    let c = 3\nend\n\nfunc g()\nend\n");
    assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
    assert_eq!(p.errors[0].line, 5);
    let file = p.file.unwrap();
    assert_eq!(file.decls.len(), 2);
    assert_eq!(func_body_len(&file, 0), 2);
}

#[test]
fn a_block_left_open_at_the_end_of_the_text_is_closed_there() {
    let p = parse_partial("package t\n\nfunc f()\n    let a = 1\n    if a == 1\n        print(\"x\")\n");
    assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
    assert!(p.errors[0].msg.contains("'end'"), "{:?}", p.errors);
    let file = p.file.unwrap();
    assert_eq!(file.decls.len(), 1);
    assert_eq!(func_body_len(&file, 0), 2, "the let and the open if are both there");
}

#[test]
fn a_broken_declaration_is_skipped_to_the_next_one() {
    let p = parse_partial("package t\n\nfunc (\n    nonsense here\n\ntype P = record\n    x: int\nend\n\nfunc ok()\nend\n");
    assert_eq!(p.errors[0].line, 3, "{:?}", p.errors);
    let file = p.file.unwrap();
    let names: Vec<String> = file
        .decls
        .iter()
        .map(|d| match &d.kind {
            DeclKind::Func(f) => f.name.clone(),
            DeclKind::Type(t) => t.name.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(names, ["P", "ok"]);
}

#[test]
fn a_text_the_lexer_cannot_finish_keeps_what_came_before() {
    let p = parse_partial("package t\n\nfunc f()\n    let a = 1\n    let s = \"never closed\nend\n");
    assert!(p.errors.iter().any(|e| e.line == 5), "{:?}", p.errors);
    let file = p.file.unwrap();
    assert_eq!(file.decls.len(), 1);
}

#[test]
fn a_correct_file_has_the_same_tree_and_no_errors() {
    let src = "package t\n\nfunc f(a: int) -> int\n    return a + 1\nend\n";
    let p = parse_partial(src);
    assert!(p.errors.is_empty());
    assert_eq!(format!("{:?}", p.file.unwrap()), format!("{:?}", parse_source(src).unwrap()));
}

/// The cut points used on the examples: a few fractions of the text and the end of some lines.
fn cuts(src: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut v: Vec<usize> = [10, 25, 40, 50, 60, 75, 90].iter().map(|pct| chars[chars.len() * pct / 100].0).collect();
    let lines: Vec<usize> = src.match_indices('\n').map(|(i, _)| i).collect();
    for k in [3usize, 7, 11] {
        if let Some(i) = lines.get(lines.len() * k / 12) {
            v.push(*i);
        }
    }
    v
}

#[test]
fn every_example_cut_anywhere_gives_a_tree_and_the_error_the_strict_parser_gives() {
    let mut tried = 0;
    for entry in fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "zila") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        for at in cuts(&src) {
            let text = &src[..at];
            let p = parse_partial(text);
            tried += 1;
            match parse_source(text) {
                Ok(_) => assert!(p.errors.is_empty() && p.file.is_some(), "{} cut at {}: {:?}", path.display(), at, p.errors),
                Err(d) => {
                    // without a `package` line there is nothing to build a tree on
                    assert!(p.file.is_some() || d.msg.contains("'package'"), "{} cut at {}: no tree", path.display(), at);
                    assert_eq!((p.errors[0].line, p.errors[0].col), (d.line, d.col), "{} cut at {}", path.display(), at);
                }
            }
        }
    }
    assert!(tried > 200, "{}", tried);
}
