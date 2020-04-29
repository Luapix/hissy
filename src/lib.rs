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
pub enum HissyError {
	Syntax(String),
	Compilation(String),
	Execution(String),
	IO(String),
}

impl fmt::Display for HissyError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "❎  ")?;
		match self {
			HissyError::Syntax(s) => write!(f, "Syntax error: {}", s),
			HissyError::Compilation(s) => write!(f, "Compilation error: {}", s),
			HissyError::Execution(s) => write!(f, "Execution error: {}", s),
			HissyError::IO(s) => write!(f, "IO error: {}", s),
		}
	}
}

impl Error for HissyError {}

