//! Brasa front end: lexer, AST and parser.
//! The grammar is defined in `brasa.ebnf`.

pub mod asm_x86;
pub mod ast;
pub mod cli;
pub mod codegen_x64;
pub mod comptime;
pub mod docs;
pub mod effects;
pub mod float_text;
pub mod fetch;
pub mod fmt;
pub mod lexer;
pub mod lsp;
pub mod noalloc;
pub mod package;
pub mod parser;
pub mod pe;
pub mod prune;
pub mod refcheck;
pub mod regalloc;
pub mod run;
pub mod sqlblock;
pub mod testrun;
pub mod warn;

pub use codegen_x64::compile_to_native;
pub use float_text::{float_text, float_text_f32};
pub use lexer::Diag;
pub use parser::parse_source;
