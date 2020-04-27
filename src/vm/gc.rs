
use std::{ptr, mem, raw, fmt};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::any::Any;
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


#[repr(C)]
pub(super) struct GCWrapper_<T: ?Sized> {
	vtable: *mut (),
	marked: bool,
	roots: u32,
	data: T,
}
pub(super) type GCWrapper = GCWrapper_<dyn GC>;
// Note: we need to use this GCWrapper_<T: ?Sized> / GCWrapper indirection
// because of Rust's still partial support for custom DSTs.

impl GCWrapper {
	fn new_boxed<T: GC>(mut value: T) -> Box<GCWrapper> {
		let trait_object: &mut dyn GC = &mut value;
		// Safety: raw::TraitObject layout should correspond to actual trait object layout
		let raw_object: raw::TraitObject = unsafe { mem::transmute(trait_object) };
		Box::new(GCWrapper_ {
			vtable: raw_object.vtable,
			marked: false,
			roots: 0,
			data: value
		})
	}
	
	// Returns a fat pointer to GCWrapper from a thin void pointer
	// Used in Value, since a fat pointer doesn't fit into a Value
	// Possible since GCWrapper contains its object's VTable
	pub fn fatten_pointer(ptr: *mut ()) -> *mut GCWrapper {
		// Safety: "vtable" is stored at base of GCWrapper thanks to #[repr(C)],
		// and raw::TraitObject layout should correspond to actual trait object layout
		unsafe {
			let vtable: *mut () = ptr::read(ptr.cast());
			mem::transmute(raw::TraitObject {
				data: ptr,
				vtable: vtable,
			})
		}
	}
	
	pub fn is_a<T: GC>(&self) -> bool {
		self.data.as_any().is::<T>()
	}
	
	pub fn get<T: GC>(&mut self) -> Option<&mut T> {
		self.data.as_any_mut().downcast_mut::<T>()
	}
	
	pub fn debug(&self) -> String {
		format!("{:?}", self)
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
			self.data.mark();
		}
	}
	
	fn reset(&mut self) {
		self.marked = false;
	}
}

impl Debug for GCWrapper {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.data.fmt(f)
	}
}


pub struct GCRef<T: GC> {
	pub(super) root: bool,
	pub(super) pointer: *mut GCWrapper,
	phantom: PhantomData<T>,
}

impl<T: GC> GCRef<T> {
	pub(super) fn from_pointer(pointer: *mut GCWrapper, root: bool) -> GCRef<T> {
		let new_ref = GCRef { root: root, pointer: pointer, phantom: PhantomData::<T> };
		assert!(new_ref.wrapper().is_a::<T>(), "Cannot make GCRef<T> to non-T Object");
		if root { new_ref.wrapper().signal_root(); }
		new_ref
	}
	
	fn wrapper(&self) -> &mut GCWrapper {
		// Safety: as long as the GC algorithm is well-behaved (it frees a reference
		// before or in the same cycle as the referee), and the collecting process
		// does not call this function, self.pointer will be valid.
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
	
	fn deref(&self) -> &T {
		self.wrapper().get::<T>().unwrap()
	}
}

impl<T: GC> Debug for GCRef<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
		write!(f, "GCRef({:?})", **self)
	}
}


pub struct GCHeap {
	objects: Vec<Box<GCWrapper>>,
}

impl GCHeap {
	pub fn new() -> GCHeap {
		GCHeap { objects: Vec::new() }
	}
	
	fn add<T: GC>(&mut self, v: T) -> &mut GCWrapper {
		let mut wrapper = GCWrapper::new_boxed(v);
		wrapper.data.unroot();
		self.objects.push(wrapper);
		self.objects.last_mut().unwrap()
	}

	pub fn make_ref<T: GC>(&mut self, v: T) -> GCRef<T> {
		GCRef::from_pointer(self.add(v), true) // Root new object
	}
	pub fn make_value<T: GC>(&mut self, v: T) -> Value {
		Value::from_pointer(self.add(v), true) // Root new object
	}
	
	pub fn collect(&mut self) {
		for wrapper in self.objects.iter_mut() {
			if wrapper.roots > 0 {
				wrapper.mark();
			}
		}
		
		self.objects.retain(|wrapper| wrapper.marked);
		
		for wrapper in self.objects.iter_mut() {
			wrapper.reset();
		}
	}
	
	pub fn inspect(&self) {
		println!("== GC inspect ==");
		for wrapper in self.objects.iter() {
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
