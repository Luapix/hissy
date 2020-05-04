//! Execution of Hissy programs.
//! 
//! Hissy is executed through a virtual machine, which interprets the bytecode generated by the compiler.
//!
//! # Quick overview of Hissy bytecode
//! 
//! ## Notations
//! - `rc` represents a one-byte (signed) register or constant index (non-negative → register, negative → constant)
//! - `r` represents a one-byte (unsigned) register index
//! - `a` represents a one-byte (signed) relative address within the bytecode, based on the byte containing the address
//! - `u` represents a one-byte (unsigned) upvalue index
//! - `c` represents a one-byte (unsigned) chunk index
//! 
//! ## Instructions
//! - `Nop`: No effect
//! - `Cpy(rc, r)`: Copies `rc` into `r`
//! - `GetUp(u, r)`, `SetUp(u, rc)`: Gets or sets an upvalue with a register
//! - `Neg/Not(rc, r)`: Computes `-rc`/`not rc` and storing the result in `r`
//! - `Or/And/Eq/Neq/Lth/Leq/Gth/Geq/Add/Sub/Mul/Div/Mod/Pow(rc1, rc2, r)`:
//!    
//!    Applies the corresponding binary operation to `rc1` and `rc2`, storing the result in `r`
//! - `Func(c, r)`: Creates a closure from the chunk with index `c`, storing the result in `r`
//! - `Call(r1, r2, r3)`: Calls the function in `r1`, using arguments starting at `r2`, storing the result in `r3`
//! - `Ret(rc)`: Returns `rc` from the current function
//! - `Jmp(a)`: Unconditional jump to `a`
//! - `Jit/Jif(a, rc)`: Jumps to `a` if `rc` is true/false (panics if not a boolean)
//! 

/// Garbage collector and tools for manipulating values in the GC heap.
pub mod gc;
/// Type-erased Hissy value type and constants.
pub mod value;
mod op;
mod object;
pub(crate) mod prelude;


use std::collections::HashMap;
use num_enum::TryFromPrimitive;
use std::ops::Deref;
use std::convert::TryFrom;
use std::{slice, iter};

use crate::{HissyError, ErrorType};
use crate::serial::*;
use crate::compiler::chunk::{Chunk, Program};

use gc::{GCHeap, GCRef};
use value::{Value, NIL};
use object::{Upvalue, UpvalueData, Closure, NativeFunction, List};


pub(crate) const MAX_REGISTERS: u8 = 128;


fn error(s: String) -> HissyError {
	HissyError(ErrorType::Execution, s, 0)
}
fn error_str(s: &str) -> HissyError {
	error(String::from(s))
}

#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub(crate) enum InstrType {
	Nop,
	Cpy, GetUp, SetUp, GetExt,
	Neg, Add, Sub, Mul, Div, Mod, Pow,
	Not, Or, And,
	Eq, Neq, Lth, Leq, Gth, Geq,
	Func, Call, Ret,
	ListNew, ListExtend, ListGet, ListSet,
	Jmp, Jit, Jif,
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
	
	pub fn free_all(&mut self) {
		self.registers.clear();
	}
	
	pub fn reg_or_cst(&self, chunk: &Chunk, heap: &mut GCHeap, reg: u8) -> Result<ValueRef, HissyError> {
		if reg < MAX_REGISTERS {
			let reg2 = self.window_start + (reg as usize);
			self.registers.get(reg2).ok_or_else(|| error_str("Invalid register")).map(ValueRef::Reg)
		} else {
			let cst_idx = usize::try_from(reg - MAX_REGISTERS).unwrap();
			let cst = chunk.constants.get(cst_idx).ok_or_else(|| error_str("Invalid constant"));
			cst.map(|cst| ValueRef::Temp(cst.to_value(heap)))
		}
	}
	
	pub fn mut_reg(&mut self, reg: u8) -> &mut Value {
		let reg2 = self.window_start + usize::from(reg);
		self.registers.get_mut(reg2).expect("Invalid register")
	}
	
	pub fn reg_range(&self, start: u8, cnt: u8) -> &[Value] {
		let start_abs = self.window_start + (start as usize);
		&self.registers[start_abs .. start_abs + (cnt as usize)]
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


fn read_rel_add<'a>(it: &mut slice::Iter<'a, u8>, code: &'a [u8]) -> Result<usize, HissyError> {
	let pos = isize::try_from(code.len() - it.len()).unwrap();
	let rel_add = isize::from(read_i8(it)?);
	usize::try_from(pos + rel_add).map_err(|_| error_str("Jumped back too far"))
}

