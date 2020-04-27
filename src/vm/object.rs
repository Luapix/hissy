
use std::cell::RefCell;
use std::ops::Deref;
use std::fmt;

use super::value::Value;
use super::gc::{Traceable, GC, GCRef};

impl Traceable for String {
	fn mark(&self) {}
	fn unroot(&mut self) {}
}

impl Traceable for Vec<Value> {
	fn unroot(&mut self) {
		for el in self {
			el.unroot();
		}
	}
	
	fn mark(&self) {
		for el in self {
			el.mark();
		}
	}
}

impl<T: GC> Traceable for Vec<GCRef<T>> {
	fn unroot(&mut self) {
		for el in self {
			el.unroot();
		}
	}
	
	fn mark(&self) {
		for el in self {
			el.mark();
		}
	}
}

#[derive(Clone)]
pub enum UpvalueData {
	OnStack(usize),
	OnHeap(Value),
}

pub struct Upvalue(RefCell<UpvalueData>, String);

impl Upvalue {
	pub fn new(stack_idx: usize, name: String) -> Upvalue {
		Upvalue(RefCell::new(UpvalueData::OnStack(stack_idx)), name)
	}
	
	pub fn get(&self) -> UpvalueData {
		self.0.borrow().clone()
	}
	
	pub fn set_inside(&self, mut val: Value) {
		val.unroot();
		self.0.replace(UpvalueData::OnHeap(val));
	}
}

impl Traceable for Upvalue {
	fn mark(&self) {
		if let UpvalueData::OnHeap(val) = self.0.borrow().deref() { val.mark(); }
	}
	fn unroot(&mut self) {
		if let UpvalueData::OnHeap(val) = self.0.get_mut() { val.unroot(); }
	}
}

impl fmt::Debug for Upvalue {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
		let ty = match self.0.borrow().deref() {
			UpvalueData::OnStack(_) => "open",
			UpvalueData::OnHeap(_) => "closed",
		};
		write!(f, "<{} upvalue {}>", ty, self.1)
	}
}


pub struct Closure {
	pub chunk_id: u8,
	pub chunk_name: String,
	pub upvalues: Vec<GCRef<Upvalue>>,
}

impl Closure {
	pub fn new(chunk_id: u8, chunk_name: String, upvalues: Vec<GCRef<Upvalue>>) -> Closure {
		Closure { chunk_id, chunk_name, upvalues }
	}
}

impl Traceable for Closure {
	fn mark(&self) { self.upvalues.mark(); }
	fn unroot(&mut self) { self.upvalues.unroot(); }
}

impl fmt::Debug for Closure {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "<closure {}>", self.chunk_name)
	}
}


#[cfg(test)]
mod tests {
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
				_l = heap.make_ref(vec![foo.clone(), bar.clone()]);
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
				_l = heap.make_value(vec![foo.clone(), bar.clone()]);
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
