
use num_enum::TryFromPrimitive;
use std::convert::TryFrom;

pub mod gc;
pub mod value;
pub mod op;
mod serial;
pub mod chunk;
pub mod object;

use gc::GCHeap;
use value::{Value, NIL, TRUE, FALSE};
use serial::{read_u8};
use chunk::Chunk;

#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum InstrType {
	Nop,
	Cst, Nil, True, False,
	Cpy,
	Neg, Add, Sub, Mul, Div, Mod, Pow,
	Not, Or, And,
	Eq, Neq, Lth, Leq, Gth, Geq,
	Log,
}

pub struct VM<'a> {
	heap: &'a mut GCHeap,
	registers: Vec<Value>,
}

impl VM<'_> {
	pub fn new(heap: &mut GCHeap) -> VM {
		VM { heap: heap, registers: vec![] }
	}
	
	pub fn mut_reg(&mut self, reg: u8) -> &mut Value {
		self.registers.get_mut(reg as usize).expect("Invalid register")
	}
	
	pub fn reg(&self, reg: u8) -> &Value {
		self.registers.get(reg as usize).expect("Invalid register")
	}

	pub fn run_chunk(&mut self, chunk: &Chunk) {
		self.registers = vec![NIL; chunk.nb_registers as usize];
		
		let mut it = chunk.iter();
		
		macro_rules! bin_op {
			($method:ident) => {{
				let (a, b, c) = (read_u8(&mut it), read_u8(&mut it), read_u8(&mut it));
				*self.mut_reg(c) = self.reg(a).$method(&*self.reg(b)).expect(concat!("Cannot '", stringify!($method), "' these values"));
			}};
		}
		
		while let Some(b) = it.next() {
			match InstrType::try_from(*b).unwrap() {
				InstrType::Nop => (),
				InstrType::Cst => {
					let cst = read_u8(&mut it);
					let reg = read_u8(&mut it);
					let value = chunk.constants.get(cst as usize).expect("Invalid constant").clone();
					*self.mut_reg(reg) = value.into_value(&mut self.heap);
				},
				InstrType::Nil => {
					*self.mut_reg(read_u8(&mut it)) = NIL;
				},
				InstrType::True => {
					*self.mut_reg(read_u8(&mut it)) = TRUE;
				},
				InstrType::False => {
					*self.mut_reg(read_u8(&mut it)) = FALSE;
				},
				InstrType::Cpy => {
					let (rin, rout) = (read_u8(&mut it), read_u8(&mut it));
					*self.mut_reg(rout) = self.reg(rin).clone();
				},
				InstrType::Neg => {
					let (rin, rout) = (read_u8(&mut it), read_u8(&mut it));
					*self.mut_reg(rout) = self.reg(rin).neg().expect("Cannot negate value");
				},
				InstrType::Add => bin_op!(add),
				InstrType::Sub => bin_op!(sub),
				InstrType::Mul => bin_op!(mul),
				InstrType::Div => bin_op!(div),
				InstrType::Pow => bin_op!(pow),
				InstrType::Mod => bin_op!(modulo),
				InstrType::Not => {
					let (rin, rout) = (read_u8(&mut it), read_u8(&mut it));
					*self.mut_reg(rout) = self.reg(rin).not().expect("Cannot apply logical NOT to value");
				},
				InstrType::Or => bin_op!(or),
				InstrType::And => bin_op!(and),
				InstrType::Eq => {
					let (a, b, c) = (read_u8(&mut it), read_u8(&mut it), read_u8(&mut it));
					*self.mut_reg(c) = Value::from(self.reg(a).eq(&*self.reg(b)));
				},
				InstrType::Neq => {
					let (a, b, c) = (read_u8(&mut it), read_u8(&mut it), read_u8(&mut it));
					*self.mut_reg(c) = Value::from(!self.reg(a).eq(&*self.reg(b)));
				},
				InstrType::Lth => bin_op!(lth),
				InstrType::Leq => bin_op!(leq),
				InstrType::Gth => bin_op!(gth),
				InstrType::Geq => bin_op!(geq),
				InstrType::Log => {
					let reg = read_u8(&mut it);
					println!("{:?}", self.registers[reg as usize]);
				},
			}
		}
	}
	
	pub fn run_bytecode_file(&mut self, path: &str) {
		let chunk = Chunk::from_file(path);
		self.run_chunk(&chunk);
	}
}
