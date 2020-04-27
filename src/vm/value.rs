
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
pub struct Value(u64);

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

pub const NIL:   Value = Value(base_value(ValueType::Nil));
pub const FALSE: Value = Value(base_value(ValueType::Bool));
pub const TRUE:  Value = Value(base_value(ValueType::Bool) | 1);

impl Value {
	pub(super) fn get_type(&self) -> ValueType {
		if self.0 < TAG_MIN {
			ValueType::Real
		} else {
			ValueType::try_from((self.0 - TAG_MIN) >> TAG_POS).unwrap()
		}
	}
	
	pub(super) fn from_pointer(pointer: *mut GCWrapper, root: bool) -> Value {
		let pointer = pointer as *mut () as u64; // Erases fat pointer data
		debug_assert!(pointer & DATA_MASK == pointer, "Object pointer has too many bits to fit in Value");
		let new_val = Value(base_value(if root { ValueType::Root } else { ValueType::Ref }) + pointer);
		if root { new_val.get_pointer().unwrap().signal_root() }
		new_val
	}
	
	pub(super) fn get_pointer(&self) -> Option<&mut GCWrapper> {
		let t = self.get_type();
		if t == ValueType::Root || t == ValueType::Ref {
			let pointer = GCWrapper::fatten_pointer((self.0 & DATA_MASK) as *mut ());
			// Safety: as long as the GC algorithm is well-behaved (it frees a reference
			// before or in the same cycle as the referee), and the collecting process
			// does not call this function, self.pointer will be valid.
			unsafe { Some(&mut *pointer) }
		} else {
			None
		}
	}
	
	/// Marks the `Value` as no longer a root reference.
	/// 
	/// THIS SHOULD NEVER BE USED OUTSIDE OF [`Traceable::unroot`]!
	/// 
	/// [`Traceable::unroot`]: ../gc/trait.Traceable.html#tymethod.unroot
	pub fn unroot(&mut self) {
		if self.get_type() == ValueType::Root {
			self.0 = base_value(ValueType::Ref) + (self.0 & DATA_MASK);
			self.get_pointer().unwrap().signal_unroot();
		}
	}
	
	/// Recursively calls `Traceable::mark()` on subobjects.
	/// 
	/// THIS SHOULD NEVER BE USED OUTSIDE OF [`Traceable::mark`]!
	/// 
	/// [`Traceable::mark`]: ../gc/trait.Traceable.html#tymethod.mark
	pub fn mark(&self) {
		if let Some(p) = self.get_pointer() {
			p.mark()
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
		Value::from_pointer(gc_ref.pointer, gc_ref.root)
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
			Value(self.0)
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
		Value(base_value(ValueType::Int) + (i as u32 as u64))
	}
}

/// Converts an `f64` into a `Value` directly (no heap allocation is performed).
impl From<f64> for Value {
	fn from(d: f64) -> Self {
		debug_assert!(f64::to_bits(d) <= TAG_MIN, "Trying to fit 'fat' NaN into Value");
		Value(f64::to_bits(d))
	}
}

/// Converts a `bool` into a `Value` directly (no heap allocation is performed).
impl From<bool> for Value {
	fn from(b: bool) -> Self {
		Value(base_value(ValueType::Bool) | (if b { 1 } else { 0 }))
	}
}

/// Attempts to convert a `Value` to an `i32`. Fails if the `Value` does not contain an integer.
impl TryFrom<&Value> for i32 {
	type Error = &'static str;
	fn try_from(value: &Value) -> std::result::Result<Self, &'static str> {
		if value.get_type() == ValueType::Int {
			debug_assert!(value.0 & DATA_MASK <= std::u32::MAX as u64, "Invalid integer Value");
			Ok((value.0 & DATA_MASK) as i32)
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
			Ok(f64::from_bits(value.0))
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
			debug_assert!(value.0 & DATA_MASK <= 1, "Invalid boolean Value");
			Ok(value.0 & 1 == 1)
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
		assert_eq!(bool::try_from(&TRUE), Ok(true));
		assert_eq!(bool::try_from(&FALSE), Ok(false));
	}
}