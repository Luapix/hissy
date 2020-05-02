
use std::collections::HashMap;
use std::path::Path;
use std::convert::TryFrom;
use std::fs;
use std::slice;

use crate::{HissyError, ErrorType};
use crate::vm::{MAX_REGISTERS, InstrType, InstrType::*, value::{NIL, TRUE, FALSE, Value}, gc::GCHeap};
use crate::serial::*;


fn error(s: String) -> HissyError {
	HissyError(ErrorType::IO, s, 0)
}
fn error_str(s: &str) -> HissyError {
	error(String::from(s))
}


#[derive(TryFromPrimitive)]
#[repr(u8)]
enum ConstantType {
	Nil,
	Bool,
	Int,
	Real,
	String,
}

#[derive(PartialEq)]
pub(crate) enum ChunkConstant {
	Nil,
	Bool(bool),
	Int(i32),
	Real(f64),
	String(String),
}

impl ChunkConstant {
	pub fn to_value(&self, heap: &mut GCHeap) -> Value {
		match self {
			ChunkConstant::Nil => NIL,
			ChunkConstant::Bool(b) => if *b { TRUE } else { FALSE },
			ChunkConstant::Int(i) => Value::from(*i),
			ChunkConstant::Real(r) => Value::from(*r),
			ChunkConstant::String(s) => heap.make_value(s.clone()),
		}
	}
	
	fn repr(&self) -> String {
		match self {
			ChunkConstant::Nil => String::from("nil"),
			ChunkConstant::Bool(b) => String::from(if *b { "true" } else { "false" }),
			ChunkConstant::Int(i) => format!("{}", *i),
			ChunkConstant::Real(r) => format!("{}", *r),
			ChunkConstant::String(s) => format!("{:?}", s),
		}
	}
}


#[derive(Default)]
pub(crate) struct ChunkInfo {
	pub name: String,
	pub upvalue_names: Vec<String>,
	pub line_numbers: Vec<(u16, u16)>, // (position in bytecode, line)
}

pub(crate) struct Chunk {
	pub nb_registers: u16,
	pub constants: Vec<ChunkConstant>,
	pub upvalues: Vec<u8>,
	pub code: Vec<u8>,
	pub debug_info: ChunkInfo,
}


impl Chunk {
	pub fn new() -> Chunk {
		Chunk { nb_registers: 0, constants: vec![], upvalues: vec![], code: vec![], debug_info: ChunkInfo::default() }
	}
	
	pub fn from_bytes(it: &mut slice::Iter<u8>, debug_info: bool) -> Result<Chunk, HissyError> {
		let mut chunk = Chunk::new();
		if debug_info {
			chunk.debug_info.name = read_small_str(it)?;
		}
		
		chunk.nb_registers = read_u16(it)?;
		
		let nb_constants = read_u16(it)?;
		for _ in 0..nb_constants {
			let t = ConstantType::try_from(read_u8(it)?).map_err(|_| error_str("Unrecognized constant type"))?;
			let value = match t {
				ConstantType::Nil => ChunkConstant::Nil,
				ConstantType::Bool => ChunkConstant::Bool(read_u8(it)? != 0),
				ConstantType::Int => ChunkConstant::Int(read_i32(it)?),
				ConstantType::Real => ChunkConstant::Real(read_f64(it)?),
				ConstantType::String => ChunkConstant::String(read_str(it)?),
			};
			chunk.constants.push(value);
		}
		
		let nb_upvalues = read_u16(it)?;
		for _ in 0..nb_upvalues {
			let reg = read_u8(it)?;
			if debug_info {
				chunk.debug_info.upvalue_names.push(read_small_str(it)?);
			}
			chunk.upvalues.push(reg);
		}
		
		if debug_info {
			let nb_line_numbers = read_u16(it)?;
			for _ in 0..nb_line_numbers {
				chunk.debug_info.line_numbers.push((read_u16(it)?, read_u16(it)?));
			}
		}
		
		let code_size = usize::from(read_u16(it)?);
		chunk.code.extend(&it.take(code_size).copied().collect::<Vec<u8>>());
		Ok(chunk)
	}
	
