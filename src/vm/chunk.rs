
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
	pub nb_registers: u16,
	pub constants: Vec<ChunkConstant>,
	pub code: Vec<u8>,
}


impl Chunk {
	pub fn new() -> Chunk {
		Chunk { nb_registers: 0, constants: vec![], code: vec![] }
	}
	
	pub fn from_file<T: AsRef<Path>>(path: T) -> Chunk {
		let mut chunk = Chunk::new();
		let contents = fs::read(path).expect("Unable to read chunk");
		let mut it = contents.iter();
		chunk.nb_registers = read_u16(&mut it);
		let nb_constants = read_u16(&mut it);
		for _ in 0..nb_constants {
			let t = ConstantType::try_from(read_u8(&mut it)).expect("Unrecognized constant type");
			let value = match t {
				ConstantType::Nil => ChunkConstant::Nil,
				ConstantType::Bool => ChunkConstant::Bool(read_u8(&mut it) != 0),
				ConstantType::Int => ChunkConstant::Int(read_i32(&mut it)),
				ConstantType::Real => ChunkConstant::Real(read_f64(&mut it)),
				ConstantType::String => {
					let length = read_u16(&mut it) as usize;
					let s = String::from_utf8(it.by_ref().take(length).copied().collect()).expect("Invalid UTF8 in string constant");
					ChunkConstant::String(s)
				},
			};
			chunk.constants.push(value);
		}
		chunk.code.extend(&it.copied().collect::<Vec<u8>>());
		chunk
	}
	
	pub fn to_file<T: AsRef<Path>>(&self, path: T) -> std::io::Result<()> {
		let mut bytes = vec![];
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
		bytes.extend(&self.code);
		fs::write(path, &bytes)
	}
	
	pub fn emit_instr(&mut self, instr: InstrType) {
		self.code.push(instr as u8);
	}
	
	pub fn emit_byte(&mut self, byte: u8) {
		self.code.push(byte);
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
	
	pub fn disassemble(&self) -> String {
		let mut s = String::new();
		s += &format!("{} registers; {} constants\n", self.nb_registers, self.constants.len());
		s += "[pos]\t[instr]\n";
		
		let mut it = self.code.iter();
		let mut pos = 0;
		while let Some(b) = it.next() {
			let instr = InstrType::try_from(*b).unwrap();
			s += &format!("{}\t{:?}(", pos, instr);
			match instr {
				Nop => {},
				Log => {
					s += &format!("{}", self.format_reg(&mut it));
				},
				Cpy | Neg | Not => {
					s += &format!("{}, {}", self.format_reg(&mut it), self.format_reg(&mut it));
				},
				Add | Sub | Mul | Div | Mod | Pow | Or | And
					| Eq | Neq | Lth | Leq | Gth | Geq => {
					s += &format!("{}, {}, {}", self.format_reg(&mut it), self.format_reg(&mut it), self.format_reg(&mut it));
				},
				Jmp => {
					s += &format!("{}", i8::from_le_bytes([*it.next().unwrap()]));
				},
				Jit | Jif => {
					s += &format!("{}, {}", i8::from_le_bytes([*it.next().unwrap()]), self.format_reg(&mut it));
				},
			}
			s += ")\n";
			pos = self.code.len() - it.len();
		}
		s += &format!("{}\n", pos);
		
		s
	}
}
