
use crate::vm::{gc::GCHeap, value::{Value, NIL}, object::NativeFunction};

pub fn list() -> Vec<String> {
	vec![
		"log"
	].iter().copied().map(String::from).collect()
}

pub fn create(heap: &mut GCHeap) -> Vec<Value> {
	let mut res = vec![];
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
