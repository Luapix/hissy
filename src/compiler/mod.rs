
use std::cmp::Reverse;
use std::collections::HashMap;
use std::convert::TryFrom;

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
	required: u16,
	used: u16,
	locals: u16,
}

impl RegisterManager {
	pub fn new() -> RegisterManager {
		RegisterManager {
			required: 0,
			used: 0,
			locals: 0,
		}
	}
	
	pub fn new_reg(&mut self) -> u8 {
		let new_reg = u8::try_from(self.used).ok().filter(|r| *r < MAX_REGISTERS)
			.expect("Cannot compile: Too many registers required");
		self.used += 1;
		if self.used > self.required {
			self.required = self.used
		}
		new_reg
	}
	
	pub fn new_reg_range(&mut self, n: u16) -> u8 {
		u8::try_from(self.used + n - 1).ok().filter(|r| *r < MAX_REGISTERS)
			.expect("Cannot compile: Too many registers required");
		let range_start = u8::try_from(self.used).unwrap();
		self.used += n;
		if self.used > self.required {
			self.required = self.used
		}
		range_start
	}
	
	// Emits register to chunk; dest if Some, else new_reg()
	pub fn emit_reg(&mut self, chunk: &mut Chunk, dest: Option<u8>) -> u8 {
		let reg = dest.map_or_else(|| self.new_reg(), |r| r);
		chunk.emit_byte(reg);
		reg
	}
	
	pub fn make_local(&mut self, i: u8) {
		debug_assert!(u16::from(i) == self.locals, "Local allocated above temporaries");
		self.locals += 1;
	}
	
	// Marks register as freed
	pub fn free_reg(&mut self, i: u8) {
		debug_assert!(u16::from(i) == self.used - 1, "Registers are not freed in FIFO order: {}, {}", i, self.used);
		self.used -= 1;
		if self.locals > self.used {
			self.locals = self.used;
		}
	}
	
	pub fn free_reg_range(&mut self, start: u8, n: u16) {
		debug_assert!(u16::from(start) + n == self.used, "Registers are not freed in FIFO order");
		self.used -= n;
		if self.locals > self.used {
			self.locals = self.used;
		}
	}
	
	// Marks register as freed if temporary
	pub fn free_temp_reg(&mut self, i: u8) {
		if i < MAX_REGISTERS && u16::from(i) >= self.locals {
			self.free_reg(i);
		}
	}
	
	pub fn free_temp_range(&mut self, start: u8, n: u16) {
		if u16::from(start) >= self.locals {
			self.free_reg_range(start, n);
		}
	}
}


pub struct Locals {
	contexts: Vec<HashMap<String, u8>>
}

impl Locals {
	fn new() -> Locals {
		Locals { contexts: Vec::new() }
	}
	
	fn find_local(&self, id: &str) -> Option<u8> {
		for ctx in self.contexts.iter().rev() {
			if let Some(reg) = ctx.get(id) {
				return Some(*reg)
			}
		}
		None
	}
	
	fn find_hyper_local(&self, id: &str) -> Option<u8> {
		self.contexts.last().unwrap().get(id).copied()
	}
}


pub struct ChunkContext {
	regs: RegisterManager,
	locals: Locals,
}

impl ChunkContext {
	pub fn new() -> ChunkContext {
		ChunkContext {
			regs: RegisterManager::new(),
			locals: Locals::new(),
		}
	}
	
	fn enter_block(&mut self) {
		self.locals.contexts.push(HashMap::new());
	}
	
	fn exit_block(&mut self) {
		let mut to_free: Vec<u8> = self.locals.contexts.last().unwrap().values().copied().collect();
		to_free.sort_by_key(|&x| Reverse(x));
		for reg in to_free {
			self.regs.free_reg(reg);
		}
		self.locals.contexts.pop();
	}
	
	fn make_local(&mut self, id: String, reg: u8) {
		self.locals.contexts.last_mut().unwrap().insert(id, reg);
		self.regs.make_local(reg);
	}
}


pub struct Compiler {
	chunks: Vec<Chunk>,
}

impl Compiler {
	pub fn new() -> Compiler {
		Compiler { chunks: Vec::new() }
	}
	
