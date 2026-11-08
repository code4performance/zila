//! Command-line parsing for `brasa-parse`.
//!
//! `FLAGS` is the single list of options the tool accepts. The argument parser is
//! driven by it, and `tests/docs.rs` checks that the documentation describes every
//! entry and nothing else.

/// Every option `brasa-parse` accepts.
pub const FLAGS: &[&str] = &["--ast", "--tokens", "--asm", "--run", "--build", "-o", "--os", "--docs", "--no-opt", "--no-prune", "--explain-link", "--fmt", "--fmt-check", "--test", "--target", "--lsp", "--comptime-timeout", "--no-comptime-cache", "--comptime-dump", "--init", "--add", "--as", "--fetch", "--tidy", "--update", "--offline"];

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
    /// Write `brasa.toml`.
    Init,
    /// Add a dependency to the manifest and fetch it.
    Add,
    /// Resolve the dependencies, fill the cache and write `brasa.lock`.
    Fetch,
    /// Remove unused dependencies and rewrite the lock.
    Tidy,
    /// Raise dependency minimums to the newest tags.
    Update,
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
    /// `--comptime-timeout <seconds>`: the limit of a run of comptime generators (default 5).
    pub comptime_timeout: Option<u64>,
    /// `--no-comptime-cache`: do not read or write the cache of generator results.
    pub no_comptime_cache: bool,
    /// `--no-prune`: keep declarations nothing reaches (see the spec `static-linking`).
    pub no_prune: bool,
    /// `--explain-link`: print the bytes each package contributes to the program.
    pub explain_link: bool,
    /// `--comptime-dump`: print the text every generator produced to standard error.
    pub comptime_dump: bool,
    /// `--add <git-url>[@version]`.
    pub add: Option<String>,
    /// `--as <name>`: the import name of an added dependency.
    pub as_name: Option<String>,
    /// `--offline`: package commands use the cache only.
    pub offline: bool,
}

/// The targets `--target` accepts.
pub const TARGETS: &[&str] = &["windows", "linux"];

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options { mode: Mode::Check, path: None, extra: Vec::new(), out: None, optimize: true, target: "windows".to_string(), target_explicit: false, comptime_timeout: None, no_comptime_cache: false, no_prune: false, explain_link: false, comptime_dump: false, add: None, as_name: None, offline: false };
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
            "--init" => o.mode = Mode::Init,
            "--fetch" => o.mode = Mode::Fetch,
            "--tidy" => o.mode = Mode::Tidy,
            "--update" => o.mode = Mode::Update,
            "--offline" => o.offline = true,
            "--add" => {
                i += 1;
                match args.get(i) {
                    Some(v) if !v.starts_with('-') => o.add = Some(v.clone()),
                    _ => return Err("--add needs a git URL, optionally followed by @version".into()),
                }
                o.mode = Mode::Add;
            }
            "--as" => {
                i += 1;
                match args.get(i) {
                    Some(v) if !v.starts_with('-') => o.as_name = Some(v.clone()),
                    _ => return Err("--as needs an import name".into()),
                }
            }
            "--no-opt" => o.optimize = false,
            "--no-comptime-cache" => o.no_comptime_cache = true,
            "--no-prune" => o.no_prune = true,
            "--explain-link" => o.explain_link = true,
            "--comptime-dump" => o.comptime_dump = true,
            "--comptime-timeout" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse::<u64>().ok()) {
                    Some(v) if v > 0 => o.comptime_timeout = Some(v),
                    _ => return Err("--comptime-timeout needs a number of seconds".into()),
                }
            }
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
        if FLAGS.contains(&a) && !matches!(a, "-o" | "--no-opt" | "--no-prune" | "--explain-link" | "--target" | "--comptime-timeout" | "--no-comptime-cache" | "--comptime-dump" | "--as" | "--offline") {
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
    fn comptime_options() {
        let o = p(&["a.br", "--run", "--comptime-timeout", "9", "--no-comptime-cache", "--comptime-dump"]).unwrap();
        assert!(o.mode == Mode::Run && o.comptime_timeout == Some(9) && o.no_comptime_cache && o.comptime_dump);
        let d = p(&["a.br"]).unwrap();
        assert!(d.comptime_timeout.is_none() && !d.no_comptime_cache && !d.comptime_dump);
        assert!(p(&["a.br", "--comptime-timeout", "x"]).is_err());
        assert!(p(&["a.br", "--comptime-timeout"]).is_err());
    }

    #[test]
    fn package_commands() {
        assert_eq!(p(&["--init"]).unwrap().mode, Mode::Init);
        assert_eq!(p(&["--init", "app"]).unwrap().path.as_deref(), Some("app"));
        let a = p(&["--add", "https://x.org/a/codec@1.2.0", "--as", "js", "--offline"]).unwrap();
        assert!(a.mode == Mode::Add && a.add.as_deref() == Some("https://x.org/a/codec@1.2.0") && a.as_name.as_deref() == Some("js") && a.offline);
        assert_eq!(p(&["--fetch"]).unwrap().mode, Mode::Fetch);
        assert_eq!(p(&["--tidy"]).unwrap().mode, Mode::Tidy);
        let u = p(&["--update", "codec"]).unwrap();
        assert!(u.mode == Mode::Update && u.path.as_deref() == Some("codec"));
        assert!(p(&["--add"]).is_err() && p(&["--add", "--fetch"]).is_err());
        assert!(p(&["--as"]).is_err());
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
                "--comptime-timeout" => vec!["a.br", "--comptime-timeout", "9"],
                "--add" => vec!["--add", "https://example.org/x/codec@1.0.0"],
                "--as" => vec!["--add", "https://example.org/x/codec", "--as", "js"],
                _ => vec!["a.br", f],
            };
            assert!(p(&args).is_ok(), "{} is listed in FLAGS but not parsed", f);
        }
    }
}
