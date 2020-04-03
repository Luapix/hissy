
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

#[derive(Debug)]
pub struct Closure {
	pub chunk_id: u8
}

impl Closure {
	pub fn new(chunk_id: u8) -> Closure {
		Closure { chunk_id }
	}
}

impl Traceable for Closure {
	fn mark(&self) {}
	fn unroot(&mut self) {}
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
		heap.examine();
		{
			let _l = heap.make_ref(vec![foo, bar]);
			heap.collect();
			heap.examine();
		}
		heap.collect();
		heap.examine();
		
		{
			let _l;
			{
				let foo = heap.make_ref(String::from("foo"));
				let bar = heap.make_ref(String::from("bar"));
				heap.collect();
				heap.examine();
				_l = heap.make_ref(vec![foo.clone(), bar.clone()]);
				heap.collect();
				heap.examine();
			}
			heap.collect();
			heap.examine();
		}
		heap.collect();
		heap.examine();
		assert!(heap.is_empty());
	}
	
	#[test]
	fn test_vec_val() {
		let mut heap = GCHeap::new();
		let foo = heap.make_value(String::from("foo"));
		let bar = heap.make_value(String::from("bar"));
		heap.collect();
		heap.examine();
		{
			let _l = heap.make_value(vec![foo, bar]);
			heap.collect();
			heap.examine();
		}
		heap.collect();
		heap.examine();
		
		{
			let _l;
			{
				let foo = heap.make_value(String::from("foo"));
				let bar = heap.make_value(String::from("bar"));
				heap.collect();
				heap.examine();
				_l = heap.make_value(vec![foo.clone(), bar.clone()]);
				heap.collect();
				heap.examine();
			}
			heap.collect();
			heap.examine();
		}
		heap.collect();
		heap.examine();
		assert!(heap.is_empty());
	}
}
