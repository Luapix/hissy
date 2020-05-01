//! `hissy` is a WIP compiler and virtual machine for the Hissy programming language.

#![feature(raw)]

#[macro_use]
extern crate num_enum;
extern crate unicode_xid;
extern crate peg;
extern crate smallstr;

mod serial;

/// Lexing and parsing of Hissy code.
pub mod parser;
/// Compilation of Hissy code into bytecode.
pub mod compiler;
pub mod vm;


use std::fmt;
use std::error::Error;

#[derive(Debug)]
pub enum ErrorType {
	Syntax,
	Compilation,
	Execution,
	IO,
}

#[derive(Debug)]
pub struct HissyError(pub ErrorType, pub String, pub u16);

const RED: &str = "\u{001b}[31;1m";
const RESET: &str = "\u{001b}[0m";

impl fmt::Display for HissyError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", RED)?;
		let HissyError(ty, s, line) = self;
		let line_str = if *line != 0 { format!(" at line {}", line) } else { String::new() };
		write!(f, "{:?} error{}:{} {}", ty, line_str, RESET, s)
	}
}

impl Error for HissyError {}

