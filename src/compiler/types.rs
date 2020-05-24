
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveType {
	Nil,
	Bool,
	Int,
	Real,
	String,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Type {
	Primitive(PrimitiveType),
	
	List(Box<Type>),
	TypedFunction(Vec<Type>, Box<Type>),
	UntypedFunction(Box<Type>),
	
	Namespace(Vec<(String, Type)>),
	
	Any,
}

#[macro_export]
macro_rules! prim_ty {
	($x:ident) => { Type::Primitive(PrimitiveType::$x) }
}

impl fmt::Debug for Type {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Type::Primitive(pt) => write!(f, "{:?}", pt),
			Type::List(t) => write!(f, "List<{:?}>", t),
			Type::TypedFunction(args_ty, res_ty) => {
				write!(f, "(")?;
				for (i, arg_ty) in args_ty.iter().enumerate() {
					write!(f, "{:?}", arg_ty)?;
					if i < args_ty.len()-1 {
						write!(f, ", ")?;
					}
				}
				write!(f, ") -> {:?}", res_ty)
			},
			Type::UntypedFunction(res_ty) => write!(f, "(...) -> {:?}", res_ty),
			Type::Namespace(_) => write!(f, "Namespace"),
			Type::Any => write!(f, "Any"),
		}
	}
}

impl Type {
	pub fn is_numeric(&self) -> bool {
		match self {
			prim_ty!(Int) | prim_ty!(Real) => true,
			_ => false,
		}
	}
	
	pub fn can_assign(&self, other: &Type) -> bool {
		match self {
			Type::Primitive(t1) => {
				if let Type::Primitive(t2) = other {
					t1 == t2
				} else {
					false
				}
			},
			Type::List(t1) => {
				if let Type::List(t2) = other {
					t1.can_assign(t2)
				} else {
					false
				}
			},
			Type::TypedFunction(args_ty1, res_ty1) => {
				if let Type::TypedFunction(args_ty2, res_ty2) = other {
					args_ty1.len() == args_ty2.len()
					&& args_ty1.iter().zip(args_ty2).all(|(t1,t2)| t2.can_assign(t1))
					&& res_ty1.can_assign(res_ty2)
				} else {
					false
				}
			},
			Type::UntypedFunction(res_ty1) => {
				let res_ty2 = match other {
					Type::TypedFunction(_, res_ty2) => res_ty2,
					Type::UntypedFunction(res_ty2) => res_ty2,
					_ => { return false; }
				};
				res_ty1.can_assign(res_ty2)
			}
			Type::Namespace(_) => false,
			Type::Any => true,
		}
	}
	
	pub fn get_method_namespace(&self) -> Option<String> {
		match self {
			Type::List(_) => Some(String::from("List")),
			_ => None,
		}
	}
}
