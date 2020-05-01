
use std::str::CharIndices;
use std::iter::Peekable;
use std::fmt;
use unicode_xid::UnicodeXID;
use peg::{Parse, ParseElem, ParseLiteral, ParseSlice, RuleResult, str::LineCol};
use smallstr::SmallString;

use crate::{HissyError, ErrorType};


fn error(s: String, pos: LineCol) -> HissyError {
	HissyError(ErrorType::Syntax, s, pos.line as u16)
}
fn error_str(s: &str, pos: LineCol) -> HissyError {
	error(String::from(s), pos)
}

type SymbolStr = SmallString<[u8;6]>;

/// A language token.
#[derive(Debug, PartialEq, Clone)]
pub enum Token {
	Symbol(SymbolStr),
	Id(String),
	Int(i32),
	Real(f64),
	String(String),
	Newline, Indent, Dedent,
	EOF,
}

static KEYWORDS: [&str; 14] = [
	"let", "if", "else", "while",
	"not", "and", "or",
	"nil", "true", "false",
	"return", "log",
	"fun",
	"pass",
];

fn is_keyword(s: &str) -> bool {
	KEYWORDS.contains(&s)
}

fn parse_number(input: &str, is_integer: bool) -> Token {
	if is_integer {
		if let Ok(i) = input.parse::<i32>() {
			return Token::Int(i);
		}
	}
	Token::Real(input.parse::<f64>().expect("Error while parsing real literal"))
}

static SIMPLE_SYMBOLS: [char; 17] = [
	'+', '-', '*', '/', '^', '%',
	'=', '<', '>',
	',', '(', ')', ':',
	'[', ']',
	'.',
	'\n',
];

static SYMBOL_START: [char; 11] = [
	'+', '-', '*', '/', '^', '%',
	'=', '<', '>',
	'!',
	'\r',
];

static COMPLEX_SYMBOLS: [&str; 21] = [
	"=", "+", "-", "*", "/", "^", "%", "<", ">",
	"==", "!=", "+=", "-=", "*=", "/=", "^=", "%=", "<=", ">=",
	"->",
	"\r\n",
];

fn parse_symbol(it: &mut Peekable<CharIndices>, c: char) -> Option<SymbolStr> {
	let simple = SIMPLE_SYMBOLS.contains(&c); // is c a symbol by itself?
	let start = SYMBOL_START.contains(&c); // could it start a complex symbol?
	
	if !simple && !start { return None; }
	it.next(); // it has to be part of a symbol, consume c.
	
	if start {
		let pair = it.peek().map(|(_,c2)| {
			let mut s = SmallString::from(c);
			s.push(*c2);
			s
		});
		if pair.as_ref().map_or(false, |p| COMPLEX_SYMBOLS.contains(&p.as_ref())) {
			it.next(); // consume second character
			return pair;
		}
	}
	
	// if we get here, it has to be a simple symbol
	Some(SmallString::from(c))
}

fn test_next_char<P>(it: &mut Peekable<CharIndices>, pred: &P) -> bool where P: Fn(char) -> bool {
	it.peek().map_or(false, |(_,c)| pred(*c))
}

fn skip_chars<P>(it: &mut Peekable<CharIndices>, pred: &P) where P: Fn(char) -> bool {
	while test_next_char(it, pred) {
		it.next();
	}
}

fn get_next_index(it: &mut Peekable<CharIndices>, end: usize) -> usize {
	it.peek().map_or(end, |(i,_)| *i)
}

/// A [`Token`] sequence, suitable for use with peg.rs parsers.
/// 
/// Can be Displayed to inspect contents.
pub struct Tokens {
	pub tokens: Vec<Token>,
	pub(super) token_pos: Vec<LineCol>,
}

impl fmt::Display for Tokens {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Tokens[")?;
		for i in 0..self.tokens.len() {
			if i != 0 { write!(f, ",")?; }
			write!(f, "\n\t{:?} @ {}", self.tokens[i], self.token_pos[i])?;
		}
		write!(f, "\n]")
	}
}