	pub fn to_bytes(&self, bytes: &mut Vec<u8>, debug_info: bool) -> Result<(), HissyError> {
		if debug_info {
			write_small_str(bytes, &self.debug_info.name);
		}
		
		write_u16(bytes, self.nb_registers);
		
		write_into_u16(bytes, self.constants.len(), error_str("Too many constants to serialize"))?;
		for cst in &self.constants {
			match cst {
				ChunkConstant::Nil => {
					write_u8(bytes, ConstantType::Nil as u8);
				},
				ChunkConstant::Bool(b) => {
					write_u8(bytes, ConstantType::Bool as u8);
					write_u8(bytes, if *b { 1 } else { 0 });
				},
				ChunkConstant::Int(i) => {
					write_u8(bytes, ConstantType::Int as u8);
					write_i32(bytes, *i);
				},
				ChunkConstant::Real(r) => {
					write_u8(bytes, ConstantType::Real as u8);
					write_f64(bytes, *r);
				},
				ChunkConstant::String(s) => {
					write_u8(bytes, ConstantType::String as u8);
					write_str(bytes, s)?;
				},
			}
		}
		
		write_into_u16(bytes, self.upvalues.len(), error_str("Too many upvalues to serialize"))?;
		for (i, upv) in self.upvalues.iter().enumerate() {
			write_u8(bytes, *upv);
			if debug_info {
				write_small_str(bytes, &self.debug_info.upvalue_names[i]);
			}
		}
		
		if debug_info {
			write_into_u16(bytes, self.debug_info.line_numbers.len(), error_str("Too many line numbers to serialize"))?;
			for (pos, line) in &self.debug_info.line_numbers {
				write_u16(bytes, *pos);
				write_u16(bytes, *line);
			}
		}
		
		write_into_u16(bytes, self.code.len(), error_str("Code too long to serialize"))?;
		bytes.extend(&self.code);
		
		Ok(())
	}
	
	pub fn emit_instr(&mut self, instr: InstrType) {
		self.code.push(instr as u8);
	}
	
	pub fn emit_byte(&mut self, byte: u8) {
		self.code.push(byte);
	}
	
	// Adds constant to the list of constants in the chunk, and return the constant's register index
	pub fn compile_constant(&mut self, val: ChunkConstant) -> Result<u8, HissyError> {
		let reg = MAX_REGISTERS as usize + self.constants.len();
		self.constants.push(val);
		u8::try_from(reg)
			.map_err(|_| HissyError(ErrorType::Compilation, String::from("Too many constants required"), 0))
	}
	
	fn format_reg(&self, it: &mut slice::Iter<u8>) -> Result<String, HissyError> {
		let reg = read_u8(it)?;
		if reg < MAX_REGISTERS {
			Ok(format!("r{}", reg))
		} else {
			let cst = usize::try_from(reg - MAX_REGISTERS).unwrap();
			Ok(self.constants[cst].repr())
		}
	}
	
	fn format_rel_add(&self, it: &mut slice::Iter<u8>) -> String {
		let pos = isize::try_from(self.code.len() - it.len()).unwrap();
		let rel_add = isize::from(i8::from_le_bytes([*it.next().unwrap()]));
		format!("@{}", pos + rel_add)
	}
}

/// A data structure representing a compiled program (ie. Hissy bytecode).
/// Can be serialized to and from a file (usually under the extension .hic, for Hissy Instruction Code).
pub struct Program {
	pub(crate) debug_info: bool,
	pub(crate) chunks: Vec<Chunk>,
}

const MAGIC_BYTES: &[u8; 4] = b"hsyc";
const FORMAT_VER: u16 = 4;

impl Program {
	/// Reads a `Program` from a bytecode file.
	pub fn from_file<T: AsRef<Path>>(path: T) -> Result<Program, HissyError> {
		let contents = fs::read(path).map_err(|_| error_str("Unable to read chunk"))?;
		
		let mut it = contents.iter();
		
		let first_bytes: [u8; 4] = read_u8s(&mut it, MAGIC_BYTES.len())?;
		if &first_bytes != MAGIC_BYTES {
			return Err(error_str("Invalid .hsyc file"));
		}
		let version = read_u16(&mut it)?;
		if version != FORMAT_VER {
			return Err(error(format!("Bytecode file format version is {}, expected {}", version, FORMAT_VER)));
		}
		
		let options = read_u8(&mut it)?;
		if options > 1 {
			return Err(error_str("Unexpected options byte in .hsyc file"));
		}
		let debug_info = options == 1;
		
		let mut chunks = vec![];
		while it.len() > 0 {
			chunks.push(Chunk::from_bytes(&mut it, debug_info)?);
		}
		
		Ok(Program { debug_info, chunks })
	}
	
