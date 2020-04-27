
/// Lexing Hissy code into `Token`s.
pub mod lexer;
/// Data structures representing Hissy code.
pub mod ast;
mod grammar;

use grammar::peg_parser;

/// Parses a string slice containing Hissy code into an Abstract Syntax Tree.
pub fn parse(input: &str) -> Result<ast::ProgramAST, String> {
	let tokens = lexer::read_tokens(input)?;
	peg_parser::program(&tokens).map_err(|e| format!("{}", e))
}

