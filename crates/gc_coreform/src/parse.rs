use num_bigint::BigInt;
use num_traits::Num;
use thiserror::Error;

use bytes::Bytes;

use crate::term::{Term, TermOrdKey};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unexpected end of input")]
    Eof,
    #[error("unexpected token at byte {at}: {msg}")]
    Unexpected { at: usize, msg: String },
    #[error("invalid escape at byte {at}: {msg}")]
    Escape { at: usize, msg: String },
    #[error("invalid integer at byte {at}: {msg}")]
    Int { at: usize, msg: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Quote,
    Int(BigInt),
    Str(String),
    Bytes(Vec<u8>),
    Symbol(String),
    Eof,
}

struct Lexer<'a> {
    s: &'a str,
    i: usize,
    bytes: &'a [u8],
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            s,
            i: 0,
            bytes: s.as_bytes(),
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.i).copied();
        if b.is_some() {
            self.i += 1;
        }
        b
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            // whitespace
            while matches!(self.peek_byte(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.i += 1;
            }

            // comment: ';' to end of line
            if self.peek_byte() == Some(b';') {
                while let Some(b) = self.bump() {
                    if b == b'\n' {
                        break;
                    }
                }
                continue;
            }

            break;
        }
    }

    fn next(&mut self) -> Result<(Tok, usize), ParseError> {
        self.skip_ws_and_comments();
        let at = self.i;
        let Some(b) = self.peek_byte() else {
            return Ok((Tok::Eof, at));
        };

        let tok = match b {
            b'(' => {
                self.i += 1;
                Tok::LParen
            }
            b')' => {
                self.i += 1;
                Tok::RParen
            }
            b'[' => {
                self.i += 1;
                Tok::LBracket
            }
            b']' => {
                self.i += 1;
                Tok::RBracket
            }
            b'{' => {
                self.i += 1;
                Tok::LBrace
            }
            b'}' => {
                self.i += 1;
                Tok::RBrace
            }
            b'\'' => {
                self.i += 1;
                Tok::Quote
            }
            b'b' => {
                // bytes literal: b"..."
                if self.bytes.get(self.i + 1) == Some(&b'"') {
                    self.i += 2; // consume b"
                    let v = self.read_bytes_string(at)?;
                    Tok::Bytes(v)
                } else {
                    Tok::Symbol(self.read_symbol())
                }
            }
            b'"' => {
                self.i += 1; // consume opening quote
                let v = self.read_string(at)?;
                Tok::Str(v)
            }
            b'-' => {
                if self
                    .bytes
                    .get(self.i + 1)
                    .is_some_and(|c| c.is_ascii_digit())
                {
                    Tok::Int(self.read_int(at)?)
                } else {
                    Tok::Symbol(self.read_symbol())
                }
            }
            _ if b.is_ascii_digit() => Tok::Int(self.read_int(at)?),
            _ => Tok::Symbol(self.read_symbol()),
        };

        Ok((tok, at))
    }

    fn read_symbol(&mut self) -> String {
        let start = self.i;
        while let Some(b) = self.peek_byte() {
            if matches!(
                b,
                b' ' | b'\t'
                    | b'\n'
                    | b'\r'
                    | b'('
                    | b')'
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b'\''
                    | b'"'
                    | b';'
            ) {
                break;
            }
            // Advance by one UTF-8 scalar to keep `self.i` on a char boundary.
            let Some(ch) = self.s[self.i..].chars().next() else {
                break;
            };
            self.i += ch.len_utf8();
        }
        self.s[start..self.i].to_owned()
    }

    fn read_int(&mut self, at: usize) -> Result<BigInt, ParseError> {
        let start = self.i;
        if self.peek_byte() == Some(b'-') {
            self.i += 1;
        }
        while self.peek_byte().is_some_and(|b| b.is_ascii_digit()) {
            self.i += 1;
        }
        let s = &self.s[start..self.i];
        BigInt::from_str_radix(s, 10).map_err(|e| ParseError::Int {
            at,
            msg: e.to_string(),
        })
    }

    fn read_string(&mut self, at: usize) -> Result<String, ParseError> {
        let mut out = String::new();
        loop {
            let Some(b) = self.peek_byte() else {
                break;
            };
            match b {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    let esc_at = self.i;
                    self.i += 1; // consume backslash
                    let Some(e) = self.bump() else {
                        return Err(ParseError::Escape {
                            at: esc_at,
                            msg: "unterminated escape".to_string(),
                        });
                    };
                    match e {
                        b'\\' => out.push('\\'),
                        b'"' => out.push('"'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let cp = self.read_hex_u32(4, esc_at)?;
                            let Some(ch) = char::from_u32(cp) else {
                                return Err(ParseError::Escape {
                                    at: esc_at,
                                    msg: "invalid unicode codepoint".to_string(),
                                });
                            };
                            out.push(ch);
                        }
                        b'x' => {
                            let v = self.read_hex_u32(2, esc_at)? as u8;
                            out.push(v as char);
                        }
                        _ => {
                            return Err(ParseError::Escape {
                                at: esc_at,
                                msg: format!("unknown escape: {}", e as char),
                            });
                        }
                    }
                }
                _ => {
                    let Some(ch) = self.s[self.i..].chars().next() else {
                        return Err(ParseError::Unexpected {
                            at,
                            msg: "invalid string character boundary".to_string(),
                        });
                    };
                    out.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
        Err(ParseError::Unexpected {
            at,
            msg: "unterminated string".to_string(),
        })
    }

    fn read_bytes_string(&mut self, at: usize) -> Result<Vec<u8>, ParseError> {
        let mut out = Vec::new();
        while let Some(b) = self.bump() {
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc_at = self.i.saturating_sub(1);
                    let Some(e) = self.bump() else {
                        return Err(ParseError::Escape {
                            at: esc_at,
                            msg: "unterminated escape".to_string(),
                        });
                    };
                    match e {
                        b'\\' => out.push(b'\\'),
                        b'"' => out.push(b'"'),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'x' => {
                            let v = self.read_hex_u32(2, esc_at)? as u8;
                            out.push(v);
                        }
                        b'u' => {
                            // Encode as UTF-8 bytes.
                            let cp = self.read_hex_u32(4, esc_at)?;
                            let Some(ch) = char::from_u32(cp) else {
                                return Err(ParseError::Escape {
                                    at: esc_at,
                                    msg: "invalid unicode codepoint".to_string(),
                                });
                            };
                            let mut buf = [0u8; 4];
                            let s = ch.encode_utf8(&mut buf);
                            out.extend_from_slice(s.as_bytes());
                        }
                        _ => {
                            return Err(ParseError::Escape {
                                at: esc_at,
                                msg: format!("unknown escape: {}", e as char),
                            });
                        }
                    }
                }
                _ => out.push(b),
            }
        }
        Err(ParseError::Unexpected {
            at,
            msg: "unterminated bytes literal".to_string(),
        })
    }

    fn read_hex_u32(&mut self, n: usize, at: usize) -> Result<u32, ParseError> {
        let mut v: u32 = 0;
        for _ in 0..n {
            let Some(b) = self.bump() else {
                return Err(ParseError::Escape {
                    at,
                    msg: "unterminated hex escape".to_string(),
                });
            };
            let d = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => (b - b'a' + 10) as u32,
                b'A'..=b'F' => (b - b'A' + 10) as u32,
                _ => {
                    return Err(ParseError::Escape {
                        at,
                        msg: "invalid hex digit".to_string(),
                    });
                }
            };
            v = (v << 4) | d;
        }
        Ok(v)
    }
}

