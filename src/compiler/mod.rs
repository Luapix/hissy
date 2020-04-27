
pub(crate) mod chunk;
pub use chunk::Program;


use std::ops::{Deref, DerefMut};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::convert::TryFrom;

use crate::parser::{parse, ast::{Expr, Stat, Cond, BinOp, UnaOp}};
use crate::vm::{MAX_REGISTERS, InstrType};
use chunk::{Chunk, ChunkConstant, ChunkUpvalue};


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

struct ChunkRegisters {
	required: u16,
	used: u16,
	local_cnt: u16,
}

impl ChunkRegisters {
	pub fn new() -> ChunkRegisters {
		ChunkRegisters {
			required: 0,
			used: 0,
			local_cnt: 0,
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
		debug_assert!(u16::from(i) == self.local_cnt, "Local allocated above temporaries");
		self.local_cnt += 1;
	}
	
	// Marks register as freed
	pub fn free_reg(&mut self, i: u8) {
		debug_assert!(u16::from(i) == self.used - 1, "Registers are not freed in FIFO order: {}, {}", i, self.used);
		self.used -= 1;
		if self.local_cnt > self.used {
			self.local_cnt = self.used;
		}
	}
	
	pub fn free_reg_range(&mut self, start: u8, n: u16) {
		debug_assert!(u16::from(start) + n == self.used, "Registers are not freed in FIFO order");
		self.used -= n;
		if self.local_cnt > self.used {
			self.local_cnt = self.used;
		}
	}
	
	// Marks register as freed if temporary
	pub fn free_temp_reg(&mut self, i: u8) {
		if i < MAX_REGISTERS && u16::from(i) >= self.local_cnt {
			self.free_reg(i);
		}
	}
	
	pub fn free_temp_range(&mut self, start: u8, n: u16) {
		if u16::from(start) >= self.local_cnt {
			self.free_reg_range(start, n);
		}
	}
}


enum Binding {
	Local(u8),
	Upvalue(u8),
}

// Note: registers 128-255 correspond to constants in bytecode,
// but correspond to upvalues in the parent chunk in upvalue tables.

impl Binding {
	fn encoded(&self) -> u8 {
		match self {
			Binding::Local(reg) => *reg,
			Binding::Upvalue(upv) => upv + MAX_REGISTERS,
		}
	}
}


type BlockContext = HashMap<String, u8>;

struct ChunkContext {
	regs: ChunkRegisters,
	blocks: Vec<BlockContext>,
	upvalues: Vec<ChunkUpvalue>,
}

impl ChunkContext {
	pub fn new() -> ChunkContext {
		ChunkContext {
			regs: ChunkRegisters::new(),
			blocks: Vec::new(),
			upvalues: Vec::new(),
		}
	}
	
	fn enter_block(&mut self) {
		self.blocks.push(BlockContext::new());
	}
	
	fn exit_block(&mut self) {
		let mut to_free: Vec<u8> = self.blocks.last().unwrap().values().copied().collect();
		to_free.sort_by_key(|&x| Reverse(x));
		for reg in to_free {
			self.regs.free_reg(reg);
		}
		self.blocks.pop();
	}
	
	fn find_block_local(&self, id: &str) -> Option<u8> {
		self.blocks.last().unwrap().get(id).copied()
	}
	
	fn find_chunk_binding(&self, id: &str) -> Option<Binding> {
		for ctx in self.blocks.iter().rev() {
			if let Some(reg) = ctx.get(id) {
				return Some(Binding::Local(*reg));
			}
		}
		if let Some((i,_)) = self.upvalues.iter().enumerate().find(|(_,u)| u.name == id) {
			return Some(Binding::Upvalue(u8::try_from(i).unwrap()));
		}
		None
	}
	
	fn make_local(&mut self, id: String, reg: u8) {
		self.blocks.last_mut().unwrap().insert(id, reg);
		self.regs.make_local(reg);
	}
	
