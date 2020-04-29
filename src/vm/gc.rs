
use std::{ptr, mem, raw, fmt};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::any::Any;
use std::ops::Deref;

use super::value::Value;


/// An auto-implemented trait to allow easier access to Any methods.
pub trait AsAny {
	fn as_any(&self) -> &dyn Any;
	fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAny for T {
	fn as_any(&self) -> &dyn Any { self }
	fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

/// This trait allows the GC to trace through objects in its heap.
pub trait Traceable {
	/// Should call .mark() on all direct GCRef/Value children of self.
	/// This is used during garbage collection.
	fn mark(&mut self);
	/// Should call .unroot() on all direct GCRef/Value children of self.
	/// This is used when placing an object in the GC heap, to remove all root references inside it.
	fn unroot(&mut self);
}

/// An auto-implemented trait with all the supertraits required for GC values.
/// 
/// To allow a custom type to be placed in the heap, implementing Traceable and Debug should be enough.
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
	
	/// Returns a fat pointer to GCWrapper from a thin void pointer.
	///
	/// Used in [`Value`], since a fat pointer doesn't fit into a `Value`
	/// Possible since `GCWrapper` contains its object's VTable
	pub fn fatten_pointer(data: *mut ()) -> *mut GCWrapper {
		// Safety: "vtable" is stored at base of GCWrapper thanks to #[repr(C)],
		// and raw::TraitObject layout should correspond to actual trait object layout
		unsafe {
			let vtable: *mut () = ptr::read(data.cast());
			mem::transmute(raw::TraitObject { data, vtable })
		}
	}
	
	pub fn is_a<T: GC>(&self) -> bool {
		self.data.as_any().is::<T>()
	}
	
	pub fn get<T: GC>(&self) -> Option<&T> {
		self.data.as_any().downcast_ref::<T>()
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


/// A typed reference to a GC object.
///
/// Obtained through `TryFrom<Value>` or [`GCHeap::make_ref`].
///
/// This is the typed equivalent of a [`Value`] containing an object, and can be converted to/from one.
/// 
/// [`Value`]: ../value/struct.Value.html
pub struct GCRef<T: GC> {
	pub(super) root: bool,
	pub(super) pointer: *mut GCWrapper,
	phantom: PhantomData<T>,
}

impl<T: GC> GCRef<T> {
	pub(super) fn from_pointer(pointer: *mut GCWrapper, root: bool) -> GCRef<T> {
		let mut new_ref = GCRef { root, pointer, phantom: PhantomData::<T> };
		assert!(new_ref.wrapper().is_a::<T>(), "Cannot make GCRef<T> to non-T Object");
		if root { new_ref.wrapper_mut().signal_root(); }
		new_ref
	}
	
	fn wrapper(&self) -> &GCWrapper {
		// Safety: as long as the GC algorithm is well-behaved (it frees a reference
		// before or in the same cycle as the referee), and the collecting process
		// does not call this function, self.pointer will be valid.
		unsafe { &*self.pointer }
	}
	
	fn wrapper_mut(&mut self) -> &mut GCWrapper {
		// Safety: as long as the GC algorithm is well-behaved (it frees a reference
		// before or in the same cycle as the referee), and the collecting process
		// does not call this function, self.pointer will be valid.
		unsafe { &mut *self.pointer }
	}
	
	/// Marks the `GCRef` as no longer a root reference.
	/// 
	/// THIS SHOULD NEVER BE USED OUTSIDE OF [`Traceable::unroot`]!
	pub fn unroot(&mut self) {
		if self.root {
			self.root = false;
			self.wrapper_mut().signal_unroot();
		}
	}
	
	/// Recursively calls `Traceable::mark` on subobjects.
	/// 
	/// THIS SHOULD NEVER BE USED OUTSIDE OF [`Traceable::mark`]!
	pub fn mark(&mut self) {
		self.wrapper_mut().mark();
	}
}


/// Clones a `GCRef`. Note that the new object will be a root reference.
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


/// Object maintaining all GC state.
/// 
/// Usually, only one should be created.
#[derive(Default)]
pub struct GCHeap {
	objects: Vec<Box<GCWrapper>>,
}

impl GCHeap {
	/// Create a new, empty GC heap.
	pub fn new() -> Self {
		Default::default()
	}
	
	fn add<T: GC>(&mut self, v: T) -> &mut GCWrapper {
		let mut wrapper = GCWrapper::new_boxed(v);
		wrapper.data.unroot();
		self.objects.push(wrapper);
		self.objects.last_mut().unwrap()
	}
	
	/// Place an object implementing GC into the heap, returning a typed reference to it.
	pub fn make_ref<T: GC>(&mut self, v: T) -> GCRef<T> {
		GCRef::from_pointer(self.add(v), true) // Root new object
	}
	/// Place an object implementing GC into the heap, returning an untyped reference to it.
	pub fn make_value<T: GC>(&mut self, v: T) -> Value {
		Value::from_pointer(self.add(v), true) // Root new object
	}
	
	/// Delete dead objects from heap.
	/// 
	/// This uses [`Traceable.mark`] to determine all live objects.
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
	
	/// Inspect current heap contents. Prints to standard output.
	pub fn inspect(&self) {
		println!("== GC inspect ==");
		for wrapper in self.objects.iter() {
			println!("{}: {} roots", wrapper.debug(), wrapper.roots);
		}
	}
	
	/// Returns the number of (live or not) objects in the heap.
	pub fn size(&self) -> usize {
		self.objects.len()
	}
	
	/// Returns whether the heap is empty (dead but not yet collected objects included).
	pub fn is_empty(&self) -> bool {
		self.objects.is_empty()
	}
}

/// The `Drop` implementation for `GCHeap` does not collect all remaining objects;
/// it simply prints a warning if the heap is not empty.
/// 
/// The user is responsible for making sure the `GCHeap` has been entirely collected before dropping it
/// (dropping all root references and calling [`GCHeap::collect()`] should be enough).
impl Drop for GCHeap {
	fn drop(&mut self) {
		self.collect();
		if !self.is_empty() {
			eprintln!("GC heap could not collect all objects!");
		}
	}
}
