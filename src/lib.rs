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

