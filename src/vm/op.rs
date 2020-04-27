use std::convert::TryFrom;

use super::value::{Value, ValueType::*};
use super::gc::GCWrapper;

enum NumPair {
	Ints(i32, i32),
	Reals(f64, f64),
	NaN,
}

macro_rules! basic_num_op {
	($met_name:ident, $fn:expr) => {
		pub fn $met_name(&self, other: &Value) -> Option<Value> {
			match self.get_num_pair(other) {
				NumPair::Ints(i1, i2) => Some(Value::from($fn(i1, i2))),
				NumPair::Reals(r1, r2) => Some(Value::from($fn(r1, r2))),
				NumPair::NaN => None,
			}
		}
	};
}

impl Value {
	pub fn is_numeric(&self) -> bool {
		match self.get_type() {
			Int | Real => true,
			_ => false,
		}
	}
	
	pub fn cast_real(&self) -> f64 {
		match self.get_type() {
			Int => i32::try_from(self).unwrap() as f64,
			Real => f64::try_from(self).unwrap(),
			_ => panic!("Cannot cast Value to real"),
		}
	}
	
	fn get_num_pair(&self, other: &Value) -> NumPair {
		if !self.is_numeric() { return NumPair::NaN; }
		if !other.is_numeric() { return NumPair::NaN; }
		if self.get_type() == Int && other.get_type() == Int {
			NumPair::Ints(i32::try_from(self).unwrap(), i32::try_from(other).unwrap())
		} else {
			NumPair::Reals(self.cast_real(), other.cast_real())
		}
	}
	
	pub fn neg(&self) -> Option<Value> {
		match self.get_type() {
			Int => Some(Value::from(-i32::try_from(self).unwrap())),
			Real => Some(Value::from(-f64::try_from(self).unwrap())),
			_ => None,
		}
	}
	
	basic_num_op!(add, |a,b| a + b);
	basic_num_op!(sub, |a,b| a - b);
	basic_num_op!(mul, |a,b| a * b);
	
	pub fn div(&self, other: &Value) -> Option<Value> {
		if !self.is_numeric() || !other.is_numeric() { return None; }
		Some(Value::from(self.cast_real() / other.cast_real()))
	}
	
	pub fn pow(&self, other: &Value) -> Option<Value> {
		if !self.is_numeric() || !other.is_numeric() { return None; }
		Some(Value::from(self.cast_real().powf(other.cast_real())))
	}
	
	pub fn modulo(&self, other: &Value) -> Option<Value> {
		match self.get_num_pair(other) {
			NumPair::Ints(i1, i2) => Some(Value::from({
				let r = i1 % i2;
				if r < 0 { r + i2.abs() } else { r }
			})),
			NumPair::Reals(r1, r2) => Some(Value::from({
				let r = r1 % r2;
				if r < 0.0 { r + r2.abs() } else { r }
			})),
			NumPair::NaN => None,
		}
	}
	
	pub fn not(&self) -> Option<Value> {
		if self.get_type() == Bool {
			Some(Value::from(!bool::try_from(self).unwrap()))
		} else {
			None
		}
	}
	
	pub fn or(&self, other: &Value) -> Option<Value> {
		if self.get_type() == Bool && other.get_type() == Bool {
			Some(Value::from(bool::try_from(self).unwrap() || bool::try_from(other).unwrap()))
		} else {
			None
		}
	}
	
	pub fn and(&self, other: &Value) -> Option<Value> {
		if self.get_type() == Bool && other.get_type() == Bool {
			Some(Value::from(bool::try_from(self).unwrap() && bool::try_from(other).unwrap()))
		} else {
			None
		}
	}
	
	pub fn eq(&self, other: &Value) -> bool {
		match (self.get_type(), other.get_type()) {
			(Nil, Nil) => true,
			(Bool, Bool) => bool::try_from(self).unwrap() == bool::try_from(other).unwrap(),
			(Int, Int) => i32::try_from(self).unwrap() == i32::try_from(other).unwrap(),
			(Real, Real) => f64::try_from(self).unwrap() == f64::try_from(other).unwrap(),
			_ =>
				if let (Some(p1), Some(p2)) = (self.get_pointer(), other.get_pointer()) {
					p1 as *const GCWrapper == p2 as *const GCWrapper
				} else {
					false
				}
		}
	}
	
	basic_num_op!(lth, |a,b| a < b);
	basic_num_op!(leq, |a,b| a <= b);
	basic_num_op!(gth, |a,b| a > b);
	basic_num_op!(geq, |a,b| a >= b);
}
