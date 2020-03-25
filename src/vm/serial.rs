
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
