
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

use gc::{GCHeap, GCRef};
use value::{Value, NIL};
use serial::*;
use chunk::{Chunk, Program};
use object::Closure;

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
	chunk_id: usize,
	return_add: usize,
	return_reg: u8,
	reg_window: usize,
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
	window: usize,
}

impl Registers {
	pub fn new() -> Registers {
		Registers { registers: vec![], window: 0 }
	}
	
	pub fn shift_window(&mut self, n: u16) {
		self.window += usize::from(n);
	}
	
	pub fn reset_window(&mut self, n: usize) {
		self.window = n;
	}
	
	pub fn enter_frame(&mut self, n: u16) {
		self.registers.resize(self.registers.len() + usize::from(n), NIL);
	}
	
	pub fn leave_frame(&mut self, n: u16) {
		self.registers.resize(self.registers.len() - usize::from(n), NIL);
	}
	
	pub fn reg_or_cst(&self, chunk: &Chunk, heap: &mut GCHeap, reg: u8) -> ValueRef {
		if reg < MAX_REGISTERS {
			let reg2 = self.window + usize::from(reg);
			ValueRef::Reg(self.registers.get(reg2).expect("Invalid register"))
		} else {
			let cst = usize::try_from(255 - reg).unwrap();
			let value = chunk.constants.get(cst).expect("Invalid constant").clone();
			let temp = value.into_value(heap);
			ValueRef::Temp(temp)
		}
	}
	
	pub fn mut_reg(&mut self, reg: u8) -> &mut Value {
		let reg2 = self.window + usize::from(reg);
		self.registers.get_mut(reg2).expect("Invalid register")
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
	
	let mut chunk_id = 0;
	let mut chunk = program.chunks.get(chunk_id).expect("Program has no main chunk");
	registers.enter_frame(chunk.nb_registers);
	
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
				let chunk_id = read_u8(&mut it);
				let rout = read_u8(&mut it);
				*registers.mut_reg(rout) = heap.make_value(Closure::new(chunk_id));
			},
			InstrType::Call => {
				let func = registers.reg_or_cst(chunk, heap, read_u8(&mut it));
				let rout = read_u8(&mut it);
				let func = GCRef::<Closure>::try_from(func.clone()).expect("Cannot call value");
				
				let pos = usize::try_from(&chunk.code.len() - it.len()).unwrap();
				calls.push(ExecRecord {
					chunk_id: chunk_id,
					return_add: pos,
					return_reg: rout,
					reg_window: registers.window,
				});
				registers.shift_window(chunk.nb_registers);
				
				chunk_id = usize::from(func.chunk_id);
				chunk = &program.chunks[chunk_id];
				registers.enter_frame(chunk.nb_registers);
				it = chunk.code.iter();
			},
			InstrType::Ret => {
				let rin = read_u8(&mut it);
				let temp = registers.reg_or_cst(chunk, heap, rin).clone();
				registers.leave_frame(chunk.nb_registers);
				let rec = calls.pop().expect("Cannot return from main chunk");
				
				registers.reset_window(rec.reg_window);
				chunk_id = rec.chunk_id;
				chunk = program.chunks.get(chunk_id).expect("Return chunk doesn't exist");
				it = iter_from(&chunk.code, rec.return_add);
				
				*registers.mut_reg(rec.return_reg) = temp;
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
	
	registers.leave_frame(chunk.nb_registers);
}
