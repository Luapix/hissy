
use std::collections::HashMap;
use std::convert::TryInto;

use super::parser::{parse, ast::{Expr, Stat, BinOp, UnaOp}};
use super::vm::{chunk::{Chunk, ChunkConstant}, InstrType};

#[derive(Debug, PartialEq, Eq)]
pub enum RegContent {
	Temp,
	Local,
}

pub struct Compiler {
	reg_cnt: u16,
	next_free_reg: u16,
	used_registers: HashMap<u8, RegContent>,
	locals: HashMap<String, u8>,
}

impl Compiler {
	pub fn new() -> Compiler {
		Compiler {
			reg_cnt: 0,
			next_free_reg: 0,
			used_registers: HashMap::new(),
			locals: HashMap::new()
		}
	}
	
	fn new_reg(&mut self) -> u8 {
		let new_reg = self.next_free_reg.try_into()
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
	fn emit_reg(&mut self, chunk: &mut Chunk, dest: Option<u8>) -> u8 {
		let reg = dest.map_or_else(|| self.new_reg(), |r| r);
		chunk.emit_byte(reg);
		reg
	}
	
	// Marks register as freed if temporary
	fn free_temp_reg(&mut self, i: u8) {
		if self.used_registers[&i] == RegContent::Temp {
			self.used_registers.remove(&i);
			if (i as u16) < self.next_free_reg {
				self.next_free_reg = i as u16;
			}
		}
	}
	
	// Compile loading of ChunkConstant into dest
	// Returns final register
	fn compile_constant(&mut self, chunk: &mut Chunk, val: ChunkConstant, dest: Option<u8>) -> u8 {
		chunk.constants.push(val);
		chunk.emit_instr(InstrType::Cst);
		chunk.emit_byte((chunk.constants.len() - 1).try_into()
			.expect("Too many constants required"));
		self.emit_reg(chunk, dest)
	}
	
	// Compile computation of expr into dest
	// Returns final register
	// Warning: Do not assume final register is a temporary, it may be a local!
	fn compile_expr(&mut self, chunk: &mut Chunk, expr: &Expr, dest: Option<u8>) -> u8 {
		match expr {
			Expr::Nil => {
				chunk.emit_instr(InstrType::Nil);
				self.emit_reg(chunk, dest)
			},
			Expr::Bool(b) => {
				chunk.emit_instr(if *b {InstrType::True} else {InstrType::False});
				self.emit_reg(chunk, dest)
			},
			Expr::Int(i) =>
				self.compile_constant(chunk, ChunkConstant::Int(*i), dest),
			Expr::Real(r) =>
				self.compile_constant(chunk, ChunkConstant::Real(*r), dest),
			Expr::String(s) => 
				self.compile_constant(chunk, ChunkConstant::Str(s.clone()), dest),
			Expr::Id(s) => {
				let src = *self.locals.get(s).expect("Referencing undefined local");
				match dest {
					Some(dest) if dest != src => {
						chunk.emit_instr(InstrType::Cpy);
						chunk.emit_byte(src);
						chunk.emit_byte(dest);
						dest
					},
					_ => src
				}
			},
			Expr::BinOp(op, e1, e2) => {
				let r1 = self.compile_expr(chunk, &e1, None);
				let r2 = self.compile_expr(chunk, &e2, None);
				self.free_temp_reg(r1);
				self.free_temp_reg(r2);
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
				self.emit_reg(chunk, dest)
			},
			Expr::UnaOp(op, e) => {
				let r = self.compile_expr(chunk, &e, dest);
				self.free_temp_reg(r);
				let instr = match op {
					UnaOp::Not => InstrType::Not,
					UnaOp::Minus => InstrType::Neg,
				};
				chunk.emit_instr(instr);
				chunk.emit_byte(r);
				self.emit_reg(chunk, dest)
			}
			
			_ => unimplemented!("Unimplemented expression type: {:?}", expr),
		}
	}
	
	pub fn compile_chunk(&mut self, input: &str) -> Result<Chunk, String> {
		let ast = parse(input)?;
		let mut chunk = Chunk::new();
		for stat in ast {
			match stat {
				Stat::Let((id, _ty), e) => {
					let reg = self.new_reg();
					self.compile_expr(&mut chunk, &e, Some(reg));
					self.locals.insert(id, reg);
					self.used_registers.insert(reg, RegContent::Local);
				},
				Stat::Set(id, e) => {
					let reg = *self.locals.get(&id).expect("Referencing undefined local");
					self.compile_expr(&mut chunk, &e, Some(reg));
				},
				Stat::Return(e) => {
					let reg = self.compile_expr(&mut chunk, &e, None);
					chunk.emit_instr(InstrType::Log); // Temp
					chunk.emit_byte(reg);
				},
				_ => unimplemented!("Unimplemented instruction type: {:?}", stat),
			}
		}
		chunk.nb_registers = self.reg_cnt;
		Ok(chunk)
	}
}
