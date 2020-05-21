
use std::fmt;
use std::ops::Deref;

/// A binary operator.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum BinOp {
	Plus, Minus,
	Times, Divides, Modulo,
	Power,
	LEq, GEq, Less, Greater,
	Equal, NEq,
	And, Or,
}

/// A unary operator.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum UnaOp {
	Not,
	Minus,
}

/// An expression (literals and operations).
#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
	Nil,
	Bool(bool),
	Int(i32),
	Real(f64),
	String(String),
	Id(String),
	
	List(Vec<Expr>),
	BinOp(BinOp, Box<Expr>, Box<Expr>),
	UnaOp(UnaOp, Box<Expr>),
	Index(Box<Expr>, Box<Expr>),
	Call(Box<Expr>, Vec<Expr>),
	Prop(Box<Expr>, String),
	Function(Vec<(String, Type)>, Block),
}

/// The guard on a condition branch (else / else if).
#[derive(Debug, PartialEq, Clone)]
pub enum Cond {
	If(Expr),
	Else,
}

/// A branch of a condition (condition + block).
pub type Branch = (Cond, Block);

/// A type description.
#[derive(Debug, PartialEq, Clone)]
pub enum Type {
	Any,
	Named(String),
	Function(Vec<Type>, Box<Type>),
}

/// The left-hand side of an assignment
#[derive(Debug, PartialEq, Clone)]
pub enum LExpr {
	Id(String),
	Index(Box<Expr>, Box<Expr>),
}

/// A statement.
#[derive(Debug, PartialEq, Clone)]
pub enum Stat {
	ExprStat(Expr),
	Let((String, Type), Expr),
	Set(LExpr, Expr),
	Cond(Vec<Branch>),
	While(Expr, Block),
	Return(Expr),
}

/// A token with an associated positioned line number
#[derive(PartialEq, Clone)]
pub struct Positioned<T>(pub T, pub (usize, usize));

impl<T> Deref for Positioned<T> {
	type Target = T;
	fn deref(&self) -> &T { &self.0 }
}

impl<T: fmt::Debug> fmt::Debug for Positioned<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:#?} @ {}:{}", self.0, (self.1).0, (self.1).1)
	}
}

pub type Block = Vec<Positioned<Stat>>;

/// A Hissy program.
pub type ProgramAST = Block;
