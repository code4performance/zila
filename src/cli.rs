//! Command-line parsing for `brasa-parse`.
//!
//! `FLAGS` is the single list of options the tool accepts. The argument parser is
//! driven by it, and `tests/docs.rs` checks that the documentation describes every
//! entry and nothing else.

/// Every option `brasa-parse` accepts.
pub const FLAGS: &[&str] = &["--ast", "--tokens", "--asm", "--run", "--build", "-o", "--os", "--docs"];

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
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options { mode: Mode::Check, path: None, extra: Vec::new(), out: None };
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
            "--build" | "build" => o.mode = Mode::Build,
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
        if FLAGS.contains(&a) && a != "-o" {
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
    }

    #[test]
    fn unknown_options_are_rejected() {
        assert!(p(&["a.br", "--bogus"]).unwrap_err().contains("unknown option"));
        assert!(p(&["a.br", "-o"]).is_err());
    }

    #[test]
    fn every_flag_is_accepted() {
        for f in FLAGS {
            let args: Vec<&str> = if *f == "-o" { vec!["a.br", "-o", "x"] } else { vec!["a.br", f] };
            assert!(p(&args).is_ok(), "{} is listed in FLAGS but not parsed", f);
        }
    }
}
