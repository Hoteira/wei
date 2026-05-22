#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    StringLit(String),
    IntLit(i64),
    LParen,
    RParen,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Bang,
    Colon,
    DotDot,
    Indent,
    Dedent,
    Newline,
    Eof,
}

pub fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];
    let mut i = 0;

    while i < bytes.len() {
        let line_start = i;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        let indent = i - line_start;

        if i < bytes.len() && bytes[i] == b'\n' {
            i += 1;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'\n' {
                i += 1;
            }
            continue;
        }
        if i >= bytes.len() {
            break;
        }

        let top = *indent_stack.last().unwrap();
        if indent > top {
            indent_stack.push(indent);
            tokens.push(Token::Indent);
        } else {
            while indent < *indent_stack.last().unwrap() {
                indent_stack.pop();
                tokens.push(Token::Dedent);
            }
            if indent != *indent_stack.last().unwrap() {
                panic!("lex error: inconsistent indentation at byte {}", line_start);
            }
        }

        while i < bytes.len() && bytes[i] != b'\n' {
            let b = bytes[i];
            match b {
                b' ' | b'\t' | b'\r' => {
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
                b'+' => {
                    tokens.push(Token::Plus);
                    i += 1;
                }
                b'-' => {
                    tokens.push(Token::Minus);
                    i += 1;
                }
                b'*' => {
                    tokens.push(Token::Star);
                    i += 1;
                }
                b'/' => {
                    tokens.push(Token::Slash);
                    i += 1;
                }
                b'%' => {
                    tokens.push(Token::Percent);
                    i += 1;
                }
                b'=' if i + 1 < bytes.len() && bytes[i + 1] == b'=' => {
                    tokens.push(Token::EqEq);
                    i += 2;
                }
                b'=' => {
                    tokens.push(Token::Eq);
                    i += 1;
                }
                b'!' if i + 1 < bytes.len() && bytes[i + 1] == b'=' => {
                    tokens.push(Token::BangEq);
                    i += 2;
                }
                b'!' => {
                    tokens.push(Token::Bang);
                    i += 1;
                }
                b'<' if i + 1 < bytes.len() && bytes[i + 1] == b'=' => {
                    tokens.push(Token::LtEq);
                    i += 2;
                }
                b'<' => {
                    tokens.push(Token::Lt);
                    i += 1;
                }
                b'>' if i + 1 < bytes.len() && bytes[i + 1] == b'=' => {
                    tokens.push(Token::GtEq);
                    i += 2;
                }
                b'>' => {
                    tokens.push(Token::Gt);
                    i += 1;
                }
                b':' => {
                    tokens.push(Token::Colon);
                    i += 1;
                }
                b'.' if i + 1 < bytes.len() && bytes[i + 1] == b'.' => {
                    tokens.push(Token::DotDot);
                    i += 2;
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
                    while i < bytes.len()
                        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                    {
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

        if i < bytes.len() && bytes[i] == b'\n' {
            tokens.push(Token::Newline);
            i += 1;
        }
    }

    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(Token::Dedent);
    }

    tokens.push(Token::Eof);
    tokens
}
