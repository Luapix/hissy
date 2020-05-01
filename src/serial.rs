
use std::fmt::Debug;
use std::convert::{TryFrom, TryInto};

use crate::{HissyError, ErrorType};


fn error_str(s: &str) -> HissyError {
	HissyError(ErrorType::IO, String::from(s), 0)
}
fn eof() -> HissyError {
	error_str("Unexpected EOF")
}

pub fn read_u8<'a>(it: &mut impl Iterator<Item = &'a u8>) -> Result<u8, HissyError> {
	it.next().copied().ok_or_else(eof)
}

pub fn read_u8s<'a, T, E: Debug>(it: &mut impl Iterator<Item = &'a u8>, n: usize) -> Result<T, HissyError>
		where for<'b> T: TryFrom<&'b [u8], Error = E> {
	let vec: Vec<u8> = it.take(n).copied().collect();
	T::try_from(&vec).map_err(|_| eof())
}

pub fn write_u8<T: Into<u8>>(out: &mut Vec<u8>, b: T) {
	out.push(b.into());
}

macro_rules! serialize_numeric {
	($read: ident, $write: ident, $write_into: ident, $t: ty) => {
		#[allow(dead_code)]
		pub fn $read<'a>(it: &mut impl Iterator<Item = &'a u8>) -> Result<$t, HissyError> {
			Ok(<$t>::from_le_bytes(read_u8s(it, std::mem::size_of::<$t>())?))
		}
		
		#[allow(dead_code)]
		pub fn $write<T: Into<$t>>(out: &mut Vec<u8>, b: T) {
			out.extend(&b.into().to_le_bytes());
		}
		
		#[allow(dead_code)]
		pub fn $write_into<T: TryInto<$t>>(out: &mut Vec<u8>, b: T, err: HissyError) -> Result<(), HissyError> where <T as TryInto<$t>>::Error: Debug {
			out.extend(&b.try_into().map_err(|_| err)?.to_le_bytes());
			Ok(())
		}
	};
}

serialize_numeric!(read_i8, write_i8, write_into_i8, i8);
serialize_numeric!(read_u16, write_u16, write_into_u16, u16);
serialize_numeric!(read_u32, write_u32, write_into_u32, u32);
serialize_numeric!(read_i32, write_i32, write_into_i32, i32);
serialize_numeric!(read_f64, write_f64, write_into_f64, f64);


pub fn read_small_str<'a>(it: &mut impl Iterator<Item = &'a u8>) -> Result<String, HissyError> {
	let length = read_u8(it)? as usize;
	String::from_utf8(read_u8s(it, length)?).map_err(|_| error_str("Invalid UTF8 in string"))
}

pub fn write_small_str(out: &mut Vec<u8>, s: &str) {
	let s = if s.len() >= 256 { &s[..255] } else { s };
	write_u8(out, u8::try_from(s.len()).unwrap());
	out.extend(s.as_bytes());
}


pub fn read_str<'a>(it: &mut impl Iterator<Item = &'a u8>) -> Result<String, HissyError> {
	let length = read_u16(it)? as usize;
	String::from_utf8(read_u8s(it, length)?).map_err(|_| error_str("Invalid UTF8 in string"))
}

pub fn write_str(out: &mut Vec<u8>, s: &str) -> Result<(), HissyError> {
	write_into_u16(out, s.len(), error_str("Cannot serialise string: string too long"))?;
	out.extend(s.as_bytes());
	Ok(())
}
