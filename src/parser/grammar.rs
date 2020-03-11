
extern crate peg;

use super::lexer::{Token, Tokens};
use super::ast::*;

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
		
		rule list() -> Expr
			= sym("[") values:(expression() ** sym(",")) sym("]") { Expr::List(values) }
		
		rule parenthesized() -> Expr = sym("(") e:expression() sym(")") { e }
		
		rule primary_expression() -> Expr
			= literal() / list() / parenthesized()
		
		pub rule expression() -> Expr = precedence!{
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
			x:(@) sym("*") y:@ { Expr::BinOp(BinOp::Times,   Box::new(x), Box::new(y)) }
			x:(@) sym("/") y:@ { Expr::BinOp(BinOp::Divides, Box::new(x), Box::new(y)) }
			x:(@) sym("%") y:@ { Expr::BinOp(BinOp::Modulo,  Box::new(x), Box::new(y)) }
			--
			x:@ sym("^") y:(@) { Expr::BinOp(BinOp::Power,   Box::new(x), Box::new(y)) }
			--
			x:@ sym("[") i:expression() sym("]") { Expr::Index(Box::new(x), Box::new(i)) }
			f:@ sym("(") args:(expression() ** sym(",")) sym(")") { Expr::Call(Box::new(f), args) }
			x:@ sym(".") p:identifier() { Expr::Prop(Box::new(x), p) }
			--
			e:primary_expression() { e }
		}
		
		rule type_desc() -> Type
			= t:identifier() { Type::Named(t) }
		rule typed_ident() -> (String, Type)
			= i:identifier() sym(":") t:type_desc() { (i, t) }
			/ i:identifier() { (i, Type::Any) }
		rule return_type() -> Type
			= sym("->") t:type_desc() { t }
			/ { Type::Any }
		
		rule function_decl() -> (Expr, Type)
			= sym("(") a:typed_ident()* sym(")") r:return_type() b:indented_block() {
				let (arg_names, arg_types) = a.iter().cloned().unzip();
				(Expr::Function(arg_names, b), Type::Function(arg_types, Box::new(r)))
			}
		
		rule if_branch() -> Branch = sym("if") c:expression() b:indented_block() { (Cond::If(c), b) }
		rule else_if_branch() -> Branch = [Token::Newline] sym("else") b:if_branch() { b }
		rule else_branch() -> Branch = [Token::Newline] sym("else") b:indented_block() { (Cond::Else, b) }
		
		rule statement() -> Stat
			= sym("let") i:typed_ident() sym("=") e:expression() { Stat::Let(i,e) }
			/ sym("let") i:identifier() f:function_decl() {
				let (fn_expr, fn_type) = f;
				Stat::Let((i, fn_type), fn_expr)
			}
			/ i:if_branch() ei:else_if_branch()* e:else_branch()? {
				let mut branches = vec![i];
				branches.extend_from_slice(&ei);
				if let Some(b) = e { branches.push(b) }
				Stat::Cond(branches)
			}
			/ sym("return") e:expression() { Stat::Return(e) }
			/ sym("while") e:expression() b:indented_block() { Stat::While(e, b) }
			/ i:identifier() sym("=") e:expression() { Stat::Set(i,e) }
			/ e:expression() { Stat::ExprStat(e) }
		
		rule block() -> Vec<Stat>
			= s:(statement() ** [Token::Newline]) { s }
		
		rule indented_block() -> Vec<Stat>
			= sym(":") [Token::Indent] b:block() [Token::Dedent] { b }
		
		pub rule program() -> Program
			= [Token::Newline] b:block() [Token::Newline]? { b }
	}
}
