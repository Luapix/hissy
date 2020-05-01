
/// Lexing Hissy code into `Token`s.
pub mod lexer;
/// Data structures representing Hissy code.
pub mod ast;
mod grammar;


use crate::{HissyError, ErrorType};
use grammar::peg_parser;

/// Parses a string slice containing Hissy code into an Abstract Syntax Tree.
pub fn parse(input: &str) -> Result<ast::ProgramAST, HissyError> {
	let tokens = lexer::read_tokens(input)?;
	peg_parser::program(&tokens, &tokens.token_pos).map_err(|err| {
		let err_str = format!("Near {:?}, expected {}", err.location.near, err.expected);
		HissyError(ErrorType::Syntax, err_str, err.location.line)
	})
}