	fn make_upvalue(&mut self, id: String, reg: u8) -> u8 {
		let upv = u8::try_from(self.upvalues.len()).expect("Too many upvalues in chunk");
		self.upvalues.push(ChunkUpvalue { name: id, reg });
		upv
	}
}


struct Context {
	chunks: Vec<ChunkContext>,
}

impl Context {
	pub fn new() -> Context {
		Context { chunks: Vec::new() }
	}
	
	fn enter_chunk(&mut self) {
		self.chunks.push(ChunkContext::new());
	}
	
	fn leave_chunk(&mut self, chunk: &mut Chunk) {
		let chunk_ctx = self.chunks.pop().expect("Cannot leave main chunk");
		chunk.nb_registers = chunk_ctx.regs.required;
		chunk.upvalues = chunk_ctx.upvalues;
	}
	
	fn get_binding(&mut self, id: &str) -> Option<Binding> {
		// Find a binding (local or known upvalue) in current chunk, otherwise...
		self.find_chunk_binding(id).or_else(|| {
			// Look for a binding in surrounding chunks, and if found...
			self.chunks.iter().enumerate().rev().skip(1).find_map(|(i, chunk)| {
				chunk.find_chunk_binding(id).map(|b| (i, b))
			}).map(|(i, mut binding)| {
				// Set it as an upvalue in all inner chunks successively.
				for chunk in self.chunks[i+1..].iter_mut() {
					let upv = chunk.make_upvalue(id.to_string(), binding.encoded());
					binding = Binding::Upvalue(upv);
				}
				binding
			})
		})
	}
}

impl Deref for Context {
	type Target = ChunkContext;
	
	fn deref(&self) -> &ChunkContext {
		self.chunks.last().unwrap()
	}
}

impl DerefMut for Context {
	fn deref_mut(&mut self) -> &mut ChunkContext {
		self.chunks.last_mut().unwrap()
	}
}


/// A struct holding state necessary to compilation.
#[derive(Default)]
pub struct Compiler {
	chunks: Vec<Chunk>,
}

impl Compiler {
	/// Creates a new `Compiler` object.
	pub fn new() -> Compiler {
		Default::default()
	}
	
