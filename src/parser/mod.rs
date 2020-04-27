
pub mod lexer;
pub mod ast;
mod grammar;

use grammar::peg_parser;

pub fn parse(input: &str) -> Result<ast::ProgramAST, String> {
	let tokens = lexer::read_tokens(input)?;
	peg_parser::program(&tokens).map_err(|e| format!("{}", e))
}

