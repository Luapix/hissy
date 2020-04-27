#![feature(raw)]

#[macro_use]
extern crate num_enum;
extern crate unicode_xid;
extern crate peg;
extern crate smallstr;

mod serial;

pub mod parser;
pub mod compiler;
pub mod vm;

