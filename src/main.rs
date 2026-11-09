use zila::ast::*;
use zila::cli::Mode;
use zila::lexer::Lexer;
use zila::{parse_source, Diag};
use std::process::exit;

fn usage() -> ! {
    eprintln!("usage: zila <file.zila> [-o output.exe] [--build] [--ast | --tokens | --asm | --run]");
    eprintln!("       zila run <file.zila | -> [--no-opt]      (build, run and delete; - reads the program from standard input)");
    eprintln!("       zila eval <code | -> | zila -e <code>    (run statements given inline or on standard input)");
    eprintln!("       zila --os <boot.zila> <kernel.zila> -o <disk.img>");
    eprintln!("       zila --docs");
    eprintln!("       zila --fmt | --fmt-check <file.zila>...");
    eprintln!("       zila --test <file.zila>... [--no-opt]");
    eprintln!("       zila <file.zila> --target linux -o <program>");
    eprintln!("       packages: --init [name] | --add <git-url>[@version] [--as <name>] | --fetch | --tidy | --update [name] [--offline]");
    eprintln!("       comptime: [--comptime-timeout <seconds>] [--no-comptime-cache] [--comptime-dump]");
    eprintln!("       link: <file.zila> --explain-link [-o output]   (bytes of code and data per package)");
    exit(2);
}

fn report(path: &str, src: &str, d: &Diag) {
    eprintln!("error: {}", d.msg);
    eprintln!("  --> {}:{}:{}", path, d.line, d.col);
    if let Some(text) = src.lines().nth((d.line as usize).saturating_sub(1)) {
        let w = d.line.to_string().len();
        eprintln!("{:w$} |", "", w = w);
        eprintln!("{} | {}", d.line, text);
        eprintln!("{:w$} | {}^", "", " ".repeat((d.col as usize).saturating_sub(1)), w = w);
    }
}

