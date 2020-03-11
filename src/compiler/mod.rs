
use std::convert::TryInto;

use super::parser::{parse, ast::{Expr, Stat}};
use super::vm::{chunk::{Chunk, ChunkConstant}, InstrType};

pub struct Compiler {
	next_free_reg: u16,
}

impl Compiler {
	pub fn new() -> Compiler {
		Compiler { next_free_reg: 0 }
	}
	
	fn insert_free_reg(&mut self, chunk: &mut Chunk) {
		chunk.emit_byte(self.next_free_reg.try_into()
			.expect("Cannot compile: Too many registers required"));
		self.next_free_reg += 1;
	}
	
	fn compile_constant(&mut self, chunk: &mut Chunk, val: ChunkConstant) {
		chunk.constants.push(val);
		chunk.emit_instr(InstrType::Cst);
		chunk.emit_byte((chunk.constants.len() - 1).try_into()
			.expect("Cannot compile: Too many constants required"));
		self.insert_free_reg(chunk);
	}
	
	fn compile_expr(&mut self, chunk: &mut Chunk, expr: Expr) -> u8 {
		match expr {
			Expr::Nil => {
				chunk.emit_instr(InstrType::Nil);
				self.insert_free_reg(chunk);
			},
			Expr::Bool(b) => {
				chunk.emit_instr(if b {InstrType::True} else {InstrType::False});
				self.insert_free_reg(chunk);
			},
			Expr::Int(i) =>
				self.compile_constant(chunk, ChunkConstant::Int(i)),
			Expr::Real(r) =>
				self.compile_constant(chunk, ChunkConstant::Real(r)),
			Expr::String(s) => 
				self.compile_constant(chunk, ChunkConstant::Str(s)),
			_ => unimplemented!("Unimplemented expression type: {:?}", expr),
		}
		(self.next_free_reg - 1) as u8
	}
	
	pub fn compile_chunk(&mut self, input: &str) -> Result<Chunk, String> {
		let ast = parse(input)?;
		let mut chunk = Chunk::new();
		for stat in ast {
			match stat {
				Stat::Return(e) => {
					let reg = self.compile_expr(&mut chunk, e);
					chunk.emit_instr(InstrType::Log); // Temp
					chunk.emit_byte(reg);
				},
				_ => unimplemented!("Unimplemented instruction type: {:?}", stat),
			}
		}
		Ok(chunk)
	}
}
