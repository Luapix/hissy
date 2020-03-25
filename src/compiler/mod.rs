
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};

use super::parser::{parse, ast::{Expr, Stat, Cond, BinOp, UnaOp}};
use super::vm::{MAX_REGISTERS, chunk::{Chunk, ChunkConstant, Program}, InstrType};

#[derive(Debug, PartialEq, Eq)]
pub enum RegContent {
	Temp,
	Local,
}

fn emit_jump_to(chunk: &mut Chunk, add: usize) {
	let from = chunk.code.len();
	let to = add;
	let rel_jmp = to as isize - from as isize;
	let rel_jmp = i8::try_from(rel_jmp).expect("Jump too large");
	chunk.emit_byte(rel_jmp as u8);
}

fn fill_in_jump_from(chunk: &mut Chunk, add: usize) {
	let from = add;
	let to = chunk.code.len();
	let rel_jmp = to as isize - from as isize;
	let rel_jmp = i8::try_from(rel_jmp).expect("Jump too large");
	chunk.code[add] = rel_jmp as u8;
}

pub struct RegisterManager {
	reg_cnt: u16,
	next_free_reg: u16,
	used_registers: HashMap<u8, RegContent>,
}

impl RegisterManager {
	pub fn new() -> RegisterManager {
		RegisterManager {
			reg_cnt: 0,
			next_free_reg: 0,
			used_registers: HashMap::new(),
		}
	}
	
	pub fn new_reg(&mut self) -> u8 {
		let new_reg = self.next_free_reg.try_into().ok().filter(|r| *r < MAX_REGISTERS)
			.expect("Cannot compile: Too many registers required");
		if new_reg as u16 + 1 > self.reg_cnt {
			self.reg_cnt = new_reg as u16 + 1;
		}
		self.used_registers.insert(new_reg, RegContent::Temp);
		while self.next_free_reg.try_into().map_or(false, |i: u8| self.used_registers.contains_key(&i)) {
			self.next_free_reg += 1;
		}
		new_reg
	}
	
	// Emits register to chunk; dest if Some, else new_reg()
	pub fn emit_reg(&mut self, chunk: &mut Chunk, dest: Option<u8>) -> u8 {
		let reg = dest.map_or_else(|| self.new_reg(), |r| r);
		chunk.emit_byte(reg);
		reg
	}
	
	// Marks register as freed
	pub fn free_reg(&mut self, i: u8) {
		self.used_registers.remove(&i);
		if (i as u16) < self.next_free_reg {
			self.next_free_reg = i as u16;
		}
	}
	
	// Marks register as freed if temporary
	pub fn free_temp_reg(&mut self, i: u8) {
		if i < MAX_REGISTERS && self.used_registers[&i] == RegContent::Temp {
			self.free_reg(i);
		}
	}
}


pub struct Compiler {
	reg_mgr: RegisterManager,
	contexts: Vec<HashMap<String, u8>>,
}

impl Compiler {
	pub fn new() -> Compiler {
		Compiler {
			reg_mgr: RegisterManager::new(),
			contexts: Vec::new(),
		}
	}
	
	fn find_local(&self, id: &str) -> Option<u8> {
		for ctx in self.contexts.iter().rev() {
			if let Some(reg) = ctx.get(id) {
				return Some(*reg)
			}
		}
		None
	}
	
	// Adds constant to the list of constants in the chunk, and return the constant's register index
	fn compile_constant(&mut self, chunk: &mut Chunk, val: ChunkConstant) -> u8 {
		chunk.constants.push(val);
		let cst_idx = isize::try_from(chunk.constants.len() - 1).unwrap();
		let reg = 255 - cst_idx;
		reg.try_into().ok().filter(|r| *r >= MAX_REGISTERS).expect("Too many constants required")
	}
	
	// Compile computation of expr (into dest if given), and returns final register
	// Warning: If no dest is given, do not assume the final register is a new, temporary one,
	// it may be a local or a constant!
	fn compile_expr(&mut self, chunk: &mut Chunk, expr: &Expr, dest: Option<u8>) -> u8 {
		let mut needs_copy = true;
		let mut reg = match expr {
			Expr::Nil =>
				self.compile_constant(chunk, ChunkConstant::Nil),
			Expr::Bool(b) =>
				self.compile_constant(chunk, ChunkConstant::Bool(*b)),
			Expr::Int(i) =>
				self.compile_constant(chunk, ChunkConstant::Int(*i)),
			Expr::Real(r) =>
				self.compile_constant(chunk, ChunkConstant::Real(*r)),
			Expr::String(s) => 
				self.compile_constant(chunk, ChunkConstant::String(s.clone())),
			Expr::Id(s) =>
				self.find_local(s).expect("Referencing undefined local"),
			Expr::BinOp(op, e1, e2) => {
				let r1 = self.compile_expr(chunk, &e1, None);
				let r2 = self.compile_expr(chunk, &e2, None);
				self.reg_mgr.free_temp_reg(r1);
				self.reg_mgr.free_temp_reg(r2);
				let instr = match op {
					BinOp::Plus => InstrType::Add,
					BinOp::Minus => InstrType::Sub,
					BinOp::Times => InstrType::Mul,
					BinOp::Divides => InstrType::Div,
					BinOp::Modulo => InstrType::Mod,
					BinOp::Power => InstrType::Pow,
					BinOp::LEq => InstrType::Leq,
					BinOp::GEq => InstrType::Geq,
					BinOp::Less => InstrType::Lth,
					BinOp::Greater => InstrType::Gth,
					BinOp::Equal => InstrType::Eq,
					BinOp::NEq => InstrType::Neq,
					BinOp::And => InstrType::And,
					BinOp::Or => InstrType::Or,
				};
				chunk.emit_instr(instr);
				chunk.emit_byte(r1);
				chunk.emit_byte(r2);
				needs_copy = false;
				self.reg_mgr.emit_reg(chunk, dest)
			},
			Expr::UnaOp(op, e) => {
				let r = self.compile_expr(chunk, &e, dest);
				self.reg_mgr.free_temp_reg(r);
				let instr = match op {
					UnaOp::Not => InstrType::Not,
					UnaOp::Minus => InstrType::Neg,
				};
				chunk.emit_instr(instr);
				chunk.emit_byte(r);
				needs_copy = false;
				self.reg_mgr.emit_reg(chunk, dest)
			},
			_ => unimplemented!("Unimplemented expression type: {:?}", expr),
		};
		
		if needs_copy {
			if let Some(dest) = dest {
				chunk.emit_instr(InstrType::Cpy);
				chunk.emit_byte(reg);
				chunk.emit_byte(dest);
				reg = dest;
			}
		}
		
		reg
	}
	
