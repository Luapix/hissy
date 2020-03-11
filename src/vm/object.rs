
use super::gc::Trace;

impl Trace for String {
	fn mark(&self) {}
	fn unroot(&mut self) {}
}
