//! Command-line parsing for `brasa-parse`.
//!
//! `FLAGS` is the single list of options the tool accepts. The argument parser is
//! driven by it, and `tests/docs.rs` checks that the documentation describes every
//! entry and nothing else.

/// Every option `brasa-parse` accepts.
pub const FLAGS: &[&str] = &["--ast", "--tokens", "--asm", "--run", "--build", "-o", "--os", "--docs", "--no-opt", "--fmt", "--fmt-check", "--test", "--target", "--lsp"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Parse and print a one-line summary.
    Check,
    Ast,
    Tokens,
    Asm,
    Run,
    Build,
    /// Build a bootable disk image from a boot file and a kernel file.
    Os,
    /// Regenerate the example blocks of the documentation site.
    Docs,
    /// Rewrite the files in the standard style.
    Fmt,
    /// List the files that are not in the standard style (exit 1 when there are any).
    FmtCheck,
    /// Build each file with a generated entry point that runs its `#[test]` functions.
    Test,
    /// Run the language server on standard input and output.
    Lsp,
}

#[derive(Debug)]
pub struct Options {
    pub mode: Mode,
    /// First positional argument (the source file, or the boot file with `--os`).
    pub path: Option<String>,
    /// Further positional arguments (the kernel file with `--os`).
    pub extra: Vec<String>,
    /// Value of `-o`.
    pub out: Option<String>,
    /// Optimizations on (the default); `--no-opt` turns them off.
    pub optimize: bool,
    /// `--target`: `windows` (the default) or `linux`.
    pub target: String,
    /// Whether `--target` was given (without it, `--run` uses the host).
    pub target_explicit: bool,
}

/// The targets `--target` accepts.
pub const TARGETS: &[&str] = &["windows", "linux"];

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options { mode: Mode::Check, path: None, extra: Vec::new(), out: None, optimize: true, target: "windows".to_string(), target_explicit: false };
    let mut explicit = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--ast" => o.mode = Mode::Ast,
            "--tokens" => o.mode = Mode::Tokens,
            "--asm" => o.mode = Mode::Asm,
            "--run" => o.mode = Mode::Run,
            "--os" => o.mode = Mode::Os,
            "--docs" => o.mode = Mode::Docs,
            "--fmt" => o.mode = Mode::Fmt,
            "--fmt-check" => o.mode = Mode::FmtCheck,
            "--test" => o.mode = Mode::Test,
            "--lsp" => o.mode = Mode::Lsp,
            "--no-opt" => o.optimize = false,
            "--build" | "build" => o.mode = Mode::Build,
            "--target" => {
                i += 1;
                match args.get(i) {
                    Some(v) if TARGETS.contains(&v.as_str()) => {
                        o.target = v.clone();
                        o.target_explicit = true;
                    }
                    Some(v) => return Err(format!("unknown target '{}' (valid targets: {})", v, TARGETS.join(", "))),
                    None => return Err(format!("--target needs a name ({})", TARGETS.join(", "))),
                }
            }
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(v) => o.out = Some(v.clone()),
                    None => return Err("-o needs a file name".into()),
                }
                // `-o` alone means "build"
                if !explicit && o.mode == Mode::Check {
                    o.mode = Mode::Build;
                }
            }
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option '{}'", s)),
            s => {
                if o.path.is_none() {
                    o.path = Some(s.to_string());
                } else {
                    o.extra.push(s.to_string());
                }
            }
        }
        if FLAGS.contains(&a) && a != "-o" && a != "--no-opt" && a != "--target" {
            explicit = true;
        }
        i += 1;
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Options, String> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn modes_and_defaults() {
        assert_eq!(p(&["a.br"]).unwrap().mode, Mode::Check);
        assert_eq!(p(&["a.br", "--ast"]).unwrap().mode, Mode::Ast);
        assert_eq!(p(&["a.br", "-o", "a.exe"]).unwrap().mode, Mode::Build);
        assert_eq!(p(&["a.br", "--asm", "-o", "x"]).unwrap().mode, Mode::Asm);
        let os = p(&["--os", "boot.br", "kernel.br", "-o", "d.img"]).unwrap();
        assert_eq!((os.mode, os.path.as_deref(), os.extra.len()), (Mode::Os, Some("boot.br"), 1));
        assert_eq!(p(&["--docs"]).unwrap().mode, Mode::Docs);
        assert!(p(&["a.br"]).unwrap().optimize);
        let n = p(&["a.br", "--no-opt", "-o", "a.exe"]).unwrap();
        assert!(!n.optimize && n.mode == Mode::Build);
    }

    #[test]
    fn target_is_explicit_only_when_given() {
        let d = p(&["a.br", "--run"]).unwrap();
        assert!(!d.target_explicit && d.target == "windows");
        let l = p(&["a.br", "--run", "--target", "linux"]).unwrap();
        assert!(l.target_explicit && l.target == "linux");
    }

    #[test]
    fn unknown_options_are_rejected() {
        assert!(p(&["a.br", "--bogus"]).unwrap_err().contains("unknown option"));
        assert!(p(&["a.br", "-o"]).is_err());
    }

    #[test]
    fn every_flag_is_accepted() {
        for f in FLAGS {
            let args: Vec<&str> = match *f {
                "-o" => vec!["a.br", "-o", "x"],
                "--target" => vec!["a.br", "--target", "linux"],
                _ => vec!["a.br", f],
            };
            assert!(p(&args).is_ok(), "{} is listed in FLAGS but not parsed", f);
        }
    }
}