fn summarize(f: &File) -> String {
    let (mut funcs, mut types, mut vars, mut externs, mut embeds) = (0, 0, 0, 0, 0);
    for d in &f.decls {
        match d.kind {
            DeclKind::Func(_) => funcs += 1,
            DeclKind::Type(_) => types += 1,
            DeclKind::Var(_) => vars += 1,
            DeclKind::Extern(_) => externs += 1,
            DeclKind::Embed(_) => embeds += 1,
        }
    }
    format!(
        "package {}, {} use, {} funcs, {} types, {} vars, {} externs, {} embeds",
        f.package,
        f.uses.len(),
        funcs,
        types,
        vars,
        externs,
        embeds
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match zila::cli::parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {}", e);
            usage();
        }
    };
    let mode = opts.mode;
    let out_exe = opts.out.clone();
    if mode == Mode::Docs {
        sync_docs();
        return;
    }
    if mode == Mode::Lsp {
        exit(zila::lsp::run());
    }
    if matches!(mode, Mode::Init | Mode::Add | Mode::Fetch | Mode::Tidy | Mode::Update) {
        exit(zila::fetch::run(&opts));
    }
    if let Some(secs) = opts.comptime_timeout {
        zila::comptime::set_timeout_secs(secs);
    }
    if opts.no_prune {
        zila::prune::set_enabled(false);
    }
    if opts.no_comptime_cache {
        zila::comptime::set_cache_enabled(false);
    }
    if opts.comptime_dump {
        zila::comptime::set_dump(true);
    }
    zila::warn::print_to_stderr(true);
    let path = opts.path.clone().unwrap_or_else(|| usage());
    if mode == Mode::Fmt || mode == Mode::FmtCheck {
        let mut files = vec![path];
        files.extend(opts.extra.iter().cloned());
        run_fmt(&files, mode == Mode::FmtCheck);
    }
    if mode == Mode::Test {
        let mut files = vec![path];
        files.extend(opts.extra.iter().cloned());
        exit(zila::testrun::run(&files, opts.optimize));
    }
    if mode == Mode::Os {
        build_os_image(&path, &opts.extra, out_exe);
        return;
    }
    if matches!(mode, Mode::Run | Mode::Eval) && opts.target_explicit {
        match zila::run::host_target() {
            Some(host) if host == opts.target => {}
            Some(host) => {
                eprintln!("error: --run can only run programs built for this machine ({}); use --build --target {}", host, opts.target);
                exit(1);
            }
            None => {
                eprintln!("error: --run is not supported on this platform");
                exit(1);
            }
        }
    }
    // Where the program comes from: a file, standard input (`-`), or an inline snippet (`eval`).
    let from_stdin = path == "-";
    if from_stdin && !matches!(mode, Mode::Run | Mode::Eval) {
        eprintln!("error: only `run` and `eval` read the program from standard input");
        exit(2);
    }
    let read_stdin = || {
        let mut s = String::new();
        if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut s) {
            eprintln!("cannot read standard input: {}", e);
            exit(2);
        }
        s
    };
    let (label, shown, wrapped, base) = if mode == Mode::Eval {
        let code = if from_stdin { read_stdin() } else { path.clone() };
        let w = zila::eval::wrap(&code);
        let label = if from_stdin { "<stdin>" } else { "<eval>" };
        (label.to_string(), code, Some(w), "eval.zila".to_string())
    } else if from_stdin {
        ("<stdin>".to_string(), read_stdin(), None, "stdin.zila".to_string())
    } else {
        match std::fs::read_to_string(&path) {
            Ok(s) => (path.clone(), s, None, path.clone()),
            Err(e) => {
                eprintln!("cannot read {}: {}", path, e);
                exit(2);
            }
        }
    };
    let src = match &wrapped {
        Some(w) => w.source.clone(),
        None => shown.clone(),
    };
    let mode = if mode == Mode::Eval { Mode::Run } else { mode };
    if let Err(e) = zila::package::install_for(std::path::Path::new(&base)) {
        eprintln!("error: {}", e);
        exit(1);
    }
    zila::comptime::register_siblings(std::path::Path::new(&base), &src);

    if mode == Mode::Tokens {
        match Lexer::new(&src).tokenize() {
            Ok(toks) => {
                for t in toks {
                    println!("{:>4}:{:<3} {:?}", t.line, t.col, t.tok);
                }
            }
            Err(d) => {
                report(&path, &src, &d);
                exit(1);
            }
        }
        return;
    }

    match parse_source(&src) {
        Ok(file) => {
            if opts.explain_link {
                let target = if opts.target == "linux" { zila::codegen_x64::Target::Linux } else { zila::codegen_x64::Target::Windows };
                match zila::codegen_x64::link_report(&file, &zila::codegen_x64::CompileOptions { optimize: opts.optimize, test_mode: false }, target) {
                    Ok(r) => print!("{}", r),
                    Err(e) => {
                        eprintln!("error: {}", e);
                        exit(1);
                    }
                }
                if mode != Mode::Build {
                    return;
                }
            }
            if mode == Mode::Ast {
                println!("{:#?}", file);
            } else if mode == Mode::Asm {
                let target = if opts.target == "linux" { zila::codegen_x64::Target::Linux } else { zila::codegen_x64::Target::Windows };
                match zila::codegen_x64::generate_asm_for(&file, &zila::codegen_x64::CompileOptions { optimize: opts.optimize, test_mode: false }, target) {
                    Ok(asm) => println!("{}", asm),
                    Err(e) => {
                        eprintln!("error: {}", e);
                        exit(1);
                    }
                }
            } else if mode == Mode::Build {
                let linux = opts.target == "linux";
                let default_out = {
                    let stem = std::path::Path::new(&path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("out");
                    if linux {
                        stem.to_string()
                    } else {
                        format!("{}.exe", stem)
                    }
                };
                let target_exe = out_exe.unwrap_or(default_out);
                let options = zila::codegen_x64::CompileOptions { optimize: opts.optimize, test_mode: false };
                println!("Compiling {} to native x86-64 binary: {}...", path, target_exe);
                let built = if linux {
                    zila::codegen_x64::compile_to_linux(&file, &target_exe, &options)
                } else {
                    zila::codegen_x64::compile_to_native_with(&file, &target_exe, &options)
                };
                match built {
                    Ok(()) => {
                        if let Ok(meta) = std::fs::metadata(&target_exe) {
                            println!("Built {} ({} bytes, native x86-64 {} binary)", target_exe, meta.len(), if linux { "ELF" } else { "PE" });
                        } else {
                            println!("Built {}", target_exe);
                        }
                    }
                    Err(e) => {
                        eprintln!("compilation error: {}", e);
                        exit(1);
                    }
                }
            } else if mode == Mode::Run {
                let host = match zila::run::host_target() {
                    Some(h) => h,
                    None => {
                        eprintln!("error: --run is not supported on this platform");
                        exit(1);
                    }
                };
                let rewrite = |e: String| match &wrapped {
                    Some(w) => w.map_message(&e, &label),
                    None => e,
                };
                exit(zila::run::run_file_with(&file, opts.optimize, host, &rewrite));
            } else {
                println!("OK {}: {}", path, summarize(&file));
            }
        }
        Err(d) => {
            match &wrapped {
                Some(w) => {
                    // Name the snippet's own line and column, and show the snippet's text.
                    let (line, col) = w.locate(d.line as usize, d.col as usize).unwrap_or((1, 1));
                    report(&label, &shown, &Diag { msg: d.msg.clone(), line: line as _, col: col as _ });
                }
                None => report(&label, &src, &d),
            }
            exit(1);
        }
    }
}

