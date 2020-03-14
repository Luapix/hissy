
use std::collections::HashSet;
use std::convert::TryInto;

use super::parser::{parse, ast::{Expr, Stat, BinOp, UnaOp}};
use super::vm::{chunk::{Chunk, ChunkConstant}, InstrType};

pub struct Compiler {
	reg_cnt: u16,
	next_free_reg: u16,
	used_registers: HashSet<u8>,
}

impl Compiler {
	pub fn new() -> Compiler {
		Compiler { reg_cnt: 0, next_free_reg: 0, used_registers: HashSet::new() }
	}
	
	fn new_reg(&mut self) -> u8 {
		let new_reg = self.next_free_reg.try_into()
			.expect("Cannot compile: Too many registers required");
		if new_reg as u16 + 1 > self.reg_cnt {
			self.reg_cnt = new_reg as u16 + 1;
		}
		self.used_registers.insert(new_reg);
		while self.next_free_reg.try_into().map_or(false, |i: u8| self.used_registers.contains(&i)) {
			self.next_free_reg += 1;
		}
		new_reg
	}
	
	fn emit_new_reg(&mut self, chunk: &mut Chunk) -> u8 {
		let new_reg = self.new_reg();
		chunk.emit_byte(new_reg);
		new_reg
	}
	
	fn free_reg(&mut self, i: u8) {
		self.used_registers.remove(&i);
		if (i as u16) < self.next_free_reg {
			self.next_free_reg = i as u16;
		}
	}
	
	fn compile_constant(&mut self, chunk: &mut Chunk, val: ChunkConstant) -> u8 {
		chunk.constants.push(val);
		chunk.emit_instr(InstrType::Cst);
		chunk.emit_byte((chunk.constants.len() - 1).try_into()
			.expect("Cannot compile: Too many constants required"));
		self.emit_new_reg(chunk)
	}
	
	fn compile_expr(&mut self, chunk: &mut Chunk, expr: &Expr) -> u8 {
		match expr {
			Expr::Nil => {
				chunk.emit_instr(InstrType::Nil);
				self.emit_new_reg(chunk)
			},
			Expr::Bool(b) => {
				chunk.emit_instr(if *b {InstrType::True} else {InstrType::False});
				self.emit_new_reg(chunk)
			},
			Expr::Int(i) =>
				self.compile_constant(chunk, ChunkConstant::Int(*i)),
			Expr::Real(r) =>
				self.compile_constant(chunk, ChunkConstant::Real(*r)),
			Expr::String(s) => 
				self.compile_constant(chunk, ChunkConstant::Str(s.clone())),
			
			Expr::BinOp(op, e1, e2) => {
				let r1 = self.compile_expr(chunk, &e1);
				let r2 = self.compile_expr(chunk, &e2);
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
				self.free_reg(r1); // Reuse r1/r2 for result
				self.free_reg(r2);
				self.emit_new_reg(chunk)
			},
			Expr::UnaOp(op, e) => {
				let r = self.compile_expr(chunk, &e);
				let instr = match op {
					UnaOp::Not => InstrType::Not,
					UnaOp::Minus => InstrType::Neg,
				};
				chunk.emit_instr(instr);
				chunk.emit_byte(r);
				self.free_reg(r); // Reuse r for result
				self.emit_new_reg(chunk)
			}
			
			_ => unimplemented!("Unimplemented expression type: {:?}", expr),
		}
	}
	
	pub fn compile_chunk(&mut self, input: &str) -> Result<Chunk, String> {
		let ast = parse(input)?;
		let mut chunk = Chunk::new();
		for stat in ast {
			match stat {
				Stat::Return(e) => {
					let reg = self.compile_expr(&mut chunk, &e);
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