	fn compile_block(&mut self, chunk: &mut Chunk, stats: Vec<Stat>) {
		let used_before = self.reg_mgr.used_registers.len();
		self.contexts.push(HashMap::new());
		for stat in stats {
			match stat {
				Stat::ExprStat(e) => {
					let reg = self.compile_expr(chunk, &e, None);
					self.reg_mgr.free_temp_reg(reg);
				},
				Stat::Let((id, _ty), e) => {
					if let Some(reg) = self.contexts.last().unwrap().get(&id).copied() { // if binding already exists
						self.reg_mgr.free_reg(reg);
					}
					let reg = self.reg_mgr.new_reg();
					self.compile_expr(chunk, &e, Some(reg));
					self.contexts.last_mut().unwrap().insert(id, reg);
					self.reg_mgr.used_registers.insert(reg, RegContent::Local);
				},
				Stat::Set(id, e) => {
					let reg = self.find_local(&id).expect("Referencing undefined local");
					self.compile_expr(chunk, &e, Some(reg));
				},
				Stat::Cond(branches) => {
					let mut last_jmp = None;
					let mut end_jmps = vec![];
					for (cond, bl) in branches {
						if let Some(from) = last_jmp {
							// Fill in jump from previous branch
							fill_in_jump_from(chunk, from);
						}
						
						match cond {
							Cond::If(e) => {
								let cond_reg = self.compile_expr(chunk, &e, None);
								
								// Jump to next branch if false
								self.reg_mgr.free_temp_reg(cond_reg);
								chunk.emit_instr(InstrType::Jif);
								let from = chunk.code.len();
								chunk.emit_byte(0); // Placeholder
								chunk.emit_byte(cond_reg);
								last_jmp = Some(from);
								
								self.compile_block(chunk, bl);
								
								// Jump out of condition at end of block
								chunk.emit_instr(InstrType::Jmp);
								let from = chunk.code.len();
								chunk.emit_byte(0); // Placeholder 2
								end_jmps.push(from);
							},
							Cond::Else => {
								self.compile_block(chunk, bl);
							}
						}
					}
					
					// Fill in jumps to end
					for from in end_jmps {
						fill_in_jump_from(chunk, from);
					}
				},
				Stat::While(e, bl) => {
					let begin = chunk.code.len();
					let cond_reg = self.compile_expr(chunk, &e, None);
					
					self.reg_mgr.free_temp_reg(cond_reg);
					chunk.emit_instr(InstrType::Jif);
					let placeholder = chunk.code.len();
					chunk.emit_byte(0); // Placeholder
					chunk.emit_byte(cond_reg);
					
					self.compile_block(chunk, bl);
					
					chunk.emit_instr(InstrType::Jmp);
					emit_jump_to(chunk, begin);
					fill_in_jump_from(chunk, placeholder);
				},
				Stat::Log(e) => {
					let reg = self.compile_expr(chunk, &e, None);
					self.reg_mgr.free_temp_reg(reg);
					chunk.emit_instr(InstrType::Log);
					chunk.emit_byte(reg);
				},
				_ => unimplemented!()
			}
		}
		for reg in self.contexts.last().unwrap().values().copied() {
			self.reg_mgr.free_reg(reg);
		}
		self.contexts.pop();
		
		debug_assert!(used_before == self.reg_mgr.used_registers.len(), "Leaked register");
		// Basic check to make sure no registers have been "leaked"
	}
	
	fn compile_chunk(&mut self, chunk: &mut Chunk, ast: Vec<Stat>) {
		self.compile_block(chunk, ast);
		chunk.nb_registers = self.reg_mgr.reg_cnt;
	}
	
	pub fn compile_program(&mut self, input: &str) -> Result<Program, String> {
		let ast = parse(input)?;
		let mut program = Program {
			main: Chunk::new()
		};
		self.compile_chunk(&mut program.main, ast);
		Ok(program)
	}
}