struct Parser<'a> {
    lex: Lexer<'a>,
    peek: Option<(Tok, usize)>,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            lex: Lexer::new(s),
            peek: None,
        }
    }

    fn peek(&mut self) -> Result<&(Tok, usize), ParseError> {
        if self.peek.is_none() {
            self.peek = Some(self.lex.next()?);
        }
        self.peek.as_ref().ok_or_else(|| ParseError::Unexpected {
            at: self.lex.i,
            msg: "parser internal error: missing lookahead token".to_string(),
        })
    }

    fn bump(&mut self) -> Result<(Tok, usize), ParseError> {
        if let Some(t) = self.peek.take() {
            return Ok(t);
        }
        self.lex.next()
    }

    fn parse_term(&mut self) -> Result<Term, ParseError> {
        // Parser is structurally recursive on nested CoreForm terms; grow stack as needed.
        stacker::maybe_grow(32 * 1024, 1024 * 1024, || self.parse_term_impl())
    }

    fn parse_term_impl(&mut self) -> Result<Term, ParseError> {
        let (t, at) = self.bump()?;
        match t {
            Tok::Eof => Err(ParseError::Eof),
            Tok::Quote => {
                let inner = self.parse_term()?;
                Ok(Term::list(vec![Term::symbol("quote"), inner]))
            }
            Tok::LParen => self.parse_list(at),
            Tok::LBracket => self.parse_vector(at),
            Tok::LBrace => self.parse_map(at),
            Tok::Int(x) => Ok(Term::Int(x)),
            Tok::Str(s) => Ok(Term::Str(s)),
            Tok::Bytes(b) => Ok(Term::Bytes(Bytes::from(b))),
            Tok::Symbol(s) => match s.as_str() {
                "nil" => Ok(Term::Nil),
                "true" => Ok(Term::Bool(true)),
                "false" => Ok(Term::Bool(false)),
                _ => Ok(Term::Symbol(s)),
            },
            Tok::RParen | Tok::RBracket | Tok::RBrace => Err(ParseError::Unexpected {
                at,
                msg: "unexpected closing delimiter".to_string(),
            }),
        }
    }

    fn parse_list(&mut self, at: usize) -> Result<Term, ParseError> {
        let mut items = Vec::new();
        loop {
            let (t, _) = self.peek()?.clone();
            if matches!(t, Tok::RParen) {
                self.bump()?;
                break;
            }
            if matches!(t, Tok::Eof) {
                return Err(ParseError::Unexpected {
                    at,
                    msg: "unterminated list".to_string(),
                });
            }
            items.push(self.parse_term()?);
        }
        Ok(Term::list(items))
    }

    fn parse_vector(&mut self, at: usize) -> Result<Term, ParseError> {
        let mut items = Vec::new();
        loop {
            let (t, _) = self.peek()?.clone();
            if matches!(t, Tok::RBracket) {
                self.bump()?;
                break;
            }
            if matches!(t, Tok::Eof) {
                return Err(ParseError::Unexpected {
                    at,
                    msg: "unterminated vector".to_string(),
                });
            }
            items.push(self.parse_term()?);
        }
        Ok(Term::Vector(items))
    }

    fn parse_map(&mut self, at: usize) -> Result<Term, ParseError> {
        let mut items = Vec::new();
        loop {
            let (t, _) = self.peek()?.clone();
            if matches!(t, Tok::RBrace) {
                self.bump()?;
                break;
            }
            if matches!(t, Tok::Eof) {
                return Err(ParseError::Unexpected {
                    at,
                    msg: "unterminated map".to_string(),
                });
            }
            items.push(self.parse_term()?);
        }
        if items.len() % 2 != 0 {
            return Err(ParseError::Unexpected {
                at,
                msg: "map literal must have even number of forms".to_string(),
            });
        }
        let mut m = std::collections::BTreeMap::new();
        for pair in items.chunks(2) {
            let k = pair[0].clone();
            let v = pair[1].clone();
            m.insert(TermOrdKey(k), v);
        }
        Ok(Term::Map(m))
    }

    fn parse_module(&mut self) -> Result<Vec<Term>, ParseError> {
        let mut forms = Vec::new();
        loop {
            let (t, _) = self.peek()?.clone();
            if matches!(t, Tok::Eof) {
                self.bump()?;
                break;
            }
            forms.push(self.parse_term()?);
        }
        Ok(forms)
    }
}

pub fn parse_term(src: &str) -> Result<Term, ParseError> {
    let mut p = Parser::new(src);
    let t = p.parse_term()?;
    let (next, at) = p.bump()?;
    match next {
        Tok::Eof => Ok(t),
        _ => Err(ParseError::Unexpected {
            at,
            msg: "trailing tokens after term".to_string(),
        }),
    }
}

pub fn parse_module(src: &str) -> Result<Vec<Term>, ParseError> {
    Parser::new(src).parse_module()
}
