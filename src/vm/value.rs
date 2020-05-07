
use std::cell::Cell;
use std::fmt;
use num_enum::TryFromPrimitive;
use std::convert::TryFrom;

use super::gc::{GC, GCRef, GCWrapper};


/// A Hissy value.
/// 
/// This value can be of the following types: nil, bool, int, real, or a GC object.
/// In the latter case, `Value` is the untyped equivalent of a [`GCRef`], and can be converted to/from one.
/// 
/// Internally, `Value`s are stored using NaN-tagging/boxing, so that non-object values are stored without heap allocation.
pub struct Value(Cell<u64>);

#[derive(TryFromPrimitive, PartialEq)]
#[repr(u64)]
pub(super) enum ValueType {
	Real,
	Nil,
	Bool,
	Int,
	Root,
	Ref,
}

const TAG_SIZE: i8 = 16; // in bits
const TAG_POS:  i8 = 64 - TAG_SIZE;
const TAG_MIN:   u64 = 0xfff8 << TAG_POS;
const DATA_MASK: u64 = std::u64::MAX >> TAG_SIZE;

const fn base_value(t: ValueType) -> u64 {
	TAG_MIN + ((t as u64) << TAG_POS)
}

// A primitive (non-object value) will never have its interior mutated
#[allow(clippy::declare_interior_mutable_const)]
pub const NIL: Value = Value::from_value(base_value(ValueType::Nil));

impl Value {
	const fn from_value(val: u64) -> Value {
		Value(Cell::new(val))
	}
	
	pub(super) fn get_type(&self) -> ValueType {
		if self.0.get() < TAG_MIN {
			ValueType::Real
		} else {
			ValueType::try_from((self.0.get() - TAG_MIN) >> TAG_POS).unwrap()
		}
	}
	
	pub(super) fn from_pointer(pointer: *const GCWrapper, root: bool) -> Value {
		let pointer = pointer as *mut () as u64; // Erases fat pointer data
		assert!(pointer & DATA_MASK == pointer, "Object pointer has too many bits to fit in Value");
		let new_val = Value::from_value(base_value(if root { ValueType::Root } else { ValueType::Ref }) + pointer);
		if root { new_val.get_pointer().unwrap().signal_root() }
		new_val
	}
	
	pub(super) fn get_pointer(&self) -> Option<&GCWrapper> {
		let t = self.get_type();
		if t == ValueType::Root || t == ValueType::Ref {
			let pointer = GCWrapper::fatten_pointer((self.0.get() & DATA_MASK) as *mut ());
			// Safety: as long as the GC algorithm is well-behaved (it frees a reference
			// before or in the same cycle as the referee), and the collecting process
			// does not call this function, self.pointer will be valid.
			unsafe { Some(&*pointer) }
		} else {
			None
		}
	}
	
	fn unroot(&self) {
		if self.get_type() == ValueType::Root {
			self.0.set(base_value(ValueType::Ref) + (self.0.get() & DATA_MASK));
			self.get_pointer().unwrap().signal_unroot();
		}
	}
	
	/// Recursively calls `Traceable::touch()` on subobjects.
	/// 
	/// THIS SHOULD NEVER BE USED OUTSIDE OF [`Traceable::touch`]!
	/// 
	/// [`Traceable::touch`]: ../gc/trait.Traceable.html#tymethod.touch
	pub fn touch(&self, initial: bool) {
		let t = self.get_type();
		if t == ValueType::Root || t == ValueType::Ref {
			if initial {
				self.unroot();
			} else {
				self.get_pointer().unwrap().mark()
			}
		}
	}
	
	/// Outputs a string representation of the `Value` depending on its internal type.
	pub fn repr(&self) -> String {
		match self.get_type() {
			ValueType::Bool => bool::try_from(self).unwrap().to_string(),
			ValueType::Int => i32::try_from(self).unwrap().to_string(),
			ValueType::Real => {
				let r = f64::try_from(self).unwrap();
				if r.is_finite() {
					let mut buf = Vec::new();
					dtoa::write(&mut buf, r).unwrap();
					String::from_utf8(buf).unwrap()
				} else {
					format!("{}", r)
				}
			},
			ValueType::Nil => "nil".to_string(),
			ValueType::Root | ValueType::Ref => self.get_pointer().unwrap().debug(),
		}
	}
}


impl fmt::Debug for Value {
	fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
		write!(f, "Value({})", self.repr())
	}
}


/// Converts a [`GCRef`] to a [`Value`], effectively erasing its type.
impl<T: GC> From<GCRef<T>> for Value {
	fn from(gc_ref: GCRef<T>) -> Value {
		Value::from_pointer(gc_ref.pointer, gc_ref.root.get())
	}
}

