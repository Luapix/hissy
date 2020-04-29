extern crate docopt;

use std::fmt::{Display, Debug};
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

use docopt::Docopt;

use hissy_lib::HissyError;
use hissy_lib::parser;
use hissy_lib::parser::{lexer::{Tokens, read_tokens}, ast::ProgramAST};
use hissy_lib::compiler::{Program, Compiler};
use hissy_lib::vm::{gc::GCHeap, run_program};


fn error(s: String) -> HissyError {
	HissyError::IO(s)
}
fn error_str(s: &str) -> HissyError {
	HissyError::IO(String::from(s))
}

fn display_result<T: Display>(r: Result<T, HissyError>) {
	match r {
		Ok(r) => println!("☑  Success: {}", r),
		Err(e) => eprintln!("{}", e),
	}
}

fn debug_result<T: Debug>(r: Result<T, HissyError>) {
	match r {
		Ok(r) => println!("☑  Success: {:#?}", r),
		Err(e) => eprintln!("{}", e),
	}
}

fn display_error(r: Result<(), HissyError>) {
	if let Err(e) = r {
		eprintln!("{}", e);
	}
}


fn lex(file: &str) -> Result<Tokens, HissyError> {
	let contents = read_to_string(file).map_err(|_| error_str("Unable to open file"))?;
	read_tokens(&contents)
}

fn parse(file: &str) -> Result<ProgramAST, HissyError> {
	let contents = read_to_string(file).map_err(|_| error_str("Unable to open file"))?;
	parser::parse(&contents)
}

fn compile(input: &str, output: Option<String>, debug_info: bool) -> Result<String, HissyError> {
	let code = read_to_string(input).map_err(|_| error_str("Unable to open file"))?;
	let compiler = Compiler::new(debug_info);
	
	let program = compiler.compile_program(&code)?;
	let output = output.map_or_else(|| Path::new(input).with_extension("hsyc"), PathBuf::from);
	program.to_file(output.clone())
		.map(|_| format!("Compiled into {:?}", output))
		.map_err(|e| error(format!("Unable to write file: {}", e)))
}

fn list(file: &str) -> Result<(), HissyError> {
	let program = Program::from_file(file)?;
	program.disassemble()
}

fn interpret(file: &str) -> Result<(), HissyError> {
	let code = read_to_string(file).map_err(|_| error_str("Unable to open file"))?;
	let compiler = Compiler::new(true); // Always output debug info when interpreting
	let program = compiler.compile_program(&code)?;
	
	let mut heap = GCHeap::new();
	run_program(&mut heap, &program)?;
	Ok(())
}

fn run(file: &str) -> Result<(), HissyError> {
	let program = Program::from_file(file)?;
	
	let mut heap = GCHeap::new();
	run_program(&mut heap, &program)?;
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
		display_error(list(args.get_str("<bytecode>")));
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