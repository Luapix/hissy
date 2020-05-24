
use std::convert::TryFrom;

use crate::prim_ty;
use crate::compiler::{Type, PrimitiveType};
use crate::vm::{gc::{GCHeap, GCRef}, value::{Value, NIL}, object::{NativeFunction, List, Namespace}};

pub fn list() -> Vec<(String, Type)> {
	vec![
		(String::from("List"), Type::Namespace(vec![
			(String::from("size"), Type::TypedFunction(vec![], Box::new(prim_ty!(Int)))),
			(String::from("add"), Type::TypedFunction(vec![Type::Any], Box::new(prim_ty!(Nil)))),
		])),
		(String::from("log"), Type::UntypedFunction(Box::new(prim_ty!(Nil))))
	]
}

pub fn create(heap: &mut GCHeap) -> Vec<Value> {
	let mut res = vec![];
	
	let list_size = heap.make_value(NativeFunction::new(|args| {
		let this = GCRef::<List>::try_from(args[0].clone()).unwrap();
		Ok(Value::from(this.len() as i32))
	}));
	let list_add = heap.make_value(NativeFunction::new(|args| {
		let this = GCRef::<List>::try_from(args[0].clone()).unwrap();
		this.extend(&[ args[1].clone() ]);
		Ok(NIL)
	}));
	res.push(heap.make_value(
		Namespace(vec![ list_size, list_add ])
	));
	
	res.push(heap.make_value(
		NativeFunction::new(|args| {
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
	res
}