/// Attempts to convert a [`Value`] to a [`GCRef<T>`]. Fails if the `Value` does not actually contain a `T`.
impl<T: GC> TryFrom<Value> for GCRef<T> {
	type Error = &'static str;
	
	fn try_from(value: Value) -> Result<Self, &'static str> {
		if let Some(pointer) = value.get_pointer() {
			if pointer.is_a::<T>() {
				let root = value.get_type() == ValueType::Root;
				Ok(GCRef::from_pointer(pointer, root))
			} else {
				Err("Cannot make GCRef<T> of non-T Value")
			}
		} else {
			Err("Cannot make GCRef<T> of non-object Value")
		}
	}
}


/// Clones a `Value`. Note that the new object will be a root reference.
impl Clone for Value {
	fn clone(&self) -> Self {
		if let Some(pointer) = self.get_pointer() {
			Value::from_pointer(pointer, true)
		} else {
			Value::from_value(self.0.get())
		}
	}
}

impl Drop for Value {
	fn drop(&mut self) {
		self.unroot(); // If we were rooting an object, unroot
	}
}


/// Converts an `i32` into a `Value` directly (no heap allocation is performed).
impl From<i32> for Value {
	fn from(i: i32) -> Self {
		Value::from_value(base_value(ValueType::Int) + (i as u32 as u64))
	}
}

/// Converts an `f64` into a `Value` directly (no heap allocation is performed).
impl From<f64> for Value {
	fn from(d: f64) -> Self {
		assert!(f64::to_bits(d) <= TAG_MIN, "Trying to fit 'fat' NaN into Value");
		Value::from_value(f64::to_bits(d))
	}
}

/// Converts a `bool` into a `Value` directly (no heap allocation is performed).
impl From<bool> for Value {
	fn from(b: bool) -> Self {
		Value::from_value(base_value(ValueType::Bool) | (if b { 1 } else { 0 }))
	}
}

/// Attempts to convert a `Value` to an `i32`. Fails if the `Value` does not contain an integer.
impl TryFrom<&Value> for i32 {
	type Error = &'static str;
	fn try_from(value: &Value) -> std::result::Result<Self, &'static str> {
		if value.get_type() == ValueType::Int {
			assert!(value.0.get() & DATA_MASK <= std::u32::MAX as u64, "Invalid integer Value");
			Ok((value.0.get() & DATA_MASK) as i32)
		} else {
			Err("Value is not an integer")
		}
	}
}

/// Attempts to convert a `Value` to an `f64`. Fails if the Value does not contain a real.
impl TryFrom<&Value> for f64 {
	type Error = &'static str;
	fn try_from(value: &Value) -> std::result::Result<Self, &'static str> {
		if value.get_type() == ValueType::Real {
			Ok(f64::from_bits(value.0.get()))
		} else {
			Err("Value is not a real")
		}
	}
}

/// Attempts to convert a `Value` to a `bool`. Fails if the Value does not contain a boolean.
impl TryFrom<&Value> for bool {
	type Error = &'static str;
	fn try_from(value: &Value) -> std::result::Result<Self, &'static str> {
		if value.get_type() == ValueType::Bool {
			assert!(value.0.get() & DATA_MASK <= 1, "Invalid boolean Value");
			Ok(value.0.get() & 1 == 1)
		} else {
			Err("Value is not a boolean")
		}
	}
}



#[cfg(test)]
mod tests {
	use super::*;

	fn test_int(i: i32) {
		assert_eq!(i32::try_from(&Value::from(i)), Ok(i));
	}

	#[test]
	fn test_ints() {
		test_int(0);
		test_int(1);
		test_int(-1);
		test_int(std::i32::MAX);
		test_int(std::i32::MIN);
	}

	fn test_real(d: f64) {
		assert_eq!(f64::try_from(&Value::from(d)), Ok(d));
	}

	#[test]
	fn test_reals() {
		test_real(0.0);
		test_real(3.141_592_653_589_793_6);
		test_real(std::f64::INFINITY);
		test_real(-std::f64::INFINITY);
		match f64::try_from(&Value::from(std::f64::NAN)) {
			Ok(d) if d.is_nan() => (), // NaN != NaN, so we have to test like this
			_ => panic!("std::f64::NAN does not round trip through Value")
		}
	}

	#[test]
	fn test_bools() {
		assert_eq!(bool::try_from(&Value::from(true)), Ok(true));
		assert_eq!(bool::try_from(&Value::from(false)), Ok(false));
	}
}