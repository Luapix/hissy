
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
	reg_win_start: usize,
	reg_win_end: usize,
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
	window_start: usize,
}

impl Registers {
	pub fn new() -> Registers {
		Registers { registers: vec![], window_start: 0 }
	}
	
	pub fn shift_window(&mut self, n: u16) {
		self.window_start += usize::from(n);
	}
	
	pub fn reset_window(&mut self, start: usize, end: usize) {
		self.window_start = start;
		self.registers.resize(end, NIL);
	}
	
	pub fn allocate(&mut self, n: u16) {
		self.registers.resize(self.registers.len() + usize::from(n), NIL);
	}
	
	pub fn free(&mut self, n: u16) {
		self.registers.resize(self.registers.len() - usize::from(n), NIL);
	}
	
	pub fn reg_or_cst(&self, chunk: &Chunk, heap: &mut GCHeap, reg: u8) -> ValueRef {
		if reg < MAX_REGISTERS {
			let reg2 = self.window_start + usize::from(reg);
			ValueRef::Reg(self.registers.get(reg2).expect("Invalid register"))
		} else {
			let cst = usize::try_from(255 - reg).unwrap();
			let value = chunk.constants.get(cst).expect("Invalid constant").clone();
			let temp = value.into_value(heap);
			ValueRef::Temp(temp)
		}
	}
	
	pub fn mut_reg(&mut self, reg: u8) -> &mut Value {
		let reg2 = self.window_start + usize::from(reg);
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


pub struct VMState<'a> {
	regs: Registers,
	chunk_id: usize,
	chunk: &'a Chunk,
	it: slice::Iter<'a, u8>,
	calls: Vec<ExecRecord>,
}

impl<'a> VMState<'a> {
	pub fn new(program: &Program) -> VMState {
		let main = program.chunks.get(0).expect("Program has no main chunk");
		let mut vm = VMState {
			regs: Registers::new(),
			chunk_id: 0,
			chunk: main,
			it: main.code.iter(),
			calls: vec![],
		};
		vm.regs.allocate(vm.chunk.nb_registers);
		vm
	}
	
	pub fn pos(&self) -> usize {
		usize::try_from(&self.chunk.code.len() - self.it.len()).unwrap()
	}
	
	pub fn call(&mut self, program: &'a Program, func: &GCRef<Closure>, args_start: u8, ret_reg: u8) {
		self.calls.push(ExecRecord {
			chunk_id: self.chunk_id,
			return_add: self.pos(),
			return_reg: ret_reg,
			reg_win_start: self.regs.window_start,
			reg_win_end: self.regs.registers.len(),
		});
		self.regs.shift_window(u16::from(args_start));
		
		self.chunk_id = usize::from(func.chunk_id);
		self.chunk = &program.chunks[self.chunk_id];
		self.regs.registers.resize(self.regs.window_start + usize::from(self.chunk.nb_registers), NIL);
		self.it = self.chunk.code.iter();
	}
	
