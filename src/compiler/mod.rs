
pub(crate) mod chunk;
#[macro_use]
pub(crate) mod types;


pub use chunk::Program;
pub use types::{Type, PrimitiveType};

use std::ops::{Deref, DerefMut};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::convert::TryFrom;

use crate::{HissyError, ErrorType};
use crate::serial::write_u16;
use crate::parser::{parse, ast, ast::*};
use crate::vm::{MAX_REGISTERS, InstrType, prelude};
use chunk::{Chunk, ChunkConstant};



fn error(s: String) -> HissyError {
	HissyError(ErrorType::Compilation, s, 0)
}
fn error_str(s: &str) -> HissyError {
	error(String::from(s))
}


fn emit_jump_to(chunk: &mut Chunk, add: usize) -> Result<(), HissyError> {
	let from = chunk.code.len();
	let to = add;
	let rel_jmp = to as isize - from as isize;
	let rel_jmp = i8::try_from(rel_jmp).map_err(|_| error_str("Jump too large"))?;
	chunk.emit_byte(rel_jmp as u8);
	Ok(())
}

fn fill_in_jump_from(chunk: &mut Chunk, add: usize) -> Result<(), HissyError> {
	let from = add;
	let to = chunk.code.len();
	let rel_jmp = to as isize - from as isize;
	let rel_jmp = i8::try_from(rel_jmp).map_err(|_| error_str("Jump too large"))?;
	chunk.code[add] = rel_jmp as u8;
	Ok(())
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
	
	pub fn new_reg(&mut self) -> Result<u8, HissyError> {
		let new_reg = u8::try_from(self.used).ok().filter(|r| *r < MAX_REGISTERS)
			.ok_or_else(|| error_str("Cannot compile: Too many registers required"))?;
		self.used += 1;
		if self.used > self.required {
			self.required = self.used
		}
		Ok(new_reg)
	}
	
	pub fn new_reg_range(&mut self, n: u8) -> Result<u8, HissyError> {
		u8::try_from(self.used + (n as u16) - 1).ok().filter(|r| *r < MAX_REGISTERS)
			.ok_or_else(|| error_str("Cannot compile: Too many registers required"))?;
		let range_start = u8::try_from(self.used).unwrap();
		self.used += n as u16;
		if self.used > self.required {
			self.required = self.used
		}
		Ok(range_start)
	}
	
	pub fn make_local(&mut self, i: u8) {
		assert!(u16::from(i) == self.local_cnt, "Local allocated above temporaries");
		self.local_cnt += 1;
	}
	
	// Marks register as freed
	pub fn free_reg(&mut self, i: u8) {
		assert!(u16::from(i) == self.used - 1, "Registers are not freed in FIFO order: {}, {}", i, self.used);
		self.used -= 1;
		if self.local_cnt > self.used {
			self.local_cnt = self.used;
		}
	}
	
	pub fn free_reg_range(&mut self, start: u8, n: u8) {
		assert!((start as u16) + (n as u16) == self.used, "Registers are not freed in FIFO order");
		self.used -= n as u16;
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
	
	pub fn free_temp_range(&mut self, start: u8, n: u8) {
		if u16::from(start) >= self.local_cnt {
			self.free_reg_range(start, n);
		}
	}
}


enum Binding {
	Local(u8, Type),
	Upvalue(u8, Type),
	External(u16, Type),
}


#[derive(Clone)]
struct Local {
	reg: u8,
	ty: Type,
	closed_over: bool,
}

type BlockContext = HashMap<String, Local>;

struct UpvalueBinding {
	name: String,
	reg: u8,
	ty: Type,
}

struct ChunkContext {
	regs: ChunkRegisters,
	blocks: Vec<BlockContext>,
	upvalues: Vec<UpvalueBinding>,
	ret_ty: Type,
}

impl ChunkContext {
	pub fn new(ret_ty: Type) -> ChunkContext {
		ChunkContext {
			regs: ChunkRegisters::new(),
			blocks: Vec::new(),
			upvalues: Vec::new(),
			ret_ty,
		}
	}
	
	fn enter_block(&mut self) {
		self.blocks.push(BlockContext::new());
	}
	
	fn leave_block(&mut self, chunk: &mut Chunk) {
		let to_close: Vec<u8> = self.blocks.last().unwrap().values()
			.filter_map(|l| if l.closed_over { Some(l.reg) } else { None }).collect();
		for reg in to_close {
			chunk.emit_instr(InstrType::CloseUp);
			chunk.emit_byte(reg);
		}
		
		let mut to_free: Vec<u8> = self.blocks.last().unwrap().values().map(|l| l.reg).collect();
		to_free.sort_by_key(|&x| Reverse(x));
		for reg in to_free {
			self.regs.free_reg(reg);
		}
		self.blocks.pop();
	}
	
	fn find_block_local(&self, id: &str) -> Option<Local> {
		self.blocks.last().unwrap().get(id).cloned()
	}
	
	fn find_chunk_binding(&self, id: &str) -> Option<Binding> {
		for ctx in self.blocks.iter().rev() {
			if let Some(local) = ctx.get(id).cloned() {
				return Some(Binding::Local(local.reg, local.ty));
			}
		}
		if let Some((i,u)) = self.upvalues.iter().enumerate().find(|(_,u)| u.name == id) {
			return Some(Binding::Upvalue(u8::try_from(i).unwrap(), u.ty.clone()));
		}
		None
	}
	
	fn make_local(&mut self, id: String, reg: u8, ty: Type) {
		self.blocks.last_mut().unwrap().insert(id, Local { reg, ty, closed_over: false });
		self.regs.make_local(reg);
	}
	
	fn make_upvalue(&mut self, id: String, reg: u8, ty: Type) -> Result<u8, HissyError> {
		let upv = u8::try_from(self.upvalues.len()).map_err(|_| error_str("Too many upvalues in chunk"));
		self.upvalues.push(UpvalueBinding { name: id, reg, ty });
		upv
	}
	
	fn close_over(&mut self, id: &str) {
		for ctx in self.blocks.iter_mut().rev() {
			if let Some(local) = ctx.get_mut(id) {
				local.closed_over = true;
				return;
			}
		}
		panic!("Trying to close over unknown local binding {}", id);
	}
}


struct Context {
	stack: Vec<ChunkContext>,
	external: Vec<(String, Type)>,
}

impl Context {
	pub fn new() -> Context {
		Context {
			stack: Vec::new(),
			external: prelude::list(),
		}
	}
	
	fn enter(&mut self, ret_ty: Type) {
		self.stack.push(ChunkContext::new(ret_ty));
	}
	
	fn leave(&mut self) {
		self.stack.pop().expect("Cannot leave main chunk");
	}
	
	fn get_binding(&mut self, id: &str) -> Result<Option<Binding>, HissyError> {
		// Find a binding (local or known upvalue) in current chunk, otherwise...
		if let Some(binding) = self.find_chunk_binding(id) {
			Ok(Some(binding))
		} else {
			// Look for a binding in surrounding chunks, and if found...
			let binding = self.stack.iter().enumerate().rev().skip(1).find_map(|(i, ctx)| {
				ctx.find_chunk_binding(id).map(|b| (i, b))
			});
			if let Some((i, mut binding)) = binding {
				if let Binding::Local(_,_) = binding {
					self.stack[i].close_over(id);
				}
				
				// Set it as an upvalue in all inner chunks successively.
				for ctx in self.stack[i+1..].iter_mut() {
					let (encoded, ty) = match binding {
						Binding::Local(reg, ty) => (reg, ty),
						Binding::Upvalue(upv, ty) => (upv + MAX_REGISTERS, ty),
						_ => unreachable!(),
					};
					// Note: registers 128-255 correspond to constants in bytecode,
					// but correspond to upvalues in the parent chunk in upvalue tables.
					let upv = ctx.make_upvalue(id.to_string(), encoded, ty.clone())?;
					binding = Binding::Upvalue(upv, ty);
				}
				Ok(Some(binding))
			} else if let Some(ext_idx) = self.external.iter().position(|(id2, _)| id == id2) {
				let ty = self.external[ext_idx].1.clone();
				let ext_idx = u16::try_from(ext_idx).expect("External index is too high");
				Ok(Some(Binding::External(ext_idx, ty)))
			} else {
				Ok(None)
			}
		}
	}
}

impl Deref for Context {
	type Target = ChunkContext;
	
	fn deref(&self) -> &ChunkContext {
		self.stack.last().unwrap()
	}
}
impl DerefMut for Context {
	fn deref_mut(&mut self) -> &mut ChunkContext {
		self.stack.last_mut().unwrap()
	}
}



struct ChunkManager {
	chunks: Vec<Chunk>,
	stack: Vec<usize>,
}

impl ChunkManager {
	fn new() -> ChunkManager {
		ChunkManager { chunks: vec![], stack: vec![] }
	}
	
	fn enter(&mut self) -> usize {
		let idx = self.chunks.len();
		self.chunks.push(Chunk::new());
		self.stack.push(idx);
		idx
	}
	fn leave(&mut self) {
		self.stack.pop().unwrap();
	}
	
	fn finish(self) -> Vec<Chunk> {
		self.chunks
	}
}

impl Deref for ChunkManager {
	type Target = Chunk;
	
	fn deref(&self) -> &Chunk {
		&self.chunks[*self.stack.last().unwrap()]
	}
}
impl DerefMut for ChunkManager {
	fn deref_mut(&mut self) -> &mut Chunk {
		&mut self.chunks[*self.stack.last().unwrap()]
	}
}


fn resolve_type(ty: &ast::Type) -> Result<Type, HissyError> {
	match ty {
		ast::Type::Named(name) => {
			match name.deref() {
				"Any" => Ok(Type::Any),
				"Nil" => Ok(prim_ty!(Nil)),
				"Bool" => Ok(prim_ty!(Bool)),
				"Int" => Ok(prim_ty!(Int)),
				"Real" => Ok(prim_ty!(Real)),
				"String" => Ok(prim_ty!(String)),
				_ => Err(error(format!("Unknown type name '{}'", name)))
			}
		},
		ast::Type::Function(args, res) => {
			let args: Result<Vec<Type>, HissyError> = args.iter().map(resolve_type).collect();
			Ok(Type::TypedFunction(args?, Box::new(resolve_type(res)?)))
		},
	}
}

fn resolve_function_type(args: &[(String, ast::Type)], res_ty: &ast::Type) -> Result<Type, HissyError> {
	let args_ty: Result<Vec<Type>, HissyError> = args.iter().map(|(_,t)| Ok(resolve_type(t)?)).collect();
	let args_ty = args_ty?;
	let res_ty = resolve_type(res_ty)?;
	Ok(Type::TypedFunction(args_ty, Box::new(res_ty)))
}


fn can_reach_end(block: &Block) -> bool {
	for Positioned(stat, _) in block {
		match stat {
			Stat::Cond(branches) => {
				if branches.iter().find(|(cond, _)| cond == &Cond::Else).is_some() { // If exhaustive match
					if branches.iter().all(|(_, block2)| !can_reach_end(block2)) {
						return false;
					}
				}
			},
			Stat::Return(_) => return false,
			_ => {},
		}
	}
	true
}


enum ObjectProp {
	Method { ns_idx: u16, prop_idx: u8, prop_ty: Type },
}


/// A struct holding state necessary to compilation.
pub struct Compiler {
	debug_info: bool,
	ctx: Context,
	chunk: ChunkManager,
}

impl Compiler {
	/// Creates a new `Compiler` object.
	pub fn new(debug_info: bool) -> Compiler {
		Compiler {
			debug_info,
			ctx: Context::new(),
			chunk: ChunkManager::new(),
		}
	}
	
	// Emits register to chunk; dest if Some, else new_reg()
	fn emit_reg(&mut self, dest: Option<u8>) -> Result<u8, HissyError> {
		let reg = dest.map_or_else(|| self.ctx.regs.new_reg(), Ok)?;
		self.chunk.emit_byte(reg);
		Ok(reg)
	}
	
	fn find_method(&self, ty: Type, prop: &str) -> Result<Option<(u16, u8, Type)>, HissyError> {
		let ns_name = if let Some(ns_name) = ty.get_method_namespace() { ns_name }
			else { return Ok(None); };
		let ns_idx = if let Some(ns_idx) = self.ctx.external.iter().position(|(id, _)| id == &ns_name) { ns_idx }
			else { return Ok(None); };
		let ns_idx = u16::try_from(ns_idx)
			.map_err(|_| error(format!("Too many externals")))?;
		let props = if let Type::Namespace(props) = &self.ctx.external[ns_idx as usize].1 { props }
			else { return Err(error(format!("Namespace name {} for type {:?} is assigned to a non-namespace", ns_name, ty))); };
		let prop_idx = if let Some(prop_idx) = props.iter().position(|(id, _)| id == prop) { prop_idx }
			else { return Ok(None); };
		let prop_idx = u8::try_from(prop_idx)
			.map_err(|_| error_str("Namespace has too many methods"))?;
		let prop_ty = props[prop_idx as usize].1.clone();
		Ok(Some((ns_idx, prop_idx, prop_ty)))
	}
	
	fn find_prop(&mut self, val: Expr, prop: &str) -> Result<(Type, Option<(u8, ObjectProp)>), HissyError> {
		let (val, ty) = self.compile_expr(val, None, None)?;
		
		let prop = self.find_method(ty.clone(), prop)?.map(|(ns_idx, prop_idx, prop_ty)| {
			(val, ObjectProp::Method { ns_idx, prop_idx, prop_ty })
		});
		Ok((ty, prop))
	}
	
	fn compile_arguments(&mut self, fun_ty: Type, mut args: Vec<Expr>) -> Result<(u8, u8, Type), HissyError> {
		let (args_ty, res_ty) = match fun_ty {
			Type::TypedFunction(args_ty, res_ty) => {
				if args_ty.len() != args.len() {
					return Err(error(format!("Expected {} arguments in function call, got {}", args_ty.len(), args.len())))
				}
				(Some(args_ty), res_ty)
			},
			Type::UntypedFunction(res_ty) => (None, res_ty),
			_ => return Err(error(format!("Cannot call non-function type {:?}", fun_ty))),
		};
		let n = u8::try_from(args.len()).map_err(|_| error_str("Too many function arguments"))?;
		let arg_range = self.ctx.regs.new_reg_range(n)?;
		for (i, arg) in args.drain(..).enumerate() {
			let rout = u8::try_from(usize::from(arg_range) + i).unwrap();
			let (_, t) = self.compile_expr(arg, Some(rout), None)?;
			if let Some(args_ty) = &args_ty {
				if !args_ty[i].can_assign(&t) {
					return Err(error(format!("Expected argument of type {:?}, got {:?}", args_ty[i], t)));
				}
			}
		}
		Ok((arg_range, n, *res_ty))
	}
	
	// Compile computation of expr (into dest if given), and returns final register
	// Warning: If no dest is given, do not assume the final register is a new, temporary one,
	// it may be a local or a constant!
	fn compile_expr(&mut self, expr: Expr, dest: Option<u8>, name: Option<String>) -> Result<(u8, Type), HissyError> {
		let mut needs_copy = true;
		
		let (mut reg, ty) = match expr {
			Expr::Nil =>
				(self.chunk.compile_constant(ChunkConstant::Nil)?, prim_ty!(Nil)),
			Expr::Bool(b) =>
				(self.chunk.compile_constant(ChunkConstant::Bool(b))?, prim_ty!(Bool)),
			Expr::Int(i) =>
				(self.chunk.compile_constant(ChunkConstant::Int(i))?, prim_ty!(Int)),
			Expr::Real(r) =>
				(self.chunk.compile_constant(ChunkConstant::Real(r))?, prim_ty!(Real)),
			Expr::String(s) => 
				(self.chunk.compile_constant(ChunkConstant::String(s))?, prim_ty!(String)),
			Expr::Id(s) => {
				let binding = self.ctx.get_binding(&s)?
					.ok_or_else(|| error(format!("Referencing undefined binding '{}'", s)))?;
				match binding {
					Binding::Local(reg, t) => (reg, t),
					Binding::Upvalue(upv, t) => {
						self.chunk.emit_instr(InstrType::GetUp);
						self.chunk.emit_byte(upv);
						needs_copy = false;
						(self.emit_reg(dest)?, t)
					},
					Binding::External(ext_idx, t) => {
						self.chunk.emit_instr(InstrType::GetExt);
						write_u16(&mut self.chunk.code, ext_idx);
						needs_copy = false;
						(self.emit_reg(dest)?, t)
					}
				}
			},
			Expr::BinOp(op, e1, e2) => {
				let (r1, t1) = self.compile_expr(*e1, None, None)?;
				let (r2, t2) = self.compile_expr(*e2, None, None)?;
				self.ctx.regs.free_temp_reg(r2);
				self.ctx.regs.free_temp_reg(r1);
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
				let ty = match op {
					  BinOp::Plus | BinOp::Minus | BinOp::Times | BinOp::Divides
					| BinOp::Modulo | BinOp::Power => {
						if !t1.is_numeric() || !t2.is_numeric() {
							return Err(error(format!("Cannot use numeric operator on {:?} and {:?}", t1, t2)));
						}
						if t1 == prim_ty!(Int) && t2 == prim_ty!(Int) && op != BinOp::Power {
							prim_ty!(Int)
						} else {
							prim_ty!(Real)
						}
					},
					BinOp::LEq | BinOp::GEq | BinOp::Less | BinOp::Greater => {
						if !t1.is_numeric() || !t2.is_numeric() {
							return Err(error(format!("Cannot use comparison operator on {:?} and {:?}", t1, t2)));
						}
						prim_ty!(Bool)
					},
					BinOp::Equal | BinOp::NEq => prim_ty!(Bool),
					BinOp::And | BinOp::Or => {
						if t1 != prim_ty!(Bool) || t2 != prim_ty!(Bool) {
							return Err(error(format!("Cannot compare {:?} and {:?}", t1, t2)));
						}
						prim_ty!(Bool)
					},
				};
				self.chunk.emit_instr(instr);
				self.chunk.emit_byte(r1);
				self.chunk.emit_byte(r2);
				needs_copy = false;
				(self.emit_reg(dest)?, ty)
			},
			Expr::UnaOp(op, e) => {
				let (r, t) = self.compile_expr(*e, dest, None)?;
				self.ctx.regs.free_temp_reg(r);
				let instr = match op {
					UnaOp::Not => InstrType::Not,
					UnaOp::Minus => InstrType::Neg,
				};
				let ty = match op {
					UnaOp::Not => {
						if t != prim_ty!(Bool) {
							return Err(error(format!("Cannot use boolean operator on {:?}", t)));
						}
						prim_ty!(Bool)
					},
					UnaOp::Minus => {
						if !t.is_numeric() {
							return Err(error(format!("Cannot use numeric operator on {:?}", t)));
						}
						t.clone()
					},
				};
				self.chunk.emit_instr(instr);
				self.chunk.emit_byte(r);
				needs_copy = false;
				(self.emit_reg(dest)?, ty)
			},
			Expr::Call(e, args) => {
				if let Expr::Prop(val, prop) = *e { // Try method call shortcut
					match self.find_prop(*val, &prop)? {
						(_ty, Some((val, ObjectProp::Method { ns_idx, prop_idx, prop_ty }))) => {
							let (arg_range, n, res_ty) = self.compile_arguments(prop_ty, args)?;
							self.ctx.regs.free_temp_range(arg_range, n);
							self.ctx.regs.free_temp_reg(val);
							self.chunk.emit_instr(InstrType::CallMethod);
							write_u16(&mut self.chunk.code, ns_idx as u16);
							self.chunk.emit_byte(prop_idx);
							self.chunk.emit_byte(val);
							self.chunk.emit_byte(arg_range);
							self.chunk.emit_byte(n);
							needs_copy = false;
							(self.emit_reg(dest)?, res_ty)
						},
						(ty, None) => return Err(error(format!("Cannot call undefined property {} of type {:?}", prop, ty)))
					}
					
				} else {
					let (func, func_ty) = self.compile_expr(*e, None, None)?;
					let (arg_range, n, res_ty) = self.compile_arguments(func_ty, args)?;
					self.ctx.regs.free_temp_range(arg_range, n);
					self.ctx.regs.free_temp_reg(func);
					self.chunk.emit_instr(InstrType::Call);
					self.chunk.emit_byte(func);
					self.chunk.emit_byte(arg_range);
					self.chunk.emit_byte(n);
					needs_copy = false;
					(self.emit_reg(dest)?, res_ty)
				}
			},
			Expr::Function(args, ret_ty, bl) =>  {
				let ty = resolve_function_type(&args, &ret_ty)?;
				let ret_ty = resolve_type(&ret_ty)?;
				let args: Result<Vec<(String, Type)>, HissyError> = args.iter().map(|(n,t)| Ok((n.clone(), resolve_type(t)?))).collect();
				let args = args?;
				let new_chunk = self.compile_chunk(name.unwrap_or_else(|| String::from("<func>")), bl, args, ret_ty)?;
				self.chunk.emit_instr(InstrType::Func);
				self.chunk.emit_byte(new_chunk);
				needs_copy = false;
				(self.emit_reg(dest)?, ty)
			},
			Expr::List(mut values) => {
				self.chunk.emit_instr(InstrType::ListNew);
				needs_copy = false;
				let reg = self.emit_reg(dest)?;
				
				let mut el_ty: Option<Type> = None;
				
				if !values.is_empty() {
					let n = u8::try_from(values.len()).map_err(|_| error_str("Too many values in list"))?;
					let val_range = self.ctx.regs.new_reg_range(n)?;
					for (i, val) in values.drain(..).enumerate() {
						let rout = u8::try_from(usize::from(val_range) + i).unwrap();
						let (_, ty) = self.compile_expr(val, Some(rout), None)?;
						if let Some(el_ty2) = &el_ty {
							if !el_ty2.can_assign(&ty) {
								if ty.can_assign(&el_ty2) {
									el_ty = Some(ty);
								} else {
									el_ty = Some(Type::Any);
								}
							}
						} else {
							el_ty = Some(ty);
						}
					}
					self.ctx.regs.free_temp_range(val_range, n);
					self.chunk.emit_instr(InstrType::ListExtend);
					self.chunk.emit_byte(reg);
					self.chunk.emit_byte(val_range);
					self.chunk.emit_byte(n);
				}
				
				(reg, Type::List(Box::new(el_ty.unwrap_or(Type::Any))))
			},
			Expr::Index(list, index) => {
				let (list, tl) = self.compile_expr(*list, None, None)?;
				let tr = if let Type::List(tr) = tl { *tr } else {
					return Err(error(format!("Cannot index object of type {:?}", tl)));
				};
				let (index, ti) = self.compile_expr(*index, None, None)?;
				if ti != prim_ty!(Int) {
					return Err(error(format!("Cannot index list with {:?}", ti)));
				}
				self.ctx.regs.free_temp_reg(list);
				self.ctx.regs.free_temp_reg(index);
				self.chunk.emit_instr(InstrType::ListGet);
				self.chunk.emit_byte(list);
				self.chunk.emit_byte(index);
				needs_copy = false;
				(self.emit_reg(dest)?, tr)
			},
			Expr::Prop(val, prop) => {
				let (val, ty) = self.compile_expr(*val, None, None)?;
				
				if let Some((ns_idx, prop_idx, prop_ty)) = self.find_method(ty.clone(), &prop)? {
					self.ctx.regs.free_temp_reg(val);
					self.chunk.emit_instr(InstrType::MakeMethod);
					write_u16(&mut self.chunk.code, ns_idx as u16);
					self.chunk.emit_byte(prop_idx);
					self.chunk.emit_byte(val);
					needs_copy = false;
					(self.emit_reg(dest)?, prop_ty)
				} else {
					return Err(error(format!("Type {:?} does not have a property {}", ty, prop)));
				}
			},
			#[allow(unreachable_patterns)]
			_ => unimplemented!("Unimplemented expression type: {:?}", expr),
		};
		
		if needs_copy {
			if let Some(dest) = dest {
				self.chunk.emit_instr(InstrType::Cpy);
				self.chunk.emit_byte(reg);
				self.chunk.emit_byte(dest);
				reg = dest;
			}
		}
		
		Ok((reg, ty))
	}


	fn compile_block(&mut self, locals: Vec<(String, u8, Type)>, stats: Block) -> Result<u16, HissyError> {
		let used_before = self.ctx.regs.used - (locals.len() as u16);
		
		self.ctx.enter_block();
		for (id, reg, ty) in locals {
			self.ctx.make_local(id, reg, ty);
		}
		
		let mut line = 0;
		for Positioned(stat, (line2, _)) in stats {
			line = u16::try_from(line2).map_err(|_| error_str("Line number too large"))?;
			if self.debug_info {
				let pos = u16::try_from(self.chunk.code.len()).unwrap(); // (The code size is already bounded by the serialization)
				self.chunk.debug_info.line_numbers.push((pos, line));
			}
			
			let compile_stat = || -> Result<(), HissyError> {
				match stat {
					Stat::ExprStat(e) => {
						let (reg, _t) = self.compile_expr(e, None, None)?;
						self.ctx.regs.free_temp_reg(reg);
					},
					Stat::Let(id, ty, e) => {
						let ty = ty.map(|ty| resolve_type(&ty)).transpose()?;
						if let Some(local) = self.ctx.find_block_local(&id) { // if binding already exists
							self.ctx.regs.free_reg(local.reg);
						}
						let reg = self.ctx.regs.new_reg()?;
						let forwarded = {
							if let Expr::Function(args, res_ty, _) = &e {
								self.ctx.make_local(id.clone(), reg, resolve_function_type(args, res_ty)?);
								true
							} else {
								false
							}
						};
						let (_, ty2) = self.compile_expr(e, Some(reg), Some(id.clone()))?;
						let ty = if let Some(ty) = ty {
							if !ty.can_assign(&ty2) {
								return Err(error(format!("Cannot define variable of type {:?} with expression of type {:?}", ty, ty2)));
							}
							ty
						} else {
							ty2
						};
						if !forwarded {
							self.ctx.make_local(id, reg, ty);
						}
					},
					Stat::Set(LExpr::Id(id), e) => {
						let binding = self.ctx.get_binding(&id)?
							.ok_or_else(|| error(format!("Referencing undefined binding '{}'", id)))?;
						let (ty, ty2) = match binding {
							Binding::Local(reg, ty) => {
								let (_, ty2) = self.compile_expr(e, Some(reg), None)?;
								(ty, ty2)
							},
							Binding::Upvalue(upv, ty) => {
								let (reg, ty2) = self.compile_expr(e, None, None)?;
								self.ctx.regs.free_temp_reg(reg);
								self.chunk.emit_instr(InstrType::SetUp);
								self.chunk.emit_byte(upv);
								self.chunk.emit_byte(reg);
								(ty, ty2)
							},
							Binding::External(_, _) => {
								return Err(error(format!("Cannot set external value '{}'", id)));
							},
						};
						if !ty.can_assign(&ty2) {
							return Err(error(format!("Cannot assign type {:?} to variable of type {:?}", ty2, ty)));
						}
					},
					Stat::Set(LExpr::Index(lst, idx), e) => {
						let (lst, tl) = self.compile_expr(*lst, None, None)?;
						let te = if let Type::List(te) = tl { *te } else {
							return Err(error(format!("Cannot index object of type {:?}", tl)));
						};
						let (idx, ti) = self.compile_expr(*idx, None, None)?;
						if ti != prim_ty!(Int) {
							return Err(error(format!("Cannot index list with {:?}", ti)));
						}
						let (e, te2) = self.compile_expr(e, None, None)?;
						if !te.can_assign(&te2) {
							return Err(error(format!("Cannot assign type {:?} into list of {:?}", te2, te)));
						}
						self.ctx.regs.free_temp_reg(lst);
						self.ctx.regs.free_temp_reg(idx);
						self.ctx.regs.free_temp_reg(e);
						self.chunk.emit_instr(InstrType::ListSet);
						self.chunk.emit_byte(lst);
						self.chunk.emit_byte(idx);
						self.chunk.emit_byte(e);
					},
					Stat::Cond(mut branches) => {
						let mut end_jmps = vec![];
						let last_branch = branches.len() - 1;
						for (i, (cond, bl)) in branches.drain(..).enumerate() {
							let mut after_jmp = None;
							match cond {
								Cond::If(e) => {
									let (cond_reg, t) = self.compile_expr(e, None, None)?;
									if t != prim_ty!(Bool) {
										return Err(error(format!("Expected boolean in condition, got {:?}", t)))
									}
									
									// Jump to next branch if false
									self.ctx.regs.free_temp_reg(cond_reg);
									self.chunk.emit_instr(InstrType::Jif);
									after_jmp = Some(self.chunk.code.len());
									self.chunk.emit_byte(0); // Placeholder
									self.chunk.emit_byte(cond_reg);
									
									self.compile_block(vec![], bl)?;
									
									if i != last_branch {
										// Jump out of condition at end of block
										self.chunk.emit_instr(InstrType::Jmp);
										let from2 = self.chunk.code.len();
										self.chunk.emit_byte(0); // Placeholder 2
										end_jmps.push(from2);
									}
								},
								Cond::Else => {
									self.compile_block(vec![], bl)?;
								}
							}
							
							if let Some(from) = after_jmp {
								fill_in_jump_from(&mut self.chunk, from)?;
							}
						}
						
						// Fill in jumps to end
						for from in end_jmps {
							fill_in_jump_from(&mut self.chunk, from)?;
						}
					},
					Stat::While(e, bl) => {
						let begin = self.chunk.code.len();
						let (cond_reg, t) = self.compile_expr(e, None, None)?;
						if t != prim_ty!(Bool) {
							return Err(error(format!("Expected boolean in condition, got {:?}", t)))
						}
						
						self.ctx.regs.free_temp_reg(cond_reg);
						self.chunk.emit_instr(InstrType::Jif);
						let placeholder = self.chunk.code.len();
						self.chunk.emit_byte(0); // Placeholder
						self.chunk.emit_byte(cond_reg);
						
						self.compile_block(vec![], bl)?;
						
						self.chunk.emit_instr(InstrType::Jmp);
						emit_jump_to(&mut self.chunk, begin)?;
						fill_in_jump_from(&mut self.chunk, placeholder)?;
					},
					Stat::For(id, el_ty, e, bl) => {
						let el_ty = el_ty.map(|ty| resolve_type(&ty)).transpose()?;
						
						let res = match self.find_prop(e, "next")? {
							(it_ty, Some((it_reg, ObjectProp::Method { ns_idx, prop_idx, prop_ty: _prop_ty }))) => {
								if let Type::Iterator(el_ty2) = it_ty {
									let el_ty = if let Some(el_ty) = el_ty {
										if !el_ty.can_assign(&el_ty2) {
											return Err(error(format!("Cannot define variable of type {:?} from iterator on type {:?}", el_ty, el_ty2)));
										}
										el_ty
									} else {
										*el_ty2
									};
									
									// Hacky way of making the iterator a "persistent temporary"
									self.ctx.regs.make_local(it_reg);
									let var_reg = self.ctx.regs.new_reg()?;
									
									let begin = self.chunk.code.len();
									self.chunk.emit_instr(InstrType::CallMethod);
									write_u16(&mut self.chunk.code, ns_idx as u16);
									self.chunk.emit_byte(prop_idx);
									self.chunk.emit_byte(it_reg);
									self.chunk.emit_byte(it_reg + 1);
									self.chunk.emit_byte(0);
									self.chunk.emit_byte(var_reg);
									Ok((it_reg, var_reg, el_ty, begin))
								} else {
									Err(it_ty)
								}
							},
							(it_ty, None) => Err(it_ty),
						};
						let (it_reg, var_reg, el_ty, begin) = res.map_err(|ty| error(format!("{:?} is not an iterable type", ty)))?;
						
						self.chunk.emit_instr(InstrType::Jin);
						let placeholder = self.chunk.code.len();
						self.chunk.emit_byte(0); // Placeholder
						self.chunk.emit_byte(var_reg);
						
						self.compile_block(vec![(id, var_reg, el_ty)], bl)?;
						
						self.chunk.emit_instr(InstrType::Jmp);
						emit_jump_to(&mut self.chunk, begin)?;
						
						self.ctx.regs.free_reg(it_reg);
						
						fill_in_jump_from(&mut self.chunk, placeholder)?;
					},
					Stat::Return(e) => {
						let (reg, tr) = self.compile_expr(e, None, None)?;
						if !self.ctx.ret_ty.can_assign(&tr) {
							return Err(error(format!("Trying to return {:?}, expected {:?}", tr, self.ctx.ret_ty)));
						}
						self.ctx.regs.free_temp_reg(reg);
						self.chunk.emit_instr(InstrType::Ret);
						self.chunk.emit_byte(reg);
					},
					#[allow(unreachable_patterns)]
					_ => return Err(error(format!("Unimplemented statement type: {:?}", stat)))
				}
				Ok(())
			};
			
			let mut res = compile_stat();
			if let Err(HissyError(ErrorType::Compilation, err, 0)) = res {
				res = Err(HissyError(ErrorType::Compilation, err, line));
			}
			res?;
		}
		
		self.ctx.leave_block(&mut self.chunk);
		
		assert!(used_before == self.ctx.regs.used, "Leaked registers: {} -> {}", used_before, self.ctx.regs.used);
		// Basic check to make sure no registers have been "leaked"
		
		Ok(line)
	}


	fn compile_chunk(&mut self, name: String, ast: Block, args: Vec<(String, Type)>, ret_ty: Type) -> Result<u8, HissyError> {
		let chunk_id = self.chunk.enter();
		self.ctx.enter(ret_ty);
		
		if self.debug_info {
			self.chunk.debug_info.name = name;
		}
		
		let args: Result<Vec<_>, _> = args.into_iter()
			.map(|(id, ty)| Ok((id, self.ctx.regs.new_reg()?, ty)))
			.collect();
		let args = args?;
		
		let implicit_return = can_reach_end(&ast);
		let last_line = self.compile_block(args, ast)?;
		if implicit_return && !self.ctx.ret_ty.can_assign(&prim_ty!(Nil)) {
			return Err(HissyError(ErrorType::Compilation,
				format!("Implicit nil return at end of function, but expected {:?}", self.ctx.ret_ty),
				last_line));
		}
		
		self.chunk.nb_registers = self.ctx.regs.required;
		self.chunk.upvalues = self.ctx.upvalues.iter().map(|b| b.reg).collect();
		if self.debug_info {
			self.chunk.debug_info.upvalue_names = self.ctx.upvalues.iter().map(|b| b.name.clone()).collect();
		}
		
		self.ctx.leave();
		self.chunk.leave();
		
		u8::try_from(chunk_id).map_err(|_| error_str("Too many chunks"))
	}
	
	/// Compiles a string slice containing Hissy code into a [`Program`], consuming the `Compiler`.
	pub fn compile_program(mut self, input: &str) -> Result<Program, HissyError> {
		let ast = parse(input)?;
		
		self.compile_chunk(String::from("<main>"), ast, Vec::new(), prim_ty!(Nil))?;
		
		Ok(Program { debug_info: self.debug_info, chunks: self.chunk.finish() })
	}
}
