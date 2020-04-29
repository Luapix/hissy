extern crate docopt;

use std::fmt::{Display, Debug};
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

use docopt::Docopt;

use hissy_lib::parser;
use hissy_lib::parser::{lexer::{Tokens, read_tokens}, ast::ProgramAST};
use hissy_lib::compiler::{Program, Compiler};
use hissy_lib::vm::{gc::GCHeap, run_program};


fn format_error<T, U: Display>(r: Result<T, U>, msg: &str) -> Result<T, String> {
	r.map_err(|e| format!("{}: {}", msg, e))
}

fn display_result<T: Display>(r: Result<T, String>) {
	println!("{}", r.map_or_else(|m| format!("❎  {}", m), |m| format!("☑  Success: {}", m)));
}

fn debug_result<T: Debug>(r: Result<T, String>) {
	println!("{}", r.map_or_else(|m| format!("❎  {}", m), |m| format!("☑  Success: {:#?}", m)));
}

fn display_error(r: Result<(), String>) {
	if let Err(e) = r {
		println!("❎  {}", e);
	}
}


fn lex(file: &str) -> Result<Tokens, String> {
	let contents = format_error(read_to_string(file), "Unable to open file")?;
	format_error(read_tokens(&contents), "Lexer error")
}

fn parse(file: &str) -> Result<ProgramAST, String> {
	let contents = format_error(read_to_string(file), "Unable to open file")?;
	format_error(parser::parse(&contents), "Parse error")
}

fn compile(input: &str, output: Option<String>, debug_info: bool) -> Result<String, String> {
	let code = format_error(read_to_string(input), "Unable to open file")?;
	let compiler = Compiler::new(debug_info);
	
	let program = format_error(compiler.compile_program(&code), "Compile error")?;
	let output = output.map_or_else(|| Path::new(input).with_extension("hsyc"), |o| PathBuf::from(o));
	let res = program.to_file(output.clone());
	format_error(res.map(|()| format!("Compiled into {:?}", output)), "Compile error")
}

fn list(file: &str) {
	let program = Program::from_file(file);
	program.disassemble();
}

fn interpret(file: &str) -> Result<(), String> {
	let code = format_error(read_to_string(file), "Unable to open file")?;
	let compiler = Compiler::new(true); // Always output debug info when interpreting
	let program = format_error(compiler.compile_program(&code), "Compile error")?;
	
	let mut heap = GCHeap::new();
	{
		run_program(&mut heap, &program);
	}
	heap.collect();
	Ok(())
}

fn run(file: &str) -> Result<(), String> {
	let program = Program::from_file(file);
	
	let mut heap = GCHeap::new();
	{
		run_program(&mut heap, &program);
	}
	heap.collect();
	Ok(())
}


const USAGE: &str = "
Usage:
  hissy interpret <code>
  hissy run <bytecode>
  hissy compile [--strip] [-o <bytecode>] <code>
  hissy (lex|parse) <code>
  hissy list <bytecode>
  hissy --help
  hissy --version

Arguments:
  <code>       A Hissy source file (usually .hsy)
  <bytecode>   A Hissy bytecode file (usually .hsyc)

Options:
  --strip      Strip debug symbols from output
  -o           Specifies the path of the resulting bytecode
  --help       Print this help message
  --version    Print the version
";

fn get_arg_option(args: &docopt::ArgvMap, key: &str) -> Option<String> {
	if args.get_bool(key) {
		Some(args.get_str(key).to_string())
	} else {
		None
	}
}

fn main() {
	let args = Docopt::new(USAGE)
		.and_then(|d| d.parse())
		.unwrap_or_else(|e| e.exit());
	
	if args.get_bool("lex") {
		display_result(lex(args.get_str("<code>")));
	} else if args.get_bool("parse") {
		debug_result(parse(args.get_str("<code>")));
	} else if args.get_bool("compile") {
		display_result(compile(args.get_str("<code>"), get_arg_option(&args, "<bytecode>"), !args.get_bool("--strip")));
	} else if args.get_bool("list") {
		list(args.get_str("<bytecode>"));
	} else if args.get_bool("interpret") {
		display_error(interpret(args.get_str("<code>")));
	} else if args.get_bool("run") {
		display_error(run(args.get_str("<bytecode>")));
	} else if args.get_bool("--version") {
		println!("Hissy v{}", env!("CARGO_PKG_VERSION"));
	} else {
		panic!("Unimplemented command");
	}
}