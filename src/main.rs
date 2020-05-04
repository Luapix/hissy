
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Debug};
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::env;

use hissy_lib::{HissyError, ErrorType};
use hissy_lib::parser;
use hissy_lib::parser::{lexer::{Tokens, read_tokens}, ast::ProgramAST};
use hissy_lib::compiler::{Program, Compiler};
use hissy_lib::vm::{gc::GCHeap, run_program};


fn error(s: String) -> HissyError {
	HissyError(ErrorType::IO, s, 0)
}
fn error_str(s: &str) -> HissyError {
	error(String::from(s))
}

const RED: &str = "\u{001b}[31;1m";
const GREEN: &str = "\u{001b}[32;1m";
const RESET: &str = "\u{001b}[0m";

fn display_result<T: Display>(r: Result<T, HissyError>) {
	match r {
		Ok(r) => println!("{}Success:{} {}", GREEN, RESET, r),
		Err(e) => eprintln!("{}", e),
	}
}

fn debug_result<T: Debug>(r: Result<T, HissyError>) {
	match r {
		Ok(r) => println!("{}Success:{} {:#?}", GREEN, RESET, r),
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
  hissy lex|parse <src>
  hissy compile [--strip] [-o <bytecode>] <src>
  hissy list <bytecode>
  hissy run <bytecode>
  hissy interpret <src>
  hissy --help|--version

Arguments:
  <src>        Path to a Hissy source file (usually .hsy)
  <bytecode>   Path to a Hissy bytecode file (usually .hsyc)

Options:
  --strip      Strip debug symbols from output
  -o           Specifies the path of the resulting bytecode
  --help       Print this help message
  --version    Print the version
";

struct CommandSpec {
	name: &'static str,
	takes_file: bool,
	parameters: &'static [&'static str],
	options: &'static [&'static str]
}
impl CommandSpec {
	const fn new(name: &'static str, takes_file: bool, parameters: &'static [&'static str], options: &'static [&'static str]) -> CommandSpec {
		CommandSpec { name, takes_file, parameters, options }
	}
}

const COMMANDS: &[CommandSpec] = &[
	CommandSpec::new("lex", true, &[], &[]),
	CommandSpec::new("parse", true, &[], &[]),
	CommandSpec::new("compile", true, &["-o"], &["--strip"]),
	CommandSpec::new("list", true, &[], &[]),
	CommandSpec::new("run", true, &[], &[]),
	CommandSpec::new("interpret", true, &[], &[]),
	CommandSpec::new("--version", false, &[], &[]),
	CommandSpec::new("--help", false, &[], &[]),
];

struct Command {
	name: &'static str,
	file: Option<String>,
	parameters: HashMap<&'static str, String>,
	options: HashSet<&'static str>
}


fn parse_args(mut args: env::Args) -> Result<Command, String> {
	let _hissy_path = args.next().unwrap();
	
	let cmd_name = args.next()
		.ok_or_else(|| String::from("Expected command name"))?;
	let cmd_spec = COMMANDS.iter().find(|cmd| cmd.name == cmd_name)
		.ok_or_else(|| format!("Unknown command '{}'", cmd_name))?;
	let mut cmd = Command {
		name: cmd_spec.name,
		file: None,
		parameters: HashMap::new(),
		options: HashSet::new(),
	};
	
	let mut positional = vec![];
	while let Some(part) = args.next() {
		if part.starts_with('-') {
			if let Some(opt_spec) = cmd_spec.options.iter().find(|opt| *opt == &part) {
				cmd.options.insert(opt_spec);
			} else if let Some(param_spec) = cmd_spec.parameters.iter().find(|opt| *opt == &part) {
				cmd.parameters.insert(param_spec, args.next()
					.ok_or_else(|| format!("Option '{}' expects a parameter", param_spec))?);
			} else {
				return Err(format!("Unknown option '{}' for command '{}'", part, cmd.name));
			}
		} else {
			positional.push(part);
		}
	}
	
	let exp_positional = if cmd_spec.takes_file { 1 } else { 0 };
	if positional.len() != exp_positional {
		return Err(format!("Expected exactly {} positional arguments for command '{}'", exp_positional, cmd.name));
	}
	if cmd_spec.takes_file {
		cmd.file = Some(positional.first().unwrap().clone());
	}
	
	Ok(cmd)
}

fn main() {
	let args = env::args();
	match parse_args(args) {
		Ok(cmd) => {
			match cmd.name {
				"lex" => display_result(lex(&cmd.file.unwrap())),
				"parse" => debug_result(parse(&cmd.file.unwrap())),
				"compile" => display_result(compile(&cmd.file.unwrap(), cmd.parameters.get("-o").cloned(), cmd.options.contains("-o"))),
				"list" => display_error(list(&cmd.file.unwrap())),
				"interpret" => display_error(interpret(&cmd.file.unwrap())),
				"run" => display_error(run(&cmd.file.unwrap())),
				"--version" => println!("Hissy v{}", env!("CARGO_PKG_VERSION")),
				"--help" => println!("{}", USAGE),
				_ => panic!("Unimplemented command"),
			}
		},
		Err(err) => {
			eprintln!("{}{}{}\n{}", RED, err, RESET, USAGE);
		}
	}
}
