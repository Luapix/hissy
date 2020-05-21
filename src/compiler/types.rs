
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
	
	pub fn can_assign(&self, other: &Type) -> bool {
		if let Type::Any = self {
			return true;
		}
		match self {
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
			Type::Any => true,
			_ => self == other,
		}
	}
}
