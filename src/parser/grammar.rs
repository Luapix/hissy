
extern crate peg;

use super::lexer::{Token, Tokens};
use super::ast::*;

use peg::str::LineCol;

peg::parser! {
	pub grammar peg_parser() for Tokens {
		
		rule token() -> &'input Token = t:$([_]) { &t[0] }
		
		rule sym(sym: &'static str) = t:token() {?
			match t {
				Token::Symbol(s) if s.as_ref() == sym => Ok(()),
				_ => Err(sym),
			}
		}
		
		rule literal() -> Expr
			= sym("nil") { Expr::Nil }
			/ sym("true") { Expr::Bool(true) }
			/ sym("false") { Expr::Bool(false) }
			/ sym("inf") { Expr::Real(std::f64::INFINITY) }
			/ sym("NaN") { Expr::Real(std::f64::NAN) }
			/ t:token() {?
				match t {
					Token::Id(s) => Ok(Expr::Id(s.clone())),
					Token::Int(i) => Ok(Expr::Int(*i)),
					Token::Real(r) => Ok(Expr::Real(*r)),
					Token::String(s) => Ok(Expr::String(s.clone())),
					_ => Err("literal"),
				}
			}
		
		rule identifier() -> String = t:token() {?
			if let Token::Id(s) = t {
				Ok(s.clone())
			} else {
				Err("identifier")
			}
		}
		
		rule list(pos: &[LineCol]) -> Expr
			= sym("[") values:(expression(pos) ** sym(",")) sym(",")? sym("]") { Expr::List(values) }
		
		rule parenthesized(pos: &[LineCol]) -> Expr = sym("(") e:expression(pos) sym(")") { e }
		
		rule function(pos: &[LineCol]) -> Expr =
			sym("fun") f:function_decl(pos) { f }
		
		rule primary_expression(pos: &[LineCol]) -> Expr
			= literal() / list(pos) / parenthesized(pos) / function(pos)
		
		pub rule expression(pos: &[LineCol]) -> Expr = precedence!{
			x:(@) sym("and") y:@ { Expr::BinOp(BinOp::And, Box::new(x), Box::new(y)) }
			x:(@) sym("or") y:@  { Expr::BinOp(BinOp::Or,  Box::new(x), Box::new(y)) }
			--
			sym("not") x:@ { Expr::UnaOp(UnaOp::Not, Box::new(x)) }
			--
			x:(@) sym("<=") y:@ { Expr::BinOp(BinOp::LEq, Box::new(x), Box::new(y)) }
			x:(@) sym(">=") y:@ { Expr::BinOp(BinOp::GEq, Box::new(x), Box::new(y)) }
			x:(@) sym("<") y:@ { Expr::BinOp(BinOp::Less,    Box::new(x), Box::new(y)) }
			x:(@) sym(">") y:@ { Expr::BinOp(BinOp::Greater, Box::new(x), Box::new(y)) }
			x:(@) sym("==") y:@ { Expr::BinOp(BinOp::Equal, Box::new(x), Box::new(y)) }
			x:(@) sym("!=") y:@ { Expr::BinOp(BinOp::NEq, Box::new(x), Box::new(y)) }
			--
			x:(@) sym("+") y:@ { Expr::BinOp(BinOp::Plus,  Box::new(x), Box::new(y)) }
			x:(@) sym("-") y:@ { Expr::BinOp(BinOp::Minus, Box::new(x), Box::new(y)) }
			--
			sym("-") x:@ { Expr::UnaOp(UnaOp::Minus, Box::new(x)) }
			--
			x:(@) sym("*") y:@ { Expr::BinOp(BinOp::Times,   Box::new(x), Box::new(y)) }
			x:(@) sym("/") y:@ { Expr::BinOp(BinOp::Divides, Box::new(x), Box::new(y)) }
			x:(@) sym("%") y:@ { Expr::BinOp(BinOp::Modulo,  Box::new(x), Box::new(y)) }
			--
			x:@ sym("^") y:(@) { Expr::BinOp(BinOp::Power,   Box::new(x), Box::new(y)) }
			--
			x:@ sym("[") i:expression(pos) sym("]") { Expr::Index(Box::new(x), Box::new(i)) }
			f:@ sym("(") args:(expression(pos) ** sym(",")) sym(",")? sym(")") { Expr::Call(Box::new(f), args) }
			x:@ sym(".") p:identifier() { Expr::Prop(Box::new(x), p) }
			--
			e:primary_expression(pos) { e }
		}
		
		rule type_desc() -> Type
			= t:identifier() { Type::Named(t) }
		rule typed_ident() -> (String, Option<Type>)
			= i:identifier() sym(":") t:type_desc() { (i, Some(t)) }
			/ i:identifier() { (i, None) }
		rule return_type() -> Type
			= sym("->") t:type_desc() { t }
			/ { Type::Named(String::from("Nil")) }
		
		rule function_decl(pos: &[LineCol]) -> Expr
			= sym("(") a:(typed_ident() ** sym(",")) sym(")") r:return_type() b:indented_block(pos) {
				let a = a.iter().map(|(i,t)|
					(i.clone(), t.clone().unwrap_or(Type::Named(String::from("Any"))))
				).collect();
				Expr::Function(a, r, b)
			}
		
		rule if_branch(pos: &[LineCol]) -> Branch = sym("if") c:expression(pos) b:indented_block(pos) { (Cond::If(c), b) }
		rule else_if_branch(pos: &[LineCol]) -> Branch = [Token::Newline] sym("else") b:if_branch(pos) { b }
		rule else_branch(pos: &[LineCol]) -> Branch = [Token::Newline] sym("else") b:indented_block(pos) { (Cond::Else, b) }
		
		rule assignment(pos: &[LineCol]) -> Expr = sym("=") e:expression(pos) { e }
		
		rule statement(pos: &[LineCol]) -> Stat
			= sym("let") i:typed_ident() sym("=") e:expression(pos) { Stat::Let(i.0, i.1, e) }
			/ sym("let") i:identifier() f:function_decl(pos) { Stat::Let(i, None, f) }
			/ i:if_branch(pos) ei:else_if_branch(pos)* e:else_branch(pos)? {
				let mut branches = vec![i];
				branches.extend_from_slice(&ei);
				if let Some(b) = e { branches.push(b) }
				Stat::Cond(branches)
			}
			/ sym("return") e:expression(pos)? { Stat::Return(e.unwrap_or(Expr::Nil)) }
			/ sym("while") e:expression(pos) b:indented_block(pos) { Stat::While(e, b) }
			/ e:expression(pos) a:assignment(pos)? {?
				if let Some(assigned) = a {
					let lexpr = match e {
						Expr::Id(s) => Ok(LExpr::Id(s)),
						Expr::Index(l, i) => Ok(LExpr::Index(l, i)),
						_ => Err("Expected LExpr in assignment"),
					};
					lexpr.map(|lexpr|
						Stat::Set(lexpr, assigned)
					)
				} else {
					Ok(Stat::ExprStat(e))
				}
			}
			/ sym("for") i:typed_ident() sym("in") e:expression(pos) b:indented_block(pos) {
				Stat::For(i.0, i.1, e, b)
			}
		
		rule positioned_statement(pos: &[LineCol]) -> Positioned<Stat>
			= p:position!() s:statement(pos) { Positioned(s, (pos[p].line, pos[p].column)) }
		
		rule block(pos: &[LineCol]) -> Block
			= s:(positioned_statement(pos) ** [Token::Newline]) { s }
		
		rule block_or_pass(pos: &[LineCol]) -> Block
			= sym("pass") { vec![] }
			/ b:block(pos) { b }
		
		rule indented_block(pos: &[LineCol]) -> Block
			= sym(":") [Token::Indent] b:block_or_pass(pos) [Token::Dedent] { b }
		
		pub rule program(pos: &[LineCol]) -> ProgramAST
			= [Token::Newline]? b:block(pos) [Token::Newline]? [Token::EOF] { b }
	}
}
