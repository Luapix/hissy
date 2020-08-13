
use std::convert::TryFrom;
use std::cell::RefCell;
use std::iter::Iterator;

use crate::{prim_ty, HissyError, ErrorType};
use crate::compiler::{Type, PrimitiveType};
use crate::vm::gc::{GCHeap, GCRef};
use crate::vm::value::{Value, NIL};
use crate::vm::object::{NativeFunction, List, Namespace, IteratorWrapper, VecIterator};

fn error(s: String) -> HissyError {
	HissyError(ErrorType::Execution, s, 0)
}

pub fn list() -> Vec<(String, Type)> {
	vec![
		(String::from("List"), Type::Namespace(vec![
			(String::from("size"), Type::TypedFunction(vec![], Box::new(prim_ty!(Int)))),
			(String::from("add"), Type::TypedFunction(vec![Type::Any], Box::new(prim_ty!(Nil)))),
			(String::from("iter"), Type::TypedFunction(vec![], Box::new(Type::Iterator(Box::new(Type::Any))))),
		])),
		(String::from("Iterator"), Type::Namespace(vec![
			(String::from("next"), Type::TypedFunction(vec![], Box::new(Type::Any))),
		])),
		(String::from("log"), Type::UntypedFunction(Box::new(prim_ty!(Nil)))),
		(String::from("range"), Type::TypedFunction(vec![prim_ty!(Int), prim_ty!(Int)], Box::new(Type::Iterator(Box::new(prim_ty!(Int)))))),
		(String::from("int"), Type::TypedFunction(vec![Type::Any], Box::new(prim_ty!(Int)))),
		(String::from("string"), Type::TypedFunction(vec![Type::Any], Box::new(prim_ty!(String)))),
	]
}

pub fn create(heap: &mut GCHeap) -> Vec<Value> {
	let mut res = vec![];
	
	let list_size = heap.make_value(NativeFunction::new(|_heap, args| {
		let this = GCRef::<List>::try_from(args[0].clone()).unwrap();
		Ok(Value::from(this.len() as i32))
	}));
	let list_add = heap.make_value(NativeFunction::new(|_heap, args| {
		let this = GCRef::<List>::try_from(args[0].clone()).unwrap();
		this.extend(&[ args[1].clone() ]);
		Ok(NIL)
	}));
	let list_iter = heap.make_value(NativeFunction::new(|heap, args| {
		let this = GCRef::<List>::try_from(args[0].clone()).unwrap();
		Ok(heap.make_value(IteratorWrapper {
			iter: Box::new(RefCell::new(
				VecIterator::new(this.get_copy())
			))
		}))
	}));
	res.push(heap.make_value(
		Namespace(vec![ list_size, list_add, list_iter ])
	));
	
	let iter_next = heap.make_value(NativeFunction::new(|heap, args| {
		let this = GCRef::<IteratorWrapper>::try_from(args[0].clone()).unwrap();
		Ok(this.next(heap).unwrap_or(NIL))
	}));
	res.push(heap.make_value(
		Namespace(vec![ iter_next ])
	));
	
	res.push(heap.make_value(
		NativeFunction::new(|_heap, args| {
			let mut it = args.iter();
			if let Some(val) = it.next() {
				print!("{}", val.repr());
				for val in it {
					print!(" {}", val.repr());
				}
			}
			println!();
			Ok(NIL)
		})
	));
	
	res.push(heap.make_value(
		NativeFunction::new(|heap, args| {
			if args.len() != 2 {
				return Err(error(format!("Expected 2 arguments, got {}", args.len())));
			}
			let start = i32::try_from(&args[0]).unwrap();
			let end = i32::try_from(&args[1]).unwrap();
			
			Ok(heap.make_value(IteratorWrapper {
				iter: Box::new(RefCell::new(
					(start..end).map(Value::from)
				))
			}))
		})
	));
	
	res.push(heap.make_value(
		NativeFunction::new(|_heap, args| {
			if args.len() != 1 {
				return Err(error(format!("Expected 1 argument, got {}", args.len())));
			}
			if i32::try_from(&args[0]).is_ok() {
				Ok(args[0].clone())
			} else {
				Err(error(format!("Expected integer value, got {:?}", &args[0])))
			}
		})
	));
	res.push(heap.make_value(
		NativeFunction::new(|_heap, args| {
			if args.len() != 1 {
				return Err(error(format!("Expected 1 argument, got {}", args.len())));
			}
			if GCRef::<String>::try_from(args[0].clone()).is_ok() {
				Ok(args[0].clone())
			} else {
				Err(error(format!("Expected string value, got {:?}", &args[0])))
			}
		})
	));
	
	res
}
