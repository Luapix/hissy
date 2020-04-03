
use std::path::Path;
use std::convert::TryFrom;
use std::fs;
use std::slice;

use super::{MAX_REGISTERS, InstrType, InstrType::*, value::{NIL, TRUE, FALSE, Value}, gc::GCHeap, serial::*};


#[derive(TryFromPrimitive)]
#[repr(u8)]
pub enum ConstantType {
	Nil,
	Bool,
	Int,
	Real,
	String,
}

#[derive(PartialEq)]
pub enum ChunkConstant {
	Nil,
	Bool(bool),
	Int(i32),
	Real(f64),
	String(String),
}

impl ChunkConstant {
	pub fn into_value(&self, heap: &mut GCHeap) -> Value {
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


pub struct Chunk {
	pub name: String,
	pub nb_registers: u16,
	pub constants: Vec<ChunkConstant>,
	pub code: Vec<u8>,
}


impl Chunk {
	pub fn new(name: String) -> Chunk {
		Chunk { name: name, nb_registers: 0, constants: vec![], code: vec![] }
	}
	
	pub fn from_bytes(it: &mut slice::Iter<u8>) -> Chunk {
		let mut chunk = Chunk::new(read_small_str(it));
		
		chunk.nb_registers = read_u16(it);
		let nb_constants = read_u16(it);
		for _ in 0..nb_constants {
			let t = ConstantType::try_from(read_u8(it)).expect("Unrecognized constant type");
			let value = match t {
				ConstantType::Nil => ChunkConstant::Nil,
				ConstantType::Bool => ChunkConstant::Bool(read_u8(it) != 0),
				ConstantType::Int => ChunkConstant::Int(read_i32(it)),
				ConstantType::Real => ChunkConstant::Real(read_f64(it)),
				ConstantType::String => ChunkConstant::String(read_str(it)),
			};
			chunk.constants.push(value);
		}
		let code_size = usize::from(read_u16(it));
		chunk.code.extend(&it.take(code_size).copied().collect::<Vec<u8>>());
		chunk
	}
	
	pub fn to_bytes(&self, bytes: &mut Vec<u8>) {
		bytes.extend(&u8::try_from(self.name.len()).unwrap().to_le_bytes());
		bytes.extend(self.name.as_bytes());
		
		bytes.extend(&self.nb_registers.to_le_bytes());
		bytes.extend(&u16::try_from(self.constants.len()).unwrap().to_le_bytes());
		for cst in &self.constants {
			match cst {
				ChunkConstant::Nil => {
					bytes.push(ConstantType::Nil as u8);
				},
				ChunkConstant::Bool(b) => {
					bytes.push(ConstantType::Bool as u8);
					bytes.push(if *b { 1 } else { 0 });
				},
				ChunkConstant::Int(i) => {
					bytes.push(ConstantType::Int as u8);
					bytes.extend(&i.to_le_bytes());
				},
				ChunkConstant::Real(r) => {
					bytes.push(ConstantType::Real as u8);
					bytes.extend(&r.to_le_bytes());
				},
				ChunkConstant::String(s) => {
					bytes.push(ConstantType::String as u8);
					bytes.extend(&u16::try_from(s.len()).unwrap().to_le_bytes());
					bytes.extend(s.as_bytes());
				},
			}
		}
		
		bytes.extend(&u16::try_from(self.code.len()).unwrap().to_le_bytes());
		bytes.extend(&self.code);
	}
	
	pub fn emit_instr(&mut self, instr: InstrType) {
		self.code.push(instr as u8);
	}
	
	pub fn emit_byte(&mut self, byte: u8) {
		self.code.push(byte);
	}
	
	// Adds constant to the list of constants in the chunk, and return the constant's register index
	pub fn compile_constant(&mut self, val: ChunkConstant) -> u8 {
		self.constants.push(val);
		let cst_idx = isize::try_from(self.constants.len() - 1).unwrap();
		let reg = 255 - cst_idx;
		u8::try_from(reg).ok().filter(|r| *r >= MAX_REGISTERS).expect("Too many constants required")
	}
	
	fn format_reg(&self, it: &mut slice::Iter<u8>) -> String {
		let reg = *it.next().unwrap();
		if reg < MAX_REGISTERS {
			format!("r{}", reg)
		} else {
			let cst = usize::try_from(255 - reg).unwrap();
			format!("{}", self.constants[cst].repr())
		}
	}
	
	fn format_rel_add(&self, it: &mut slice::Iter<u8>) -> String {
		let pos = isize::try_from(self.code.len() - it.len()).unwrap();
		let rel_add = isize::from(i8::from_le_bytes([*it.next().unwrap()]));
		format!("@{}", pos + rel_add)
	}
	
	pub fn disassemble(&self, s: &mut String) {
		s.push_str(&format!("[Chunk '{}'] ({} registers; {} constants)\n",
			self.name, self.nb_registers, self.constants.len()));
		
		let mut it = self.code.iter();
		let mut pos = 0;
		while let Some(b) = it.next() {
			let instr = InstrType::try_from(*b).unwrap();
			s.push_str(&format!("{}\t{:?}(", pos, instr));
			match instr {
				Nop => {},
				Log => {
					s.push_str(&format!("{}", self.format_reg(&mut it)));
				},
				Cpy | Neg | Not => {
					s.push_str(&format!("{}, {}", self.format_reg(&mut it), self.format_reg(&mut it)));
				},
				Add | Sub | Mul | Div | Mod | Pow | Or | And
					| Eq | Neq | Lth | Leq | Gth | Geq => {
					s.push_str(&format!("{}, {}, {}", self.format_reg(&mut it), self.format_reg(&mut it), self.format_reg(&mut it)));
				},
				Func => {
					s.push_str(&format!("{}, {}", read_u8(&mut it), self.format_reg(&mut it)));
				},
				Call => {
					s.push_str(&format!("{}, {}", self.format_reg(&mut it), self.format_reg(&mut it)));
				},
				Ret => {
					s.push_str(&format!("{}", self.format_reg(&mut it)));
				},
				Jmp => {
					s.push_str(&format!("{}", self.format_rel_add(&mut it)));
				},
				Jit | Jif => {
					s.push_str(&format!("{}, {}", self.format_rel_add(&mut it), self.format_reg(&mut it)));
				},
			}
			s.push_str(")\n");
			pos = self.code.len() - it.len();
		}
		s.push_str(&format!("{}\n", pos));
	}
}

pub struct Program {
	pub chunks: Vec<Chunk>,
}

impl Program {
	pub fn from_file<T: AsRef<Path>>(path: T) -> Program {
		let contents = fs::read(path).expect("Unable to read chunk");
		
		let mut it = contents.iter();
		let mut chunks = vec![];
		while it.len() > 0 {
			chunks.push(Chunk::from_bytes(&mut it));
		}
		
		Program { chunks: chunks }
	}
	
	pub fn to_file<T: AsRef<Path>>(&self, path: T) -> std::io::Result<()> {
		let mut bytes = vec![];
		for chunk in &self.chunks {
			chunk.to_bytes(&mut bytes);
		}
		fs::write(path, &bytes)
	}
	
	pub fn disassemble(&self) -> String {
		let mut s = String::new();
		
		for chunk in &self.chunks {
			chunk.disassemble(&mut s);
		}
		s
	}
}
