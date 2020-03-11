
pub mod lexer;
pub mod ast;
mod grammar;

use ast::Program;
use lexer::read_tokens;
use grammar::peg_parser::program;

pub fn parse(input: &str) -> Result<Program, String> {
	let tokens = read_tokens(input)?;
	program(&tokens).map_err(|e| format!("{}", e))
}