fn build_os_image(boot_path: &str, extra: &[String], out: Option<String>) {
    if extra.len() != 1 {
        usage();
    }
    let kernel_path = &extra[0];
    let load = |p: &str| -> zila::ast::File {
        let src = match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cannot read {}: {}", p, e);
                exit(2);
            }
        };
        match parse_source(&src) {
            Ok(f) => f,
            Err(d) => {
                report(p, &src, &d);
                exit(1);
            }
        }
    };
    let (boot, kernel) = (load(boot_path), load(kernel_path));
    let out = out.unwrap_or_else(|| "zila-os.img".to_string());
    match zila::codegen_x64::build_os(&boot, &kernel) {
        Ok(image) => {
            if let Err(e) = std::fs::write(&out, &image) {
                eprintln!("cannot write {}: {}", out, e);
                exit(2);
            }
            println!("Built {} ({} bytes: 1 boot sector + {} kernel sectors)", out, image.len(), image.len() / 512 - 1);
        }
        Err(e) => {
            eprintln!("error: {}", e);
            exit(1);
        }
    }
}

/// `--fmt` and `--fmt-check`: formats the files, or lists the ones that are not formatted.
fn run_fmt(files: &[String], check: bool) -> ! {
    let mut failed = false;
    for path in files {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cannot read {}: {}", path, e);
                failed = true;
                continue;
            }
        };
        let _ = zila::package::install_for(std::path::Path::new(path));
        zila::comptime::register_siblings(std::path::Path::new(path), &src);
        match zila::fmt::format_source(&src) {
            Err(e) => {
                eprintln!("{}:{}", path, e);
                failed = true;
            }
            Ok(out) if out == src => {}
            Ok(out) => {
                if check {
                    println!("not formatted: {}", path);
                    failed = true;
                } else if let Err(e) = std::fs::write(path, out) {
                    eprintln!("cannot write {}: {}", path, e);
                    failed = true;
                } else {
                    println!("formatted {}", path);
                }
            }
        }
    }
    exit(if failed { 1 } else { 0 });
}

/// `--docs`: rewrites the example blocks of the documentation from `examples/`.
fn sync_docs() {
    match zila::docs::sync(std::path::Path::new(".")) {
        Ok(report) => {
            for line in &report.changes {
                println!("{}", line);
            }
            println!("docs: {} example block(s) checked, {} updated", report.checked, report.changes.len());
        }
        Err(e) => {
            eprintln!("error: {}", e);
            exit(1);
        }
    }
}
