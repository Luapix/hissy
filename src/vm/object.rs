
use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::fmt;

use crate::{HissyError, ErrorType};
use super::value::Value;
use super::gc::{Traceable, GC, GCRef};


fn error(s: String) -> HissyError {
	HissyError(ErrorType::Execution, s, 0)
}


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
		if let UpvalueData::OnHeap(val) = self.0.borrow().deref() {
			val.touch(initial);
		}
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
	fn touch(&self, initial: bool) {
		self.upvalues.touch(initial);
	}
}

impl fmt::Debug for Closure {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "<function>")
	}
}


pub type HissyFun = dyn FnMut(Vec<Value>) -> Result<Value, HissyError>;

pub struct NativeFunction {
	pub fun: Box<RefCell<HissyFun>>
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


#[derive(Default)]
pub struct List {
	data: RefCell<Vec<Value>>
}

impl List {
	pub fn new() -> List {
		List::default()
	}
	
	pub fn len(&self) -> usize {
		self.data.borrow().len()
	}
	
	pub fn extend(&self, values: &[Value]) {
		let mut data = self.data.borrow_mut();
		let start = data.len();
		data.extend(values.iter().cloned());
		for i in start..data.len() {
			data[i].touch(true);
		}
	}
	
	pub fn get(&self, idx: usize) -> Result<Value, HissyError> {
		self.data.borrow().get(idx).cloned()
			.ok_or_else(|| error(format!("Can't get value at index {} in list of length {}", idx, self.len())))
	}
	
	pub fn set(&self, idx: usize, val: Value) -> Result<(), HissyError> {
		let mut data = self.data.borrow_mut();
		let val2 = data.get_mut(idx)
			.ok_or_else(|| error(format!("Can't set value at index {} in list of length {}", idx, self.len())))?;
		*val2 = val;
		Ok(())
	}
}

impl Traceable for List {
	fn touch(&self, initial: bool) {
		self.data.borrow().touch(initial);
	}
}

impl fmt::Debug for List {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "[")?;
		for (i, val) in self.data.borrow().iter().enumerate() {
			write!(f, "{}", val.repr())?;
			if i != self.len()-1 {
				write!(f, ", ")?;
			}
		}
		write!(f, "]")
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
