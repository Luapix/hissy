
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum BinOp {
	Plus, Minus,
	Times, Divides, Modulo,
	Power,
	LEq, GEq, Less, Greater,
	Equal, NEq,
	And, Or,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum UnaOp {
	Not,
	Minus,
}

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
	Function(Vec<String>, Vec<Stat>),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Cond {
	If(Expr),
	Else,
}

pub type Branch = (Cond, Vec<Stat>);

#[derive(Debug, PartialEq, Clone)]
pub enum Type {
	Any,
	Named(String),
	Function(Vec<Type>, Box<Type>),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Stat {
	ExprStat(Expr),
	Let((String, Type), Expr),
	Set(String, Expr),
	Cond(Vec<Branch>),
	While(Expr, Vec<Stat>),
	Log(Expr),
	Return(Expr),
}

pub type ProgramAST = Vec<Stat>;
