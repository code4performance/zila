use brasa::ast::*;
use brasa::cli::Mode;
use brasa::lexer::Lexer;
use brasa::{parse_source, Diag};
use std::process::exit;

fn usage() -> ! {
    eprintln!("usage: brasa-parse <file.br> [-o output.exe] [--build] [--ast | --tokens | --asm | --run]");
    eprintln!("       brasa-parse --os <boot.br> <kernel.br> -o <disk.img>");
    eprintln!("       brasa-parse --docs");
    eprintln!("       brasa-parse --fmt | --fmt-check <file.br>...");
    eprintln!("       brasa-parse --test <file.br>... [--no-opt]");
    eprintln!("       brasa-parse <file.br> --target linux -o <program>");
    eprintln!("       packages: --init [name] | --add <git-url>[@version] [--as <name>] | --fetch | --tidy | --update [name] [--offline]");
    eprintln!("       comptime: [--comptime-timeout <seconds>] [--no-comptime-cache] [--comptime-dump]");
    eprintln!("       link: <file.br> --explain-link [-o output]   (bytes of code and data per package)");
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
    let opts = match brasa::cli::parse(&args) {
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
        exit(brasa::lsp::run());
    }
    if matches!(mode, Mode::Init | Mode::Add | Mode::Fetch | Mode::Tidy | Mode::Update) {
        exit(brasa::fetch::run(&opts));
    }
    if let Some(secs) = opts.comptime_timeout {
        brasa::comptime::set_timeout_secs(secs);
    }
    if opts.no_prune {
        brasa::prune::set_enabled(false);
    }
    if opts.no_comptime_cache {
        brasa::comptime::set_cache_enabled(false);
    }
    if opts.comptime_dump {
        brasa::comptime::set_dump(true);
    }
    brasa::warn::print_to_stderr(true);
    let path = opts.path.clone().unwrap_or_else(|| usage());
    if mode == Mode::Fmt || mode == Mode::FmtCheck {
        let mut files = vec![path];
        files.extend(opts.extra.iter().cloned());
        run_fmt(&files, mode == Mode::FmtCheck);
    }
    if mode == Mode::Test {
        let mut files = vec![path];
        files.extend(opts.extra.iter().cloned());
        exit(brasa::testrun::run(&files, opts.optimize));
    }
    if mode == Mode::Os {
        build_os_image(&path, &opts.extra, out_exe);
        return;
    }
    if mode == Mode::Run && opts.target_explicit {
        match brasa::run::host_target() {
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
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {}", path, e);
            exit(2);
        }
    };
    if let Err(e) = brasa::package::install_for(std::path::Path::new(&path)) {
        eprintln!("error: {}", e);
        exit(1);
    }
    brasa::comptime::register_siblings(std::path::Path::new(&path), &src);

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
                let target = if opts.target == "linux" { brasa::codegen_x64::Target::Linux } else { brasa::codegen_x64::Target::Windows };
                match brasa::codegen_x64::link_report(&file, &brasa::codegen_x64::CompileOptions { optimize: opts.optimize, test_mode: false }, target) {
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
                let target = if opts.target == "linux" { brasa::codegen_x64::Target::Linux } else { brasa::codegen_x64::Target::Windows };
                match brasa::codegen_x64::generate_asm_for(&file, &brasa::codegen_x64::CompileOptions { optimize: opts.optimize, test_mode: false }, target) {
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
                let options = brasa::codegen_x64::CompileOptions { optimize: opts.optimize, test_mode: false };
                println!("Compiling {} to native x86-64 binary: {}...", path, target_exe);
                let built = if linux {
                    brasa::codegen_x64::compile_to_linux(&file, &target_exe, &options)
                } else {
                    brasa::codegen_x64::compile_to_native_with(&file, &target_exe, &options)
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
                let host = match brasa::run::host_target() {
                    Some(h) => h,
                    None => {
                        eprintln!("error: --run is not supported on this platform");
                        exit(1);
                    }
                };
                exit(brasa::run::run_file(&file, opts.optimize, host));
            } else {
                println!("OK {}: {}", path, summarize(&file));
            }
        }
        Err(d) => {
            report(&path, &src, &d);
            exit(1);
        }
    }
}

fn build_os_image(boot_path: &str, extra: &[String], out: Option<String>) {
    if extra.len() != 1 {
        usage();
    }
    let kernel_path = &extra[0];
    let load = |p: &str| -> brasa::ast::File {
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
    let out = out.unwrap_or_else(|| "brasa-os.img".to_string());
    match brasa::codegen_x64::build_os(&boot, &kernel) {
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
        let _ = brasa::package::install_for(std::path::Path::new(path));
        brasa::comptime::register_siblings(std::path::Path::new(path), &src);
        match brasa::fmt::format_source(&src) {
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
    match brasa::docs::sync(std::path::Path::new(".")) {
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
