
use num_enum::TryFromPrimitive;
use std::ops::Deref;
use std::convert::TryFrom;
use std::slice;

pub mod gc;
pub mod value;
pub mod op;
mod serial;
pub mod chunk;
pub mod object;

pub const MAX_REGISTERS: u8 = 128;

use gc::GCHeap;
use value::{Value, NIL};
use serial::*;
use chunk::{Chunk, Program};

#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum InstrType {
	Nop,
	Cpy,
	Neg, Add, Sub, Mul, Div, Mod, Pow,
	Not, Or, And,
	Eq, Neq, Lth, Leq, Gth, Geq,
	Func, Call, Ret,
	Jmp, Jit, Jif,
	Log,
}


struct ExecRecord {
	chunk_id: u32,
	return_add: u32,
	return_reg: u8,
}


enum ValueRef<'a> {
	Reg(&'a Value),
	Temp(Value),
}

impl<'a> Deref for ValueRef<'a> {
	type Target = Value;
	
	fn deref(&self) -> &Value {
		match self {
			ValueRef::Reg(r) => r,
			ValueRef::Temp(v) => v,
		}
	}
}


struct Registers {
	registers: Vec<Value>,
}

impl Registers {
	pub fn new() -> Registers {
		Registers { registers: vec![] }
	}
	
	pub fn add(&mut self, n: u16) {
		self.registers.resize(self.registers.len() + usize::from(n), NIL);
	}
	
	pub fn remove(&mut self, n: u16) {
		self.registers.resize(self.registers.len() - usize::from(n), NIL);
	}
	
	pub fn reg_or_cst(&self, chunk: &Chunk, heap: &mut GCHeap, reg: u8) -> ValueRef {
		if reg < MAX_REGISTERS {
			ValueRef::Reg(self.registers.get(usize::try_from(reg).unwrap()).expect("Invalid register"))
		} else {
			let cst = usize::try_from(255 - reg).unwrap();
			let value = chunk.constants.get(cst).expect("Invalid constant").clone();
			let temp = value.into_value(heap);
			ValueRef::Temp(temp)
		}
	}
	
	pub fn mut_reg(&mut self, reg: u8) -> &mut Value {
		self.registers.get_mut(reg as usize).expect("Invalid register")
	}
}


fn read_rel_add<'a>(it: &mut slice::Iter<'a, u8>, code: &'a Vec<u8>) -> usize {
	let pos = isize::try_from(code.len() - it.len()).unwrap();
	let rel_add = isize::from(read_i8(it));
	usize::try_from(pos + rel_add).expect("Jumped back too far")
}

fn iter_from<'a>(code: &'a Vec<u8>, pos: usize) -> slice::Iter<'a, u8> {
	code.get(pos..).expect("Jumped forward too far").iter()
}

pub fn run_program(heap: &mut GCHeap, program: &Program) {
	let mut registers = Registers::new();
	
	let chunk_id = 0;
	let chunk = program.chunks.get(chunk_id).expect("Program has no main chunk");
	registers.add(chunk.nb_registers);
	
	let mut it = chunk.code.iter();
	
	let mut calls: Vec<ExecRecord> = vec![];
	
	let mut counter = 0;
	
	macro_rules! bin_op {
		($method:ident) => {{
			let (a, b, c) = (read_u8(&mut it), read_u8(&mut it), read_u8(&mut it));
			let a = registers.reg_or_cst(chunk, heap, a);
			let b = registers.reg_or_cst(chunk, heap, b);
			*registers.mut_reg(c) = a.$method(&b).expect(concat!("Cannot '", stringify!($method), "' these values"));
		}};
	}
	
	while let Some(b) = it.next() {
		match InstrType::try_from(*b).unwrap() {
			InstrType::Nop => (),
			InstrType::Cpy => {
				let (rin, rout) = (read_u8(&mut it), read_u8(&mut it));
				let rin = registers.reg_or_cst(chunk, heap, rin);
				*registers.mut_reg(rout) = rin.clone();
			},
			InstrType::Neg => {
				let (rin, rout) = (read_u8(&mut it), read_u8(&mut it));
				let rin = registers.reg_or_cst(chunk, heap, rin);
				*registers.mut_reg(rout) = rin.neg().expect("Cannot negate value");
			},
			InstrType::Add => bin_op!(add),
			InstrType::Sub => bin_op!(sub),
			InstrType::Mul => bin_op!(mul),
			InstrType::Div => bin_op!(div),
			InstrType::Pow => bin_op!(pow),
			InstrType::Mod => bin_op!(modulo),
			InstrType::Not => {
				let (rin, rout) = (read_u8(&mut it), read_u8(&mut it));
				let rin = registers.reg_or_cst(chunk, heap, rin);
				*registers.mut_reg(rout) = rin.not().expect("Cannot apply logical NOT to value");
			},
			InstrType::Or => bin_op!(or),
			InstrType::And => bin_op!(and),
			InstrType::Eq => {
				let (a, b, c) = (read_u8(&mut it), read_u8(&mut it), read_u8(&mut it));
				let a = registers.reg_or_cst(chunk, heap, a);
				let b = registers.reg_or_cst(chunk, heap, b);
				*registers.mut_reg(c) = Value::from(a.eq(&b));
			},
			InstrType::Neq => {
				let (a, b, c) = (read_u8(&mut it), read_u8(&mut it), read_u8(&mut it));
				let a = registers.reg_or_cst(chunk, heap, a);
				let b = registers.reg_or_cst(chunk, heap, b);
				*registers.mut_reg(c) = Value::from(!a.eq(&b));
			},
			InstrType::Lth => bin_op!(lth),
			InstrType::Leq => bin_op!(leq),
			InstrType::Gth => bin_op!(gth),
			InstrType::Geq => bin_op!(geq),
			InstrType::Func => {
				unimplemented!();
			},
			InstrType::Call => {
				unimplemented!();
			},
			InstrType::Ret => {
				unimplemented!();
			}
			InstrType::Jmp => {
				let final_add = read_rel_add(&mut it, &chunk.code);
				it = iter_from(&chunk.code, final_add);
			},
			InstrType::Jit => {
				let final_add = read_rel_add(&mut it, &chunk.code);
				let cond_val = registers.reg_or_cst(chunk, heap, read_u8(&mut it));
				let cond = bool::try_from(cond_val.deref()).expect("Non-bool used in condition");
				if cond {
					it = iter_from(&chunk.code, final_add);
				}
			},
			InstrType::Jif => {
				let final_add = read_rel_add(&mut it, &chunk.code);
				let cond_val = registers.reg_or_cst(chunk, heap, read_u8(&mut it));
				let cond = bool::try_from(cond_val.deref()).expect("Non-bool used in condition");
				if !cond {
					it = iter_from(&chunk.code, final_add);
				}
			},
			InstrType::Log => {
				let v = registers.reg_or_cst(chunk, heap, read_u8(&mut it));
				println!("{}", v.repr());
			},
		}
		
		counter += 1;
		if counter % 100 == 0 {
			heap.collect();
		}
	}
	
	registers.remove(chunk.nb_registers);
}
