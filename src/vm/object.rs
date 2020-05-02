
use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::fmt;

use crate::HissyError;
use super::value::Value;
use super::gc::{Traceable, GC, GCRef};

impl Traceable for String {}

impl Traceable for Vec<Value> {
	fn touch(&self, initial: bool) {
		for el in self {
			el.touch(initial);
		}
	}
}

impl<T: GC> Traceable for Vec<GCRef<T>> {
	fn touch(&self, initial: bool) {
		for el in self {
			el.touch(initial);
		}
	}
}

#[derive(Clone)]
pub(super) enum UpvalueData {
	OnStack(usize),
	OnHeap(Value),
}

pub(super) struct Upvalue(RefCell<UpvalueData>);

impl Upvalue {
	pub fn new(stack_idx: usize) -> Upvalue {
		Upvalue(RefCell::new(UpvalueData::OnStack(stack_idx)))
	}
	
	pub fn get(&self) -> UpvalueData {
		self.0.borrow().clone()
	}
	
	pub fn set_inside(&self, val: Value) {
		val.touch(true);
		self.0.replace(UpvalueData::OnHeap(val));
	}
}

impl Traceable for Upvalue {
	fn touch(&self, initial: bool) {
		if let UpvalueData::OnHeap(val) = self.0.borrow().deref() { val.touch(initial); }
	}
}

impl fmt::Debug for Upvalue {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
		let ty = match self.0.borrow().deref() {
			UpvalueData::OnStack(_) => "open",
			UpvalueData::OnHeap(_) => "closed",
		};
		write!(f, "<{} upvalue>", ty)
	}
}


pub(super) struct Closure {
	pub chunk_id: u8,
	pub upvalues: Vec<GCRef<Upvalue>>,
}

impl Closure {
	pub fn new(chunk_id: u8, upvalues: Vec<GCRef<Upvalue>>) -> Closure {
		Closure { chunk_id, upvalues }
	}
}

impl Traceable for Closure {
	fn touch(&self, initial: bool) { self.upvalues.touch(initial); }
}

impl fmt::Debug for Closure {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "<function>")
	}
}


pub struct NativeFunction {
	pub fun: Box<RefCell<dyn FnMut(Vec<Value>) -> Result<Value, HissyError>>>
}

impl NativeFunction {
	pub(crate) fn new(fun: impl FnMut(Vec<Value>) -> Result<Value, HissyError> + 'static) -> NativeFunction {
		NativeFunction {
			fun: Box::new(RefCell::new(fun)),
		}
	}
	
	pub fn call(&self, args: Vec<Value>) -> Result<Value, HissyError> {
		self.fun.borrow_mut().deref_mut()(args)
	}
}

impl Traceable for NativeFunction {}

impl fmt::Debug for NativeFunction {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "<function>")
	}
}


#[cfg(test)]
mod tests {
	#![allow(clippy::blacklisted_name)]
	
	use super::super::gc::GCHeap;
	
	#[test]
	fn test_vec_ref() {
		let mut heap = GCHeap::new();
		let foo = heap.make_ref(String::from("foo"));
		let bar = heap.make_ref(String::from("bar"));
		heap.collect();
		heap.inspect();
		{
			let _l = heap.make_ref(vec![foo, bar]);
			heap.collect();
			heap.inspect();
		}
		heap.collect();
		heap.inspect();
		
		{
			let _l;
			{
				let foo = heap.make_ref(String::from("foo"));
				let bar = heap.make_ref(String::from("bar"));
				heap.collect();
				heap.inspect();
				_l = heap.make_ref(vec![foo, bar]);
				heap.collect();
				heap.inspect();
			}
			heap.collect();
			heap.inspect();
		}
		heap.collect();
		heap.inspect();
		assert!(heap.is_empty());
	}
	
	#[test]
	fn test_vec_val() {
		let mut heap = GCHeap::new();
		let foo = heap.make_value(String::from("foo"));
		let bar = heap.make_value(String::from("bar"));
		heap.collect();
		heap.inspect();
		{
			let _l = heap.make_value(vec![foo, bar]);
			heap.collect();
			heap.inspect();
		}
		heap.collect();
		heap.inspect();
		
		{
			let _l;
			{
				let foo = heap.make_value(String::from("foo"));
				let bar = heap.make_value(String::from("bar"));
				heap.collect();
				heap.inspect();
				_l = heap.make_value(vec![foo, bar]);
				heap.collect();
				heap.inspect();
			}
			heap.collect();
			heap.inspect();
		}
		heap.collect();
		heap.inspect();
		assert!(heap.is_empty());
	}
}