	// Compile computation of expr (into dest if given), and returns final register
	// Warning: If no dest is given, do not assume the final register is a new, temporary one,
	// it may be a local or a constant!
	fn compile_expr(&mut self, chunk: usize, ctx: &mut ChunkContext, expr: Expr, dest: Option<u8>) -> u8 {
		let mut needs_copy = true;
		let mut reg = match expr {
			Expr::Nil =>
				self.chunks[chunk].compile_constant(ChunkConstant::Nil),
			Expr::Bool(b) =>
				self.chunks[chunk].compile_constant(ChunkConstant::Bool(b)),
			Expr::Int(i) =>
				self.chunks[chunk].compile_constant(ChunkConstant::Int(i)),
			Expr::Real(r) =>
				self.chunks[chunk].compile_constant(ChunkConstant::Real(r)),
			Expr::String(s) => 
				self.chunks[chunk].compile_constant(ChunkConstant::String(s.clone())),
			Expr::Id(s) =>
				ctx.locals.find_local(&s).expect("Referencing undefined local"),
			Expr::BinOp(op, e1, e2) => {
				let r1 = self.compile_expr(chunk, ctx, *e1, None);
				let r2 = self.compile_expr(chunk, ctx, *e2, None);
				ctx.regs.free_temp_reg(r2);
				ctx.regs.free_temp_reg(r1);
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
				self.chunks[chunk].emit_instr(instr);
				self.chunks[chunk].emit_byte(r1);
				self.chunks[chunk].emit_byte(r2);
				needs_copy = false;
				ctx.regs.emit_reg(&mut self.chunks[chunk], dest)
			},
			Expr::UnaOp(op, e) => {
				let r = self.compile_expr(chunk, ctx, *e, dest);
				ctx.regs.free_temp_reg(r);
				let instr = match op {
					UnaOp::Not => InstrType::Not,
					UnaOp::Minus => InstrType::Neg,
				};
				self.chunks[chunk].emit_instr(instr);
				self.chunks[chunk].emit_byte(r);
				needs_copy = false;
				ctx.regs.emit_reg(&mut self.chunks[chunk], dest)
			},
			Expr::Call(e, mut args) => {
				let func = self.compile_expr(chunk, ctx, *e, None);
				let n = u16::try_from(args.len()).unwrap();
				let arg_range = ctx.regs.new_reg_range(n);
				for (i, arg) in args.drain(..).enumerate() {
					let rout = u8::try_from(usize::from(arg_range) + i).unwrap();
					self.compile_expr(chunk, ctx, arg, Some(rout));
				}
				ctx.regs.free_temp_range(arg_range, n);
				ctx.regs.free_temp_reg(func);
				self.chunks[chunk].emit_instr(InstrType::Call);
				self.chunks[chunk].emit_byte(func);
				self.chunks[chunk].emit_byte(arg_range);
				needs_copy = false;
				ctx.regs.emit_reg(&mut self.chunks[chunk], dest)
			},
			Expr::Function(args, bl) =>  {
				let new_chunk = self.compile_chunk(String::from("func"), bl, args);
				self.chunks[chunk].emit_instr(InstrType::Func);
				self.chunks[chunk].emit_byte(new_chunk);
				needs_copy = false;
				ctx.regs.emit_reg(&mut self.chunks[chunk], dest)
			},
			_ => unimplemented!("Unimplemented expression type: {:?}", expr),
		};
		
		if needs_copy {
			if let Some(dest) = dest {
				self.chunks[chunk].emit_instr(InstrType::Cpy);
				self.chunks[chunk].emit_byte(reg);
				self.chunks[chunk].emit_byte(dest);
				reg = dest;
			}
		}
		
		reg
	}


