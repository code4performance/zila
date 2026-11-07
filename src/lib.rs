//! Brasa front end: lexer, AST and parser.
//! The grammar is defined in `brasa.ebnf`.

pub mod asm_x86;
pub mod ast;
pub mod cli;
pub mod codegen_x64;
pub mod docs;
pub mod fmt;
pub mod interp;
pub mod lexer;
pub mod lsp;
pub mod parser;
pub mod regalloc;
pub mod testrun;

pub use codegen_x64::compile_to_native;
pub use interp::{float_text, float_text_f32, Interpreter};
pub use lexer::Diag;
pub use parser::parse_source;
