
use std::fmt::Debug;
use std::marker::PhantomData;
use std::any::Any;
use std::collections::HashSet;
use std::ops::Deref;

use super::value::Value;


pub trait AsAny {
	fn as_any(&self) -> &dyn Any;
	fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAny for T {
	fn as_any(&self) -> &dyn Any { self }
	fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

pub trait Traceable {
	fn mark(&self); // Call .mark() on direct GCRef/Value children
	fn unroot(&mut self); // Call .unroot() on direct GCRef/Value children
}

pub trait GC: 'static + Traceable + AsAny + Debug {}
impl<T: 'static + Traceable + AsAny + Debug> GC for T {}


pub struct GCWrapper {
	marked: bool,
	roots: u32,
	data: Box<dyn GC>,
}

impl GCWrapper {
	fn new<T: GC>(value: T) -> GCWrapper {
		GCWrapper { marked: false, roots: 0, data: Box::new(value) }
	}
	
	pub fn is_a<T: GC>(&self) -> bool {
		(*self.data).as_any().is::<T>()
	}
	
	pub fn get<T: GC>(&mut self) -> Option<&mut T> {
		(*self.data).as_any_mut().downcast_mut::<T>()
	}
	
	pub fn debug(&self) -> String {
		format!("{:?}", self.data)
	}
	
	pub fn signal_root(&mut self) {
		self.roots += 1;
	}
	pub fn signal_unroot(&mut self) {
		self.roots -= 1;
	}
	
	pub fn mark(&mut self) {
		if !self.marked {
			self.marked = true;
			(*self.data).mark();
		}
	}
	
	pub fn reset(&mut self) {
		self.marked = false;
	}
}


pub struct GCRef<T: GC> {
	pub root: bool,
	pub pointer: *mut GCWrapper,
	phantom: PhantomData<T>,
}

impl<T: GC> GCRef<T> {
	pub fn from_pointer(pointer: *mut GCWrapper, root: bool) -> GCRef<T> {
		let new_ref = GCRef { root: root, pointer: pointer, phantom: PhantomData::<T> };
		assert!(new_ref.wrapper().is_a::<T>(), "Cannot make GCRef<T> to non-T Object");
		if root { new_ref.wrapper().signal_root(); }
		new_ref
	}
	
	fn wrapper(&self) -> &mut GCWrapper {
		// Safety: as long as the GC algorithm is well-behaved, ie. frees a reference
		// before or in the same cycle as the referee, self.pointer will be valid.
		unsafe { &mut *self.pointer }
	}
	
	pub fn unroot(&mut self) {
		if self.root {
			self.root = false;
			self.wrapper().signal_unroot();
		}
	}
	
	pub fn mark(&self) {
		self.wrapper().mark();
	}
	
	pub fn reset(&self) {
		self.wrapper().reset();
	}
}


impl<T: GC> Clone for GCRef<T> {
	fn clone(&self) -> Self {
		GCRef::from_pointer(self.pointer, true)
	}
}

impl<T: GC> Drop for GCRef<T> {
	fn drop(&mut self) {
		self.unroot();
	}
}


impl<T: GC> Deref for GCRef<T> {
	type Target = T;
	
	fn deref(&self) -> &Self::Target {
		self.wrapper().get::<T>().unwrap()
	}
}

impl<T: GC> Debug for GCRef<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
		write!(f, "GCRef({:?})", **self)
	}
}


pub struct GCHeap {
	objects: HashSet<*mut GCWrapper>,
}

impl GCHeap {
	pub fn new() -> GCHeap {
		GCHeap { objects: HashSet::new() }
	}
	
	fn add<T: GC>(&mut self, v: T) -> *mut GCWrapper {
		let pointer = Box::into_raw(Box::new(GCWrapper::new(v))); // Leak GCWrapper memory
		unsafe { (*pointer).data.unroot(); }
		// Unroots any GCRef/Values that may have been moved into the object
		// Safety: Of course, the pointer is still valid at this point
		self.objects.insert(pointer);
		pointer
	}

	pub fn make_ref<T: GC>(&mut self, v: T) -> GCRef<T> {
		GCRef::from_pointer(self.add(v), true) // Root new object
	}
	pub fn make_value<T: GC>(&mut self, v: T) -> Value {
		Value::from_pointer(self.add(v), true) // Root new object
	}
	
	pub fn collect(&mut self) {
		for pointer in self.objects.iter() {
			let wrapper = unsafe { &mut **pointer };
			if wrapper.roots > 0 {
				wrapper.mark();
			}
		}
		
		self.objects.retain(|&pointer| {
			let wrapper = unsafe { &mut *pointer };
			if wrapper.marked {
				wrapper.reset();
				true // Keep pointer
			} else {
				unsafe { Box::from_raw(pointer); } // Free pointer
				false // Remove it from set
			}
		});
	}
	
	pub fn examine(&self) {
		println!("== GC examine ==");
		for pointer in self.objects.iter() {
			let wrapper = unsafe { &**pointer };
			// Safety of previous line: The GC algorithm should remove pointers from self.objects
			// as soon as the object is freed.
			println!("{}: {} roots", wrapper.debug(), wrapper.roots);
		}
	}
	
	pub fn size(&self) -> usize {
		self.objects.len()
	}
	
	pub fn is_empty(&self) -> bool {
		self.objects.is_empty()
	}
}

// The Drop implementation for GCHeap is only a warning; the user is responsible
// for making sure the GCHeap has been entirely collected before dropping it
// (dropping all root references and .collect() should be enough)

impl Drop for GCHeap {
	fn drop(&mut self) {
		if !self.objects.is_empty() {
			println!("Warning: GCHeap was not empty when dropped");
		}
	}
}
