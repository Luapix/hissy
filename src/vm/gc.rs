
use std::pin::Pin;
use std::cell::Cell;
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
	/// Should call .touch(initial) on all direct GCRef/Value children of self.
	/// This is used for garbage collection.
	fn touch(&self, _initial: bool) {}
}

/// An auto-implemented trait with all the supertraits required for GC values.
/// 
/// To allow a custom type to be placed in the heap, implementing Traceable and Debug should be enough.
pub trait GC: 'static + Traceable + AsAny + Debug {}
impl<T: 'static + Traceable + AsAny + Debug> GC for T {}


#[repr(C)]
pub(super) struct GCWrapper_<T: ?Sized> {
	vtable: *mut (),
	marked: Cell<bool>,
	roots: Cell<u32>,
	data: T,
}
pub(super) type GCWrapper = GCWrapper_<dyn GC>;
// Note: we need to use this GCWrapper_<T: ?Sized> / GCWrapper indirection
// because of Rust's still partial support for custom DSTs.

impl GCWrapper {
	fn new_pinned<T: GC>(mut value: T) -> Pin<Box<GCWrapper>> {
		let trait_object: &mut dyn GC = &mut value;
		// Safety: raw::TraitObject layout should correspond to actual trait object layout
		let raw_object: raw::TraitObject = unsafe { mem::transmute(trait_object) };
		Box::pin(GCWrapper_ {
			vtable: raw_object.vtable,
			marked: Cell::new(false),
			roots: Cell::new(0),
			data: value
		})
	}
	
	/// Returns a fat pointer to GCWrapper from a thin void pointer.
	///
	/// Used in [`Value`], since a fat pointer doesn't fit into a `Value`
	/// Possible since `GCWrapper` contains its object's VTable
	pub fn fatten_pointer(data: *mut ()) -> *const GCWrapper {
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
	
	fn size(&self) -> usize {
		mem::size_of_val(&self) + mem::size_of_val(&self.data)
	}
	
	pub fn signal_root(&self) {
		self.roots.set(self.roots.get() + 1);
	}
	pub fn signal_unroot(&self) {
		self.roots.set(self.roots.get() - 1);
	}
	
	pub fn mark(&self) {
		if !self.marked.get() {
			self.marked.set(true);
			self.data.touch(false);
		}
	}
	
	pub fn unroot_children(&self) {
		self.data.touch(true);
	}
	
	fn reset(&self) {
		self.marked.set(false);
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
	pub(super) root: Cell<bool>,
	pub(super) pointer: *const GCWrapper,
	phantom: PhantomData<T>,
}

impl<T: GC> GCRef<T> {
	pub(super) fn from_pointer(pointer: *const GCWrapper, root: bool) -> GCRef<T> {
		let new_ref = GCRef { root: Cell::new(root), pointer, phantom: PhantomData::<T> };
		assert!(new_ref.wrapper().is_a::<T>(), "Cannot make GCRef<T> to non-T Object");
		if root { new_ref.wrapper().signal_root(); }
		new_ref
	}
	
	fn wrapper(&self) -> &GCWrapper {
		// Safety: as long as the GC algorithm is well-behaved (it frees a reference
		// before or in the same cycle as the referee), and the collecting process
		// does not call this function, self.pointer will be valid.
		unsafe { &*self.pointer }
	}
	
	fn unroot(&self) {
		if self.root.get() {
			self.root.set(false);
			self.wrapper().signal_unroot();
		}
	}
	
	fn mark(&self) {
		self.wrapper().mark();
	}
	
	/// Recursively calls `Traceable::touch` on subobjects.
	/// 
	/// THIS SHOULD NEVER BE USED OUTSIDE OF [`Traceable::touch`]!
	pub fn touch(&self, initial: bool) {
		if initial { // Unroot
			self.unroot();
		} else { // Mark subobjects
			self.mark();
		}
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


const INIT_THRESHOLD: usize = 64;

/// Object maintaining all GC state.
/// 
/// Usually, only one should be created.
#[derive(Default)]
pub struct GCHeap {
	objects: Vec<Pin<Box<GCWrapper>>>,
	threshold: usize,
	used: usize,
}

impl GCHeap {
	/// Create a new, empty GC heap.
	pub fn new() -> GCHeap {
		GCHeap {
			objects: vec![],
			threshold: INIT_THRESHOLD,
			used: 0,
		}
	}
	
	fn add<T: GC>(&mut self, v: T) -> &GCWrapper {
		let wrapper = GCWrapper::new_pinned(v);
		self.used += wrapper.size();
		wrapper.unroot_children(); // Unroot children
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
	/// This uses [`Traceable.touch`] to determine all live objects.
	pub fn collect(&mut self) {
		for wrapper in self.objects.iter_mut() {
			if wrapper.roots.get() > 0 {
				wrapper.mark();
			}
		}
		
		self.objects.retain(|wrapper| wrapper.marked.get());
		
		self.used = 0;
		for wrapper in self.objects.iter_mut() {
			wrapper.reset();
			self.used += wrapper.size();
		}
	}
	
	/// Calls collect() if the used memory is past a threshold.
	/// 
	/// The threshold is set to some initial value, and will be set to double
	/// the current usage at the end of any collection initiated by this function.
	pub fn step(&mut self) {
		if self.used >= self.threshold {
			self.collect();
			self.threshold = self.used * 2;
		}
	}
	
	/// Inspect current heap contents. Prints to standard output.
	pub fn inspect(&self) {
		println!("[GC inspect] ({}B used, collect at {}B)", self.used, self.threshold);
		for wrapper in self.objects.iter() {
			println!("{}: {} roots", wrapper.debug(), wrapper.roots.get());
		}
	}
	
	/// Returns the total number of bytes stored in the GC heap.
	pub fn used_memory(&self) -> usize {
		self.used
	}
	
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
			eprintln!("GC heap could not collect all objects before being dropped; references will be left dangling!");
		}
	}
}