	pub fn ret(&mut self, program: &'a Program, ret_val: Value) {
		let rec = self.calls.pop().expect("Cannot return from main chunk");
		
		self.regs.reset_window(rec.reg_win_start, rec.reg_win_end);
		self.chunk_id = rec.chunk_id;
		self.chunk = &program.chunks[self.chunk_id];
		self.it = iter_from(&self.chunk.code, rec.return_add);
		
		*self.regs.mut_reg(rec.return_reg) = ret_val;
	}
}

pub fn run_program(heap: &mut GCHeap, program: &Program) {
	let mut vm = VMState::new(program);
	
	let mut counter = 0;
	
	macro_rules! bin_op {
		($method:ident) => {{
			let (a, b, c) = (read_u8(&mut vm.it), read_u8(&mut vm.it), read_u8(&mut vm.it));
			let a = vm.regs.reg_or_cst(vm.chunk, heap, a);
			let b = vm.regs.reg_or_cst(vm.chunk, heap, b);
			*vm.regs.mut_reg(c) = a.$method(&b).expect(concat!("Cannot '", stringify!($method), "' these values"));
		}};
	}
	
	loop {
		//println!("({}) {}@{}, {}/{}", vm.calls.len(), vm.chunk_id, vm.pos(), vm.regs.window_start, vm.regs.registers.len());
		
		if let Some(b) = vm.it.next() {
			match InstrType::try_from(*b).unwrap() {
				InstrType::Nop => (),
				InstrType::Cpy => {
					let (rin, rout) = (read_u8(&mut vm.it), read_u8(&mut vm.it));
					let rin = vm.regs.reg_or_cst(vm.chunk, heap, rin);
					*vm.regs.mut_reg(rout) = rin.clone();
				},
				InstrType::Neg => {
					let (rin, rout) = (read_u8(&mut vm.it), read_u8(&mut vm.it));
					let rin = vm.regs.reg_or_cst(vm.chunk, heap, rin);
					*vm.regs.mut_reg(rout) = rin.neg().expect("Cannot negate value");
				},
				InstrType::Add => bin_op!(add),
				InstrType::Sub => bin_op!(sub),
				InstrType::Mul => bin_op!(mul),
				InstrType::Div => bin_op!(div),
				InstrType::Pow => bin_op!(pow),
				InstrType::Mod => bin_op!(modulo),
				InstrType::Not => {
					let (rin, rout) = (read_u8(&mut vm.it), read_u8(&mut vm.it));
					let rin = vm.regs.reg_or_cst(vm.chunk, heap, rin);
					*vm.regs.mut_reg(rout) = rin.not().expect("Cannot apply logical NOT to value");
				},
				InstrType::Or => bin_op!(or),
				InstrType::And => bin_op!(and),
				InstrType::Eq => {
					let (a, b, c) = (read_u8(&mut vm.it), read_u8(&mut vm.it), read_u8(&mut vm.it));
					let a = vm.regs.reg_or_cst(vm.chunk, heap, a);
					let b = vm.regs.reg_or_cst(vm.chunk, heap, b);
					*vm.regs.mut_reg(c) = Value::from(a.eq(&b));
				},
				InstrType::Neq => {
					let (a, b, c) = (read_u8(&mut vm.it), read_u8(&mut vm.it), read_u8(&mut vm.it));
					let a = vm.regs.reg_or_cst(vm.chunk, heap, a);
					let b = vm.regs.reg_or_cst(vm.chunk, heap, b);
					*vm.regs.mut_reg(c) = Value::from(!a.eq(&b));
				},
				InstrType::Lth => bin_op!(lth),
				InstrType::Leq => bin_op!(leq),
				InstrType::Gth => bin_op!(gth),
				InstrType::Geq => bin_op!(geq),
				InstrType::Func => {
					let chunk_id = read_u8(&mut vm.it);
					let rout = read_u8(&mut vm.it);
					*vm.regs.mut_reg(rout) = heap.make_value(Closure::new(chunk_id));
				},
				InstrType::Call => {
					let func = vm.regs.reg_or_cst(vm.chunk, heap, read_u8(&mut vm.it));
					let args_start = read_u8(&mut vm.it);
					let rout = read_u8(&mut vm.it);
					let func = GCRef::<Closure>::try_from(func.clone()).expect("Cannot call value");
					
					vm.call(program, &func, args_start, rout);
				},
				InstrType::Ret => {
					let rin = read_u8(&mut vm.it);
					let temp = vm.regs.reg_or_cst(vm.chunk, heap, rin).clone();
					
					vm.ret(program, temp);
				}
				InstrType::Jmp => {
					let final_add = read_rel_add(&mut vm.it, &vm.chunk.code);
					vm.it = iter_from(&vm.chunk.code, final_add);
				},
				InstrType::Jit => {
					let final_add = read_rel_add(&mut vm.it, &vm.chunk.code);
					let cond_val = vm.regs.reg_or_cst(vm.chunk, heap, read_u8(&mut vm.it));
					let cond = bool::try_from(cond_val.deref()).expect("Non-bool used in condition");
					if cond {
						vm.it = iter_from(&vm.chunk.code, final_add);
					}
				},
				InstrType::Jif => {
					let final_add = read_rel_add(&mut vm.it, &vm.chunk.code);
					let cond_val = vm.regs.reg_or_cst(vm.chunk, heap, read_u8(&mut vm.it));
					let cond = bool::try_from(cond_val.deref()).expect("Non-bool used in condition");
					if !cond {
						vm.it = iter_from(&vm.chunk.code, final_add);
					}
				},
				InstrType::Log => {
					let v = vm.regs.reg_or_cst(vm.chunk, heap, read_u8(&mut vm.it));
					println!("{}", v.repr());
				},
			}
		} else if vm.chunk_id == 0 {
			break;
		} else { // implicit return
			vm.ret(program, NIL);
		}
		
		counter += 1;
		if counter % 100 == 0 {
			heap.collect();
		}
	}
	
	vm.regs.free(vm.chunk.nb_registers);
	heap.collect();
}
