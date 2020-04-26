#![feature(raw)]

#[macro_use]
extern crate num_enum;
extern crate unicode_xid;
extern crate peg;
extern crate smallstr;


use std::fmt::{Debug, Display};

pub mod parser;
pub mod compiler;
pub mod vm;


pub fn format_error<T, U: Display>(r: Result<T, U>, msg: &str) -> Result<T, String> {
	r.map_err(|e| format!("{}: {}", msg, e))
}

pub fn display_result<T: Display>(r: Result<T, String>) {
	println!("{}", r.map_or_else(|m| format!("❎  {}", m), |m| format!("☑  Result: {}", m)));
}

pub fn debug_result<T: Debug>(r: Result<T, String>) {
	println!("{}", r.map_or_else(|m| format!("❎  {}", m), |m| format!("☑  Result: {:#?}", m)));
}

pub fn display_error(r: Result<(), String>) {
	if let Err(e) = r {
		println!("❎  {}", e);
	}
}