/// Lexes a string slice into a `Tokens` container.
pub fn read_tokens(input: &str) -> Result<Tokens, HissyError> {
	let mut tokens = vec![];
	let mut token_pos = vec![];
	let mut it = input.char_indices().peekable();
	let mut indent_levels = vec![""];
	let mut cur_line = 1;
	let mut line_start = 0;
	
	'outer: while let Some((i,c)) = it.peek().copied() {
		if c.is_ascii_whitespace() { // Get indent
			let mut start = i;
			let end;
			loop {
				if let Some((i, c)) = it.peek() {
					if !c.is_ascii_whitespace() {
						end = *i;
						break;
					}
					if *c == '\n' {
						cur_line += 1;
						line_start = i + 1; // Assuming '\n' is always 1 byte
						start = line_start;
					}
					it.next();
				} else { // If at end of file, ignore whitespace
					break 'outer;
				}
			}
			
			let new_indent = &input[start..end];
			let pos = LineCol { line: cur_line, column: 1, offset: start };
			let last_indent = *indent_levels.last().unwrap();
			if last_indent == new_indent {
				token_pos.push(pos);
				tokens.push(Token::Newline);
			} else if new_indent.starts_with(last_indent) {
				indent_levels.push(new_indent);
				token_pos.push(pos);
				tokens.push(Token::Indent);
			} else if let Some(i) = indent_levels.iter().position(|indent| indent == &new_indent) {
				let removed = indent_levels.len() - i - 1;
				indent_levels.resize(i + 1, "");
				for _ in 0..removed {
					token_pos.push(pos.clone());
					tokens.push(Token::Dedent);
				}
				token_pos.push(pos);
				tokens.push(Token::Newline);
			} else {
				return Err(error(format!("Invalid indentation {:?}", new_indent), pos));
			}
			
		} else {
			let pos = LineCol { line: cur_line, column: i - line_start + 1, offset: i };
			token_pos.push(pos.clone());
			
			if c.is_xid_start() {
				let start = i;
				skip_chars(&mut it, &|c| c.is_xid_continue());
				let end = get_next_index(&mut it, input.len());
				let id = &input[start..end];
				if is_keyword(id) {
					tokens.push(Token::Symbol(SmallString::from(id)));
				} else {
					tokens.push(Token::Id(String::from(id)));
				}
			} else if c.is_ascii_digit() {
				let start = i;
				let mut is_integer = true;
				skip_chars(&mut it, &|c| c.is_ascii_digit());
				if test_next_char(&mut it, &|c| c == '.') {
					is_integer = false;
					it.next();
					skip_chars(&mut it, &|c| c.is_ascii_digit());
				}
				if test_next_char(&mut it, &|c| c == 'e' || c == 'E') {
					is_integer = false;
					it.next();
					if test_next_char(&mut it, &|c| c == '+' || c == '-') {
						it.next();
					}
					skip_chars(&mut it, &|c| c.is_ascii_digit());
				}
				let end = get_next_index(&mut it, input.len());
				tokens.push(parse_number(&input[start..end], is_integer));
			} else if c == '"' {
				it.next();
				let mut contents = String::new();
				let mut escaping = false;
				loop {
					let (i,c) = it.next().ok_or_else(|| error_str("Unfinished string literal", pos.clone()))?;
					if escaping {
						if c == '\n' {
							cur_line += 1;
							line_start = i + 1;
						}
						contents.push(match c {
							'\\' | '"' | '\n' => c,
							't' => '\t',
							'r' => '\r',
							'n' => '\n',
							_ => return Err(error(format!("Invalid escape sequence '\\{}' in string", c.escape_default()), pos))
						});
						escaping = false;
					} else if c == '\\' {
						escaping = true;
					} else if c == '"' {
						break;
					} else if c == '\n' {
						return Err(error_str("EOL in the middle of string", pos));
					} else {
						contents.push(c);
					}
				}
				tokens.push(Token::String(contents));
			} else if let Some(s) = parse_symbol(&mut it, c) {
				tokens.push(Token::Symbol(s));
			} else {
				return Err(error(format!("Unexpected character {:?}", c), pos))
			}
		}
		
		skip_chars(&mut it, &|c| c == ' ' || c == '\t');
	}
	
	let i = input.len();
	let pos = LineCol { line: cur_line, column: i - line_start + 1, offset: i };
	
	while indent_levels.len() > 1 {
		indent_levels.pop();
		token_pos.push(pos.clone());
		tokens.push(Token::Dedent);
	}
	
	token_pos.push(pos);
	tokens.push(Token::EOF);
	
	Ok(Tokens { tokens, token_pos })
}

impl Tokens {
	pub fn len(&self) -> usize { self.tokens.len() }
	pub fn is_empty(&self) -> bool { self.tokens.is_empty() }
}

pub struct Position {
	pub(crate) near: Token,
	pub(crate) line: u16,
}

impl fmt::Display for Position {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "line {} near {:?}", self.line, self.near)
	}
}

impl Parse for Tokens {
	type PositionRepr = Position;
	
	fn start(&self) -> usize { 0 }
	fn position_repr(&self, p: usize) -> Self::PositionRepr {
		Position {
			near: self.tokens[p-1].clone(),
			line: self.token_pos[p-1].line as u16,
		}
	}
}

impl ParseElem for Tokens {
	type Element = Token;
	
	fn parse_elem(&self, pos: usize) -> RuleResult<Self::Element> {
		self.tokens.get(pos).map_or(RuleResult::Failed, |t| RuleResult::Matched(pos + 1, t.clone()))
	}
}

impl ParseLiteral for Tokens {
	fn parse_string_literal(&self, pos: usize, literal: &str) -> RuleResult<()> {
		if pos < self.tokens.len() {
			if let Token::Symbol(ss) = &self.tokens[pos] {
				if ss == literal {
					return RuleResult::Matched(pos + 1, ());
				}
			}
		}
		RuleResult::Failed
	}
}

impl<'input> ParseSlice<'input> for Tokens {
	type Slice = &'input [Token];
	
	fn parse_slice(&'input self, p1: usize, p2: usize) -> Self::Slice {
		&self.tokens[p1..p2]
	}
}