	fn compile_block(&mut self, chunk: usize, ctx: &mut ChunkContext, stats: Vec<Stat>) {
		let used_before = ctx.regs.used;
		
		ctx.enter_block();
		
		for stat in stats {
			match stat {
				Stat::ExprStat(e) => {
					let reg = self.compile_expr(chunk, ctx, e, None);
					ctx.regs.free_temp_reg(reg);
				},
				Stat::Let((id, _ty), e) => {
					if let Some(reg) = ctx.locals.find_hyper_local(&id) { // if binding already exists
						ctx.regs.free_reg(reg);
					}
					let reg = ctx.regs.new_reg();
					self.compile_expr(chunk, ctx, e, Some(reg));
					ctx.make_local(id, reg);
				},
				Stat::Set(id, e) => {
					let reg = ctx.locals.find_local(&id).expect("Referencing undefined local");
					self.compile_expr(chunk, ctx, e, Some(reg));
				},
				Stat::Cond(mut branches) => {
					let mut end_jmps = vec![];
					let last_branch = branches.len() - 1;
					for (i, (cond, bl)) in branches.drain(..).enumerate() {
						let mut after_jmp = None;
						match cond {
							Cond::If(e) => {
								let cond_reg = self.compile_expr(chunk, ctx, e, None);
								
								// Jump to next branch if false
								ctx.regs.free_temp_reg(cond_reg);
								self.chunks[chunk].emit_instr(InstrType::Jif);
								after_jmp = Some(self.chunks[chunk].code.len());
								self.chunks[chunk].emit_byte(0); // Placeholder
								self.chunks[chunk].emit_byte(cond_reg);
								
								self.compile_block(chunk, ctx, bl);
								
								if i != last_branch {
									// Jump out of condition at end of block
									self.chunks[chunk].emit_instr(InstrType::Jmp);
									let from2 = self.chunks[chunk].code.len();
									self.chunks[chunk].emit_byte(0); // Placeholder 2
									end_jmps.push(from2);
								}
							},
							Cond::Else => {
								self.compile_block(chunk, ctx, bl);
							}
						}
						
						if let Some(from) = after_jmp {
							fill_in_jump_from(&mut self.chunks[chunk], from);
						}
					}
					
					// Fill in jumps to end
					for from in end_jmps {
						fill_in_jump_from(&mut self.chunks[chunk], from);
					}
				},
				Stat::While(e, bl) => {
					let begin = self.chunks[chunk].code.len();
					let cond_reg = self.compile_expr(chunk, ctx, e, None);
					
					ctx.regs.free_temp_reg(cond_reg);
					self.chunks[chunk].emit_instr(InstrType::Jif);
					let placeholder = self.chunks[chunk].code.len();
					self.chunks[chunk].emit_byte(0); // Placeholder
					self.chunks[chunk].emit_byte(cond_reg);
					
					self.compile_block(chunk, ctx, bl);
					
					self.chunks[chunk].emit_instr(InstrType::Jmp);
					emit_jump_to(&mut self.chunks[chunk], begin);
					fill_in_jump_from(&mut self.chunks[chunk], placeholder);
				},
				Stat::Log(e) => {
					let reg = self.compile_expr(chunk, ctx, e, None);
					ctx.regs.free_temp_reg(reg);
					self.chunks[chunk].emit_instr(InstrType::Log);
					self.chunks[chunk].emit_byte(reg);
				},
				Stat::Return(e) => {
					let reg = self.compile_expr(chunk, ctx, e, None);
					ctx.regs.free_temp_reg(reg);
					self.chunks[chunk].emit_instr(InstrType::Ret);
					self.chunks[chunk].emit_byte(reg);
				},
				_ => unimplemented!()
			}
		}
		
		ctx.exit_block();
		
		debug_assert!(used_before == ctx.regs.used, "Leaked register");
		// Basic check to make sure no registers have been "leaked"
	}


	fn compile_chunk(&mut self, name: String, ast: Vec<Stat>, args: Vec<String>) -> u8 {
		self.chunks.push(Chunk::new(name));
		let chunk_id = self.chunks.len() - 1;
		let mut ctx = ChunkContext::new();
		ctx.enter_block();
		for id in args {
			let reg = ctx.regs.new_reg();
			ctx.make_local(id, reg);
		}
		self.compile_block(chunk_id, &mut ctx, ast);
		ctx.exit_block();
		self.chunks[chunk_id].nb_registers = ctx.regs.required;
		u8::try_from(chunk_id).expect("Too many chunks")
	}
	
	pub fn compile_program(mut self, input: &str) -> Result<Program, String> {
		let ast = parse(input)?;
		self.compile_chunk(String::from("main"), ast, Vec::new());
		Ok(Program { chunks: self.chunks })
	}
}
