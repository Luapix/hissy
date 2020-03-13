
use std::path::Path;
use std::convert::TryFrom;
use std::fs;

use super::{InstrType, value::Value, gc::GCHeap, serial::*};


#[derive(TryFromPrimitive)]
#[repr(u8)]
pub enum ConstantType {
	Int,
	Real,
	String,
}

pub enum ChunkConstant {
	Int(i32),
	Real(f64),
	Str(String),
}

impl ChunkConstant {
	pub fn into_value(&self, heap: &mut GCHeap) -> Value {
		match self {
			ChunkConstant::Int(i) => Value::from(*i),
			ChunkConstant::Real(r) => Value::from(*r),
			ChunkConstant::Str(s) => heap.make_value(s.clone()),
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
				ConstantType::Int => ChunkConstant::Int(read_i32(&mut it)),
				ConstantType::Real => ChunkConstant::Real(read_f64(&mut it)),
				ConstantType::String => {
					let length = read_u16(&mut it) as usize;
					let s = String::from_utf8(it.by_ref().take(length).copied().collect()).expect("Invalid UTF8 in string constant");
					ChunkConstant::Str(s)
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
				ChunkConstant::Int(i) => {
					bytes.push(ConstantType::Int as u8);
					bytes.extend(&i.to_le_bytes());
				},
				ChunkConstant::Real(r) => {
					bytes.push(ConstantType::Real as u8);
					bytes.extend(&r.to_le_bytes());
				},
				ChunkConstant::Str(s) => {
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
	
	pub fn iter(&self) -> impl Iterator<Item = &u8> {
		self.code.iter()
	}
}
