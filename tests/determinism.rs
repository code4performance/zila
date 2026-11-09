//! The assembly of a program is a function of the program and the flags only (spec
//! `static-linking`): compiling the same file twice gives the same text, in the same order. Hash
//! maps iterate in an order that differs from one run to the next, so any place that walks one
//! without sorting shows up here as a difference between two compiles.

use zila::codegen_x64::{generate_asm_for, CompileOptions, Target};
use zila::parse_source;

#[test]
fn compiling_a_program_twice_gives_the_same_assembly() {
    let mut compared = 0;
    let mut names = Vec::new();
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "zila") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let Ok(file) = parse_source(&src) else { continue };
        for (target, optimize) in [(Target::Windows, true), (Target::Linux, true), (Target::Windows, false)] {
            let options = CompileOptions { optimize, test_mode: false };
            let (Ok(a), Ok(b)) = (generate_asm_for(&file, &options, target), generate_asm_for(&file, &options, target)) else { continue };
            assert!(a == b, "{} ({:?}, optimize {}) compiles to different text on two runs", path.display(), target, optimize);
            compared += 1;
        }
        names.push(path.display().to_string());
    }
    assert!(compared >= 30, "only {} compilations compared (programs: {:?})", compared, names);
}