fn iter_from(code: &[u8], pos: usize) -> slice::Iter<u8> {
	code.get(pos..).expect("Jumped forward too far").iter()
}


struct VMState<'a> {
	regs: Registers,
	chunk_id: usize,
	chunk: &'a Chunk,
	it: slice::Iter<'a, u8>,
	calls: Vec<ExecRecord>,
	external: Vec<Value>,
}

impl<'a> VMState<'a> {
	pub fn new(program: &Program) -> VMState {
		let mut vm = VMState {
			regs: Registers::new(),
			chunk_id: 0,
			chunk: program.chunks.get(0).expect("Program contains no chunks"),
			it: [].iter(),
			calls: vec![],
			external: vec![],
		};
		vm.regs.allocate(vm.chunk.nb_registers);
		vm
	}
	
	pub fn pos(&self) -> usize {
		usize::try_from(self.chunk.code.len() - self.it.len()).unwrap()
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
	
	pub fn ret(&mut self, program: &'a Program, heap: &mut GCHeap, ret_val: Value) -> Result<(), HissyError> {
		let cur_call = self.calls.pop().unwrap();
		let prev_call = self.calls.last().unwrap();
		
		for (reg, upv) in cur_call.upvalues { // Close upvalues
			let val = self.regs.reg_or_cst(self.chunk, heap, reg)?.clone();
			upv.set_inside(val);
		}
		self.regs.reset_window(prev_call.reg_win.0, prev_call.reg_win.1);
		
		self.chunk_id = prev_call.closure.chunk_id as usize;
		self.chunk = &program.chunks[self.chunk_id];
		let ret = cur_call.return_params.expect("No return address/register set");
		self.it = iter_from(&self.chunk.code, ret.add);
		*self.regs.mut_reg(ret.reg) = ret_val;
		
		Ok(())
	}
}

/// Runs a compiled Hissy program, using an existing GC heap.
pub fn run_program(heap: &mut GCHeap, program: &Program) -> Result<(), HissyError> {
	let mut vm = VMState::new(program);
	
	vm.external.extend(prelude::create(heap));
	
	let main = heap.make_ref(Closure::new(0, vec![]));
	vm.call(program, main, 0, None);
	
	macro_rules! bin_op {
		($method:ident) => {{
			let (a, b, c) = (read_u8(&mut vm.it)?, read_u8(&mut vm.it)?, read_u8(&mut vm.it)?);
			let a = vm.regs.reg_or_cst(vm.chunk, heap, a)?;
			let b = vm.regs.reg_or_cst(vm.chunk, heap, b)?;
			*vm.regs.mut_reg(c) = a.$method(&b)
				.ok_or_else(|| error_str(concat!("Cannot ", stringify!($method), " these values")))?;
		}};
	}
	
	loop {
		// println!("({}) {}@{}", vm.calls.len(), vm.chunk_id, vm.pos());
		
		let instr_pos = vm.pos() as u16;
		
		let mut run_instr = || -> Result<bool, HissyError> {
			if let Some(b) = vm.it.next() {
				match InstrType::try_from(*b).unwrap() {
					InstrType::Nop => (),
					InstrType::Cpy => {
						let (rin, rout) = (read_u8(&mut vm.it)?, read_u8(&mut vm.it)?);
						let rin = vm.regs.reg_or_cst(vm.chunk, heap, rin)?;
						*vm.regs.mut_reg(rout) = rin.clone();
					},
					InstrType::Neg => {
						let (rin, rout) = (read_u8(&mut vm.it)?, read_u8(&mut vm.it)?);
						let rin = vm.regs.reg_or_cst(vm.chunk, heap, rin)?;
						*vm.regs.mut_reg(rout) = rin.neg().ok_or_else(|| error_str("Cannot negate value!"))?;
					},
					InstrType::Add => bin_op!(add),
					InstrType::Sub => bin_op!(sub),
					InstrType::Mul => bin_op!(mul),
					InstrType::Div => bin_op!(div),
					InstrType::Pow => bin_op!(pow),
					InstrType::Mod => bin_op!(modulo),
					InstrType::Not => {
						let (rin, rout) = (read_u8(&mut vm.it)?, read_u8(&mut vm.it)?);
						let rin = vm.regs.reg_or_cst(vm.chunk, heap, rin)?;
						*vm.regs.mut_reg(rout) = rin.not().ok_or_else(|| error_str("Cannot apply logical NOT to value"))?;
					},
					InstrType::Or => bin_op!(or),
					InstrType::And => bin_op!(and),
					InstrType::Eq => {
						let (a, b, c) = (read_u8(&mut vm.it)?, read_u8(&mut vm.it)?, read_u8(&mut vm.it)?);
						let a = vm.regs.reg_or_cst(vm.chunk, heap, a)?;
						let b = vm.regs.reg_or_cst(vm.chunk, heap, b)?;
						*vm.regs.mut_reg(c) = Value::from(a.eq(&b));
					},
					InstrType::Neq => {
						let (a, b, c) = (read_u8(&mut vm.it)?, read_u8(&mut vm.it)?, read_u8(&mut vm.it)?);
						let a = vm.regs.reg_or_cst(vm.chunk, heap, a)?;
						let b = vm.regs.reg_or_cst(vm.chunk, heap, b)?;
						*vm.regs.mut_reg(c) = Value::from(!a.eq(&b));
					},
					InstrType::Lth => bin_op!(lth),
					InstrType::Leq => bin_op!(leq),
					InstrType::Gth => bin_op!(gth),
					InstrType::Geq => bin_op!(geq),
					InstrType::Func => {
						let chunk_id = read_u8(&mut vm.it)?;
						let rout = read_u8(&mut vm.it)?;
						let chunk = program.chunks.get(chunk_id as usize)
							.ok_or_else(|| error_str("Invalid chunk id"))?;
						let cur_call = vm.calls.last_mut().unwrap();
						let upvalues = chunk.upvalues.iter().copied().map(|reg| {
							if reg < MAX_REGISTERS { // Upvalue points to register 
								if let Some(upv) = cur_call.upvalues.get(&reg) {
									upv.clone()
								} else {
									let idx = cur_call.reg_win.0 + (reg as usize);
									let upv = heap.make_ref(Upvalue::new(idx));
									cur_call.upvalues.insert(reg, upv.clone());
									upv
								}
							} else { // Upvalue points to upvalue
								cur_call.closure.upvalues[(reg - MAX_REGISTERS) as usize].clone()
							}
						}).collect();
						*vm.regs.mut_reg(rout) = heap.make_value(Closure::new(chunk_id, upvalues));
					},
					InstrType::Call => {
						let func = vm.regs.reg_or_cst(vm.chunk, heap, read_u8(&mut vm.it)?)?;
						let args_start = read_u8(&mut vm.it)?;
						let args_cnt = read_u8(&mut vm.it)?;
						let rout = read_u8(&mut vm.it)?;
						if let Ok(func) = GCRef::<Closure>::try_from(func.clone()) {
							vm.call(program, func, args_start, Some(rout));
						} else if let Ok(func) = GCRef::<NativeFunction>::try_from(func.clone()) {
							let args = vm.regs.reg_range(args_start, args_cnt);
							let res = func.call(args.to_vec())?;
							*vm.regs.mut_reg(rout) = res;
						} else {
							return Err(error(format!("Cannot call value {}", func.repr())));
						}
					},
					InstrType::Ret => {
						let rin = read_u8(&mut vm.it)?;
						let temp = vm.regs.reg_or_cst(vm.chunk, heap, rin)?.clone();
						
						vm.ret(program, heap, temp)?;
					}
					InstrType::Jmp => {
						let final_add = read_rel_add(&mut vm.it, &vm.chunk.code)?;
						vm.it = iter_from(&vm.chunk.code, final_add);
					},
					InstrType::Jit => {
						let final_add = read_rel_add(&mut vm.it, &vm.chunk.code)?;
						let cond_val = vm.regs.reg_or_cst(vm.chunk, heap, read_u8(&mut vm.it)?)?;
						let cond = bool::try_from(cond_val.deref())
							.map_err(|_| error_str("Non-bool used in condition"))?;
						if cond {
							vm.it = iter_from(&vm.chunk.code, final_add);
						}
					},
					InstrType::Jif => {
						let final_add = read_rel_add(&mut vm.it, &vm.chunk.code)?;
						let cond_val = vm.regs.reg_or_cst(vm.chunk, heap, read_u8(&mut vm.it)?)?;
						let cond = bool::try_from(cond_val.deref())
							.map_err(|_| error_str("Non-bool used in condition"))?;
						if !cond {
							vm.it = iter_from(&vm.chunk.code, final_add);
						}
					},
					InstrType::GetUp => {
						let upv_idx = read_u8(&mut vm.it)?;
						let rout = read_u8(&mut vm.it)?;
						let upv = vm.calls.last().unwrap().closure.upvalues[upv_idx as usize].clone();
						*vm.regs.mut_reg(rout) = vm.regs.get_upvalue(upv);
					},
					InstrType::SetUp => {
						let upv_idx = read_u8(&mut vm.it)?;
						let rin = read_u8(&mut vm.it)?;
						let upv = vm.calls.last().unwrap().closure.upvalues[upv_idx as usize].clone();
						vm.regs.set_upvalue(upv, vm.regs.reg_or_cst(vm.chunk, heap, rin)?.clone());
					},
					InstrType::GetExt => {
						let ext_idx = read_u16(&mut vm.it)?;
						let rout = read_u8(&mut vm.it)?;
						*vm.regs.mut_reg(rout) = vm.external.get(ext_idx as usize)
							.ok_or_else(|| error_str("Invalid external value"))?.clone();
					},
					InstrType::ListNew => {
						let rout = read_u8(&mut vm.it)?;
						*vm.regs.mut_reg(rout) = heap.make_value(List::new());
					},
					InstrType::ListExtend => {
						let list = read_u8(&mut vm.it)?;
						let vals_start = read_u8(&mut vm.it)?;
						let vals_cnt = read_u8(&mut vm.it)?;
						let list = GCRef::<List>::try_from(vm.regs.reg_or_cst(vm.chunk, heap, list)?.deref().clone())
							.map_err(|_| error_str("Cannot use ListExtend on non-List value"))?;
						let vals = vm.regs.reg_range(vals_start, vals_cnt);
						list.extend(vals);
					},
					InstrType::ListGet => {
						let list = read_u8(&mut vm.it)?;
						let index = read_u8(&mut vm.it)?;
						let rout = read_u8(&mut vm.it)?;
						let list = GCRef::<List>::try_from(vm.regs.reg_or_cst(vm.chunk, heap, list)?.deref().clone())
							.map_err(|_| error_str("Cannot index non-list value"))?;
						let index = i32::try_from(vm.regs.reg_or_cst(vm.chunk, heap, index)?.deref())
							.map_err(|_| error_str("Cannot index list with non-integer"))?;
						let index = usize::try_from(index)
							.map_err(|_| error_str("Cannot index list with negative integer"))?;
						*vm.regs.mut_reg(rout) = list.get(index)?;
					}
					#[allow(unreachable_patterns)]
					i => unimplemented!("Unimplemented instruction: {:?}", i)
				}
			} else if vm.chunk_id == 0 {
				return Ok(true);
			} else { // implicit return
				vm.ret(program, heap, NIL)?;
			}
			Ok(false)
		};
		
		let mut stop = run_instr();
		
		if program.debug_info {
			if let Err(HissyError(ErrorType::Execution, err, 0)) = stop {
				let line_numbers = &vm.chunk.debug_info.line_numbers;
				let line_idx = line_numbers.iter().position(|(pos2, _)| instr_pos < *pos2)
					.unwrap_or_else(|| line_numbers.len()) - 1;
				let line = line_numbers.get(line_idx)
					.expect("Could not get line number of instruction").1;
				stop = Err(HissyError(ErrorType::Execution, err, line));
			}
		}
		
		if stop? {
			break;
		}
		
		heap.step();
	}
	
	vm.regs.free_all();
	heap.collect();
	
	Ok(())
}
