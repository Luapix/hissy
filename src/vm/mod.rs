
use std::collections::HashMap;
use num_enum::TryFromPrimitive;
use std::ops::Deref;
use std::convert::TryFrom;
use std::{slice, iter};

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
use object::{Upvalue, UpvalueData, Closure};

#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum InstrType {
	Nop,
	Cpy, GetUp, SetUp,
	Neg, Add, Sub, Mul, Div, Mod, Pow,
	Not, Or, And,
	Eq, Neq, Lth, Leq, Gth, Geq,
	Func, Call, Ret,
	Jmp, Jit, Jif,
	Log,
}


struct ReturnParams {
	add: usize,
	reg: u8,
}

struct ExecRecord {
	closure: GCRef<Closure>,
	upvalues: HashMap<u8, GCRef<Upvalue>>,
	return_params: Option<ReturnParams>,
	reg_win: (usize, usize),
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
		self.registers.splice(self.window_start.., iter::repeat(NIL).take(end.saturating_sub(self.window_start)));
		// Note: self.registers.resize(end, NIL) is more economical, but less precise
		self.window_start = start;
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
	
	pub fn get_upvalue(&self, upv: GCRef<Upvalue>) -> Value {
		match upv.get() {
			UpvalueData::OnStack(idx) => self.registers[idx].clone(),
			UpvalueData::OnHeap(val) => val,
		}
	}
	
	pub fn set_upvalue(&mut self, upv: GCRef<Upvalue>, val: Value) {
		match upv.get() {
			UpvalueData::OnStack(idx) => self.registers[idx] = val,
			UpvalueData::OnHeap(_) => upv.set_inside(val),
		}
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
		let mut vm = VMState {
			regs: Registers::new(),
			chunk_id: 0,
			chunk: program.chunks.get(0).expect("Program contains no chunks"),
			it: [].iter(),
			calls: vec![],
		};
		vm.regs.allocate(vm.chunk.nb_registers);
		vm
	}
	
	pub fn pos(&self) -> usize {
		usize::try_from(&self.chunk.code.len() - self.it.len()).unwrap()
	}
	
	pub fn call(&mut self, program: &'a Program, func: GCRef<Closure>, args_start: u8, ret_reg: Option<u8>) {
		let ret_add = self.pos();
		
		self.chunk_id = usize::from(func.chunk_id);
		self.chunk = &program.chunks[self.chunk_id];
		self.it = self.chunk.code.iter();
		
		self.regs.shift_window(u16::from(args_start));
		self.regs.registers.resize(self.regs.window_start + usize::from(self.chunk.nb_registers), NIL);
		
		self.calls.push(ExecRecord {
			closure: func,
			upvalues: HashMap::new(),
			return_params: ret_reg.map(|ret_reg| ReturnParams {
				add: ret_add,
				reg: ret_reg,
			}),
			reg_win: (self.regs.window_start, self.regs.registers.len()),
		});
	}
	
	pub fn ret(&mut self, program: &'a Program, heap: &mut GCHeap, ret_val: Value) {
		let cur_call = self.calls.pop().unwrap();
		let prev_call = self.calls.last().unwrap();
		
		for (reg, upv) in cur_call.upvalues { // Close upvalues
			let val = self.regs.reg_or_cst(self.chunk, heap, reg).clone();
			upv.set_inside(val);
		}
		self.regs.reset_window(prev_call.reg_win.0, prev_call.reg_win.1);
		
		self.chunk_id = prev_call.closure.chunk_id as usize;
		self.chunk = &program.chunks[self.chunk_id];
		let ret = cur_call.return_params.expect("No return address/register set");
		self.it = iter_from(&self.chunk.code, ret.add);
		*self.regs.mut_reg(ret.reg) = ret_val;
	}
}

pub fn run_program(heap: &mut GCHeap, program: &Program) {
	let mut vm = VMState::new(program);
	let main = heap.make_ref(Closure::new(0, String::from("<main>"), vec![]));
	vm.call(program, main, 0, None);
	
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
		// println!("({}) {}@{}", vm.calls.len(), vm.chunk_id, vm.pos());
		
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
					let chunk = program.chunks.get(chunk_id as usize).expect("Invalid chunk id");
					let cur_call = vm.calls.last_mut().unwrap();
					let upvalues = chunk.upvalues.iter().map(|upv| {
						let reg = upv.reg;
						if let Some(upv) = cur_call.upvalues.get(&reg) {
							upv.clone()
						} else {
							let idx = cur_call.reg_win.0 + (reg as usize);
							let upv = heap.make_ref(Upvalue::new(idx, upv.name.clone() + "@" + &chunk.name));
							cur_call.upvalues.insert(reg, upv.clone());
							upv
						}
					}).collect();
					*vm.regs.mut_reg(rout) = heap.make_value(Closure::new(chunk_id, chunk.name.clone(), upvalues));
				},
				InstrType::Call => {
					let func = vm.regs.reg_or_cst(vm.chunk, heap, read_u8(&mut vm.it));
					let args_start = read_u8(&mut vm.it);
					let rout = read_u8(&mut vm.it);
					let func = GCRef::<Closure>::try_from(func.clone()).expect("Cannot call value");
					
					vm.call(program, func, args_start, Some(rout));
				},
				InstrType::Ret => {
					let rin = read_u8(&mut vm.it);
					let temp = vm.regs.reg_or_cst(vm.chunk, heap, rin).clone();
					
					vm.ret(program, heap, temp);
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
				InstrType::GetUp => {
					let upv_idx = read_u8(&mut vm.it);
					let rout = read_u8(&mut vm.it);
					let upv = vm.calls.last().unwrap().closure.upvalues[upv_idx as usize].clone();
					*vm.regs.mut_reg(rout) = vm.regs.get_upvalue(upv);
				},
				InstrType::SetUp => {
					let upv_idx = read_u8(&mut vm.it);
					let rin = read_u8(&mut vm.it);
					let upv = vm.calls.last().unwrap().closure.upvalues[upv_idx as usize].clone();
					vm.regs.set_upvalue(upv, vm.regs.reg_or_cst(vm.chunk, heap, rin).clone());
				},
				#[allow(unreachable_patterns)]
				i => unimplemented!("Unimplemented instruction: {:?}", i)
			}
		} else if vm.chunk_id == 0 {
			break;
		} else { // implicit return
			vm.ret(program, heap, NIL);
		}
		
		counter += 1;
		if counter % 100 == 0 {
			heap.collect();
			// heap.inspect();
		}
	}
	
	vm.regs.free(vm.chunk.nb_registers);
	heap.collect();
}
