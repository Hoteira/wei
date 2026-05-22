#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    StringLit(String),
    IntLit(i64),
    LParen,
    RParen,
    Newline,
    Eof,
}

pub fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' | b'\r' => {
                i += 1;
            }
            b'\n' => {
                tokens.push(Token::Newline);
                i += 1;
            }
            b'(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            b')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                i += 1; // opening quote
                let start = i;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += 1;
                }
                if i >= bytes.len() {
                    panic!("lex error: unterminated string literal");
                }
                let content = std::str::from_utf8(&bytes[start..i])
                    .expect("lex error: invalid UTF-8 in string literal")
                    .to_string();
                tokens.push(Token::StringLit(content));
                i += 1; // closing quote
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let name = std::str::from_utf8(&bytes[start..i])
                    .expect("lex error: invalid UTF-8 in identifier")
                    .to_string();
                tokens.push(Token::Ident(name));
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                    i += 1;
                }
                let raw = std::str::from_utf8(&bytes[start..i]).unwrap();
                let cleaned: String = raw.chars().filter(|&c| c != '_').collect();
                let value: i64 = cleaned
                    .parse()
                    .unwrap_or_else(|_| panic!("lex error: invalid integer literal {:?}", raw));
                tokens.push(Token::IntLit(value));
            }
            _ => {
                panic!(
                    "lex error: unexpected character {:?} at byte {}",
                    b as char, i
                );
            }
        }
    }

    tokens.push(Token::Eof);
    tokens
}