	/// Serializes a `Program` object to a bytecode file.
	pub fn to_file<T: AsRef<Path>>(&self, path: T) -> Result<(), HissyError> {
		let mut bytes = vec![];
		
		bytes.extend(MAGIC_BYTES);
		write_u16(&mut bytes, FORMAT_VER);
		
		let options = if self.debug_info { 1 } else { 0 };
		bytes.push(options);
		
		for chunk in &self.chunks {
			chunk.to_bytes(&mut bytes, self.debug_info)?;
		}
		fs::write(path, &bytes).map_err(|_| error_str("Could not write file"))
	}
	
	fn format_chunk_name(&self, chunk_id: usize) -> Result<String, HissyError> {
		if self.debug_info {
			Ok(self.chunks.get(chunk_id).ok_or_else(|| error_str("Invalid chunk ID"))?.debug_info.name.clone())
		} else {
			Ok(format!("chunk{}", chunk_id))
		}
	}
	
	/// Inspects the `Program`, printing to standard output.
	/// Corresponds to the CLI's "list" output.
	pub fn disassemble(&self) -> Result<(), HissyError> {
		if !self.debug_info {
			println!("[no debug info]");
		}
		
		for (chunk_id, chunk) in self.chunks.iter().enumerate() {
			println!("{} ({} registers; {} constants)", self.format_chunk_name(chunk_id)?,
				chunk.nb_registers, chunk.constants.len());
			
			if !chunk.upvalues.is_empty() {
				print!("(upvalues: ");
				for (i,u) in chunk.upvalues.iter().enumerate() {
					let ty = if *u >= MAX_REGISTERS { "u" } else { "r" };
					if self.debug_info {
						print!("{} (", chunk.debug_info.upvalue_names[i]);
					}
					print!("{}{}", ty, u % MAX_REGISTERS);
					if self.debug_info {
						print!(")");
					}
				}
				println!(")");
			}
			
			let line_numbers = chunk.debug_info.line_numbers.iter().copied().collect::<HashMap<u16,u16>>();
			
			let mut it = chunk.code.iter();
			let mut pos = 0;
			while let Some(b) = it.next() {
				let instr = InstrType::try_from(*b).map_err(|_| error_str("Invalid instruction in bytecode"))?;
				print!("{:<5}", pos);
				if let Some(line) = u16::try_from(pos).ok().and_then(|pos| line_numbers.get(&pos)) {
					print!("l{:<5}", line);
				} else {
					print!("      ");
				}
				print!("{:?}(", instr);
				match instr {
					Nop => {},
					Cpy | Neg | Not => {
						print!("{}, {}", chunk.format_reg(&mut it)?, chunk.format_reg(&mut it)?);
					},
					Add | Sub | Mul | Div | Mod | Pow | Or | And
						| Eq | Neq | Lth | Leq | Gth | Geq => {
						print!("{}, {}, {}", chunk.format_reg(&mut it)?, chunk.format_reg(&mut it)?, chunk.format_reg(&mut it)?);
					},
					Func => {
						print!("{}, {}", self.format_chunk_name(read_u8(&mut it)? as usize)?, chunk.format_reg(&mut it)?);
					},
					Call => {
						print!("{}, {}, {}, {}", chunk.format_reg(&mut it)?, chunk.format_reg(&mut it)?, read_u8(&mut it)?, chunk.format_reg(&mut it)?);
					},
					Ret => {
						print!("{}", chunk.format_reg(&mut it)?);
					},
					Jmp => {
						print!("{}", chunk.format_rel_add(&mut it));
					},
					Jit | Jif => {
						print!("{}, {}", chunk.format_rel_add(&mut it), chunk.format_reg(&mut it)?);
					},
					GetUp | SetUp => {
						print!("u{}, {}", read_u8(&mut it)?, chunk.format_reg(&mut it)?);
					},
					GetExt => {
						print!("e{}, {}", read_u16(&mut it)?, chunk.format_reg(&mut it)?);
					},
					#[allow(unreachable_patterns)]
					_ => unimplemented!("Unimplemented disassembly for instruction: {:?}", instr)
				}
				println!(")");
				pos = chunk.code.len() - it.len();
			}
			println!("{}\n", pos);
		}
		
		Ok(())
	}
}
