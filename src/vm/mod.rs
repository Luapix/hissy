
use num_enum::TryFromPrimitive;
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
use chunk::Chunk;

#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum InstrType {
	Nop,
	Cpy,
	Neg, Add, Sub, Mul, Div, Mod, Pow,
	Not, Or, And,
	Eq, Neq, Lth, Leq, Gth, Geq,
	Jmp, Jit, Jif,
	Log,
}

pub struct VM<'a> {
	heap: &'a mut GCHeap,
	registers: Vec<Value>,
}

fn compute_jump<'a>(rel_jmp: isize, code: &'a Vec<u8>, it: &slice::Iter<'a, u8>) -> slice::Iter<'a, u8> {
	let pos = code.len() - it.len();
	let final_pos = usize::try_from(pos as isize + rel_jmp).expect("Jumped back too far");
	code.get(final_pos..).expect("Jumped forward too far").iter()
}

impl VM<'_> {
	pub fn new(heap: &mut GCHeap) -> VM {
		VM { heap: heap, registers: vec![] }
	}
	
	pub fn mut_reg(&mut self, reg: u8) -> &mut Value {
		self.registers.get_mut(reg as usize).expect("Invalid register")
	}

	pub fn run_chunk(&mut self, chunk: &Chunk) {
		self.registers = vec![NIL; chunk.nb_registers as usize];
		
		let mut it = chunk.code.iter();
		
		macro_rules! reg_or_cst {
			($var:ident, $reg:expr) => {
				let temp;
				let reg = $reg;
				let $var = if reg < MAX_REGISTERS {
					self.registers.get(usize::try_from(reg).unwrap()).expect("Invalid register")
				} else {
					let cst = usize::try_from(255 - reg).unwrap();
					let value = chunk.constants.get(cst).expect("Invalid constant").clone();
					temp = value.into_value(&mut self.heap);
					&temp
				};
			};
		}
		
		macro_rules! bin_op {
			($method:ident) => {{
				let (a, b, c) = (read_u8(&mut it), read_u8(&mut it), read_u8(&mut it));
				reg_or_cst!(a, a);
				reg_or_cst!(b, b);
				*self.mut_reg(c) = a.$method(b).expect(concat!("Cannot '", stringify!($method), "' these values"));
			}};
		}
		
		while let Some(b) = it.next() {
			match InstrType::try_from(*b).unwrap() {
				InstrType::Nop => (),
				InstrType::Cpy => {
					let (rin, rout) = (read_u8(&mut it), read_u8(&mut it));
					reg_or_cst!(rin, rin);
					*self.mut_reg(rout) = rin.clone();
				},
				InstrType::Neg => {
					let (rin, rout) = (read_u8(&mut it), read_u8(&mut it));
					reg_or_cst!(rin, rin);
					*self.mut_reg(rout) = rin.neg().expect("Cannot negate value");
				},
				InstrType::Add => bin_op!(add),
				InstrType::Sub => bin_op!(sub),
				InstrType::Mul => bin_op!(mul),
				InstrType::Div => bin_op!(div),
				InstrType::Pow => bin_op!(pow),
				InstrType::Mod => bin_op!(modulo),
				InstrType::Not => {
					let (rin, rout) = (read_u8(&mut it), read_u8(&mut it));
					reg_or_cst!(rin, rin);
					*self.mut_reg(rout) = rin.not().expect("Cannot apply logical NOT to value");
				},
				InstrType::Or => bin_op!(or),
				InstrType::And => bin_op!(and),
				InstrType::Eq => {
					let (a, b, c) = (read_u8(&mut it), read_u8(&mut it), read_u8(&mut it));
					reg_or_cst!(a, a);
					reg_or_cst!(b, b);
					*self.mut_reg(c) = Value::from(a.eq(b));
				},
				InstrType::Neq => {
					let (a, b, c) = (read_u8(&mut it), read_u8(&mut it), read_u8(&mut it));
					reg_or_cst!(a, a);
					reg_or_cst!(b, b);
					*self.mut_reg(c) = Value::from(!a.eq(b));
				},
				InstrType::Lth => bin_op!(lth),
				InstrType::Leq => bin_op!(leq),
				InstrType::Gth => bin_op!(gth),
				InstrType::Geq => bin_op!(geq),
				InstrType::Jmp => {
					let rel_jmp = read_i8(&mut it);
					it = compute_jump(isize::try_from(rel_jmp).unwrap(), &chunk.code, &it);
				},
				InstrType::Jit => {
					let rel_jmp = read_i8(&mut it);
					reg_or_cst!(cond_val, read_u8(&mut it));
					let cond = bool::try_from(cond_val).expect("Non-bool used in condition");
					if cond {
						it = compute_jump(isize::try_from(rel_jmp).unwrap(), &chunk.code, &it);
					}
				},
				InstrType::Jif => {
					let rel_jmp = read_i8(&mut it);
					reg_or_cst!(cond_val, read_u8(&mut it));
					let cond = bool::try_from(cond_val).expect("Non-bool used in condition");
					if !cond {
						it = compute_jump(isize::try_from(rel_jmp).unwrap(), &chunk.code, &it);
					}
				},
				InstrType::Log => {
					reg_or_cst!(v, read_u8(&mut it));
					println!("{}", v.repr());
				},
			}
		}
	}
	
	pub fn run_bytecode_file(&mut self, path: &str) {
		let chunk = Chunk::from_file(path);
		self.run_chunk(&chunk);
	}
}
