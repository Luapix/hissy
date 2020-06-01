
use std::convert::TryFrom;
use std::cell::RefCell;
use std::iter::Iterator;

use crate::{prim_ty, HissyError, ErrorType};
use crate::compiler::{Type, PrimitiveType};
use crate::vm::{gc::{GCHeap, GCRef}, value::{Value, NIL}, object::{NativeFunction, List, Namespace, IteratorWrapper}};

fn error(s: String) -> HissyError {
	HissyError(ErrorType::Execution, s, 0)
}
fn error_str(s: &str) -> HissyError {
	error(String::from(s))
}

pub fn list() -> Vec<(String, Type)> {
	vec![
		(String::from("List"), Type::Namespace(vec![
			(String::from("size"), Type::TypedFunction(vec![], Box::new(prim_ty!(Int)))),
			(String::from("add"), Type::TypedFunction(vec![Type::Any], Box::new(prim_ty!(Nil)))),
		])),
		(String::from("Iterator"), Type::Namespace(vec![
			(String::from("next"), Type::TypedFunction(vec![], Box::new(Type::Any))),
		])),
		(String::from("log"), Type::UntypedFunction(Box::new(prim_ty!(Nil)))),
		(String::from("range"), Type::TypedFunction(vec![prim_ty!(Int), prim_ty!(Int)], Box::new(Type::Iterator(Box::new(prim_ty!(Int)))))),
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
	res.push(heap.make_value(
		Namespace(vec![ list_size, list_add ])
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
	
	res
}