	// Compile computation of expr (into dest if given), and returns final register
	// Warning: If no dest is given, do not assume the final register is a new, temporary one,
	// it may be a local or a constant!
	fn compile_expr(&mut self, chunk: usize, ctx: &mut Context, expr: Expr, dest: Option<u8>, name: Option<String>) -> u8 {
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
				self.chunks[chunk].compile_constant(ChunkConstant::String(s)),
			Expr::Id(s) => {
				let binding = ctx.get_binding(&s).expect("Referencing undefined binding");
				match binding {
					Binding::Local(reg) => reg,
					Binding::Upvalue(upv) => {
						self.chunks[chunk].emit_instr(InstrType::GetUp);
						self.chunks[chunk].emit_byte(upv);
						needs_copy = false;
						ctx.regs.emit_reg(&mut self.chunks[chunk], dest)
					},
				}
			},
			Expr::BinOp(op, e1, e2) => {
				let r1 = self.compile_expr(chunk, ctx, *e1, None, None);
				let r2 = self.compile_expr(chunk, ctx, *e2, None, None);
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
				let r = self.compile_expr(chunk, ctx, *e, dest, None);
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
				let func = self.compile_expr(chunk, ctx, *e, None, None);
				let n = u16::try_from(args.len()).unwrap();
				let arg_range = ctx.regs.new_reg_range(n);
				for (i, arg) in args.drain(..).enumerate() {
					let rout = u8::try_from(usize::from(arg_range) + i).unwrap();
					self.compile_expr(chunk, ctx, arg, Some(rout), None);
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
				let new_chunk = self.compile_chunk(ctx, name.unwrap_or_else(|| String::from("<func>")), bl, args);
				self.chunks[chunk].emit_instr(InstrType::Func);
				self.chunks[chunk].emit_byte(new_chunk);
				needs_copy = false;
				ctx.regs.emit_reg(&mut self.chunks[chunk], dest)
			},
			#[allow(unreachable_patterns)]
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


	fn compile_block(&mut self, chunk: usize, ctx: &mut Context, stats: Vec<Stat>) {
		let used_before = ctx.regs.used;
		
		ctx.enter_block();
		
		for stat in stats {
			match stat {
				Stat::ExprStat(e) => {
					let reg = self.compile_expr(chunk, ctx, e, None, None);
					ctx.regs.free_temp_reg(reg);
				},
				Stat::Let((id, _ty), e) => {
					if let Some(reg) = ctx.find_block_local(&id) { // if binding already exists
						ctx.regs.free_reg(reg);
					}
					let reg = ctx.regs.new_reg();
					self.compile_expr(chunk, ctx, e, Some(reg), Some(id.clone()));
					ctx.make_local(id, reg);
				},
				Stat::Set(id, e) => {
					let binding = ctx.get_binding(&id).expect("Referencing undefined binding");
					match binding {
						Binding::Local(reg) => {
							self.compile_expr(chunk, ctx, e, Some(reg), None);
						},
						Binding::Upvalue(upv) => {
							let reg = self.compile_expr(chunk, ctx, e, None, None);
							ctx.regs.free_temp_reg(reg);
							self.chunks[chunk].emit_instr(InstrType::SetUp);
							self.chunks[chunk].emit_byte(upv);
							self.chunks[chunk].emit_byte(reg);
						},
					}
				},
				Stat::Cond(mut branches) => {
					let mut end_jmps = vec![];
					let last_branch = branches.len() - 1;
					for (i, (cond, bl)) in branches.drain(..).enumerate() {
						let mut after_jmp = None;
						match cond {
							Cond::If(e) => {
								let cond_reg = self.compile_expr(chunk, ctx, e, None, None);
								
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
					let cond_reg = self.compile_expr(chunk, ctx, e, None, None);
					
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
					let reg = self.compile_expr(chunk, ctx, e, None, None);
					ctx.regs.free_temp_reg(reg);
					self.chunks[chunk].emit_instr(InstrType::Log);
					self.chunks[chunk].emit_byte(reg);
				},
				Stat::Return(e) => {
					let reg = self.compile_expr(chunk, ctx, e, None, None);
					ctx.regs.free_temp_reg(reg);
					self.chunks[chunk].emit_instr(InstrType::Ret);
					self.chunks[chunk].emit_byte(reg);
				},
				#[allow(unreachable_patterns)]
				_ => unimplemented!("Unimplemented statement type: {:?}", stat)
			}
		}
		
		ctx.exit_block();
		
		debug_assert!(used_before == ctx.regs.used, "Leaked register");
		// Basic check to make sure no registers have been "leaked"
	}


	fn compile_chunk(&mut self, ctx: &mut Context, name: String, ast: Vec<Stat>, args: Vec<String>) -> u8 {
		self.chunks.push(Chunk::new(name));
		let chunk_id = self.chunks.len() - 1;
		
		ctx.enter_chunk();
		ctx.enter_block();
		for id in args {
			let reg = ctx.regs.new_reg();
			ctx.make_local(id, reg);
		}
		self.compile_block(chunk_id, ctx, ast);
		ctx.exit_block();
		ctx.leave_chunk(&mut self.chunks[chunk_id]);
		
		u8::try_from(chunk_id).expect("Too many chunks")
	}
	
	/// Compiles a string slice containing Hissy code into a [`Program`], consuming the `Compiler`.
	pub fn compile_program(mut self, input: &str) -> Result<Program, String> {
		let ast = parse(input)?;
		let mut ctx = Context::new();
		self.compile_chunk(&mut ctx, String::from("<main>"), ast, Vec::new());
		Ok(Program { chunks: self.chunks })
	}
}
