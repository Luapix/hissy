
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
	Nil,
	Bool,
	Int,
	Real,
	String,
	
	List(Box<Type>),
	TypedFunction(Vec<Type>, Box<Type>),
	UntypedFunction(Box<Type>),
	
	Any,
}

impl Type {
	pub fn is_numeric(&self) -> bool {
		match self {
			Type::Int | Type::Real => true,
			_ => false,
		}
	}
}
