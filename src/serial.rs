
use std::fmt::Debug;
use std::convert::TryInto;

pub fn read_u8<'a>(it: &mut impl Iterator<Item = &'a u8>) -> u8 {
	*it.next().expect("Unexpected EOF")
}

pub fn read_i8<'a>(it: &mut impl Iterator<Item = &'a u8>) -> i8 {
	i8::from_le_bytes([read_u8(it)])
}

pub fn read_u16<'a>(it: &mut impl Iterator<Item = &'a u8>) -> u16 {
	let bytes = [read_u8(it), read_u8(it)];
	u16::from_le_bytes(bytes)
}

pub fn read_i32<'a>(it: &mut impl Iterator<Item = &'a u8>) -> i32 {
	let mut bytes = [0; 4];
	for i in 0..4 { bytes[i] = read_u8(it); }
	i32::from_le_bytes(bytes)
}

pub fn read_f64<'a>(it: &mut impl Iterator<Item = &'a u8>) -> f64 {
	let mut bytes = [0; 8];
	for i in 0..8 { bytes[i] = read_u8(it); }
	f64::from_le_bytes(bytes)
}

pub fn read_small_str<'a>(it: &mut impl Iterator<Item = &'a u8>) -> String {
	let length = read_u8(it) as usize;
	let s = String::from_utf8(it.by_ref().take(length).copied().collect()).expect("Invalid UTF8 in string");
	s
}

pub fn read_str<'a>(it: &mut impl Iterator<Item = &'a u8>) -> String {
	let length = read_u16(it) as usize;
	let s = String::from_utf8(it.by_ref().take(length).copied().collect()).expect("Invalid UTF8 in string");
	s
}


pub fn write_u8<T: Into<u8>>(out: &mut Vec<u8>, b: T) {
	out.push(b.into());
}

pub fn write_into_u8<T: TryInto<u8>>(out: &mut Vec<u8>, b: T, mes: &str) where <T as TryInto<u8>>::Error: Debug {
	out.push(b.try_into().expect(mes));
}


macro_rules! write_numeric {
	($write: ident, $write_into: ident, $t: ty) => {
		#[allow(dead_code)]
		pub fn $write<T: Into<$t>>(out: &mut Vec<u8>, b: T) {
			out.extend(&b.into().to_le_bytes());
		}
		
		#[allow(dead_code)]
		pub fn $write_into<T: TryInto<$t>>(out: &mut Vec<u8>, b: T, mes: &str) where <T as TryInto<$t>>::Error: Debug {
			out.extend(&b.try_into().expect(mes).to_le_bytes());
		}
	};
}

write_numeric!(write_u16, write_into_u16, u16);
write_numeric!(write_u32, write_into_u32, u32);
write_numeric!(write_i32, write_into_i32, i32);
write_numeric!(write_f64, write_into_f64, f64);


pub fn write_small_str(out: &mut Vec<u8>, s: &str) {
	write_into_u8(out, s.len(), "Cannot serialize small string: string too long");
	out.extend(s.as_bytes());
}

pub fn write_str(out: &mut Vec<u8>, s: &str) {
	write_into_u16(out, s.len(), "Cannot serialise string: string too long");
	out.extend(s.as_bytes());
}
