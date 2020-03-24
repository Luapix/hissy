
use std::str::CharIndices;
use std::iter::Peekable;
use std::fmt;
use unicode_xid::UnicodeXID;
use peg::{Parse, ParseElem, ParseLiteral, ParseSlice, RuleResult, str::LineCol};
use smallstr::SmallString;

type SymbolStr = SmallString<[u8;6]>;

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
	Symbol(SymbolStr),
	Id(String),
	Int(i32),
	Real(f64),
	String(String),
	Newline, Indent, Dedent,
}

static KEYWORDS: [&'static str; 13] = [
	"let", "if", "else", "while",
	"not", "and", "or",
	"nil", "true", "false",
	"return", "log",
	"fun"
];

fn is_keyword(s: &str) -> bool {
	KEYWORDS.contains(&s)
}

fn parse_number(input: &str, is_integer: bool) -> Result<Token, String> {
	if is_integer {
		if let Ok(i) = input.parse::<i32>() {
			return Ok(Token::Int(i));
		}
	}
	input.parse::<f64>()
		.map(|r| Token::Real(r))
		.map_err(|_| "Error while parsing real literal".to_string())
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

pub struct Tokens {
	tokens: Vec<Token>,
	token_pos: Vec<LineCol>,
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

pub fn read_tokens(input: &str) -> Result<Tokens, String> {
	let mut tokens = vec![];
	let mut token_pos = vec![];
	let mut it = input.char_indices().peekable();
	let mut indent_levels = vec![""];
	let mut cur_line = 1;
	let mut line_start = 0;
	let mut at_start = true;
	while let Some((i, c)) = it.peek().copied() {
		if at_start || c == '\r' || c == '\n' { // Get new indent
			if at_start { at_start = false; }
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
				} else {
					end = input.len();
					break;
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
				return Err("Invalid indentation: ".to_string() + &format!("{:?}", new_indent));
			}
		} else {
			token_pos.push(LineCol { line: cur_line, column: i - line_start + 1, offset: i });
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
				tokens.push(parse_number(&input[start..end], is_integer)?);
			} else if c == '"' {
				it.next();
				let mut contents = String::new();
				let mut escaping = false;
				loop {
					let (_,c) = it.next().ok_or("Unfinished string literal")?;
					if escaping {
						contents.push(match c {
							'\\' | '"' => c,
							't' => '\t',
							'r' => '\r',
							'n' => '\n',
							_ => return Err("Invalid escape sequence".to_string())
						});
					} else {
						if c == '\\' {
							escaping = true;
						} else if c == '"' {
							break;
						} else {
							contents.push(c);
						}
					}
				}
				tokens.push(Token::String(contents));
			} else {
				if let Some(s) = parse_symbol(&mut it, c) {
					tokens.push(Token::Symbol(s));
				} else {
					return Err("Unexpected character: '".to_string() + &c.escape_default().collect::<String>() + "'")
				}
			}
		}
		
		skip_chars(&mut it, &|c| c == ' ' || c == '\t');
	}
	Ok(Tokens { tokens: tokens, token_pos: token_pos })
}

impl Tokens {
	pub fn len(&self) -> usize { self.tokens.len() }
}

impl Parse for Tokens {
	type PositionRepr = String;
	
	fn start(&self) -> usize { 0 }
	fn position_repr(&self, p: usize) -> Self::PositionRepr {
		self.tokens.get(p).map_or("EOF".to_string(),
			|t| format!("{} near {:?}", self.token_pos[p], t))
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
