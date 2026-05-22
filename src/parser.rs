use crate::ast::{Expr, Program, Stmt};
use crate::lexer::Token;

pub fn parse(tokens: &[Token]) -> Program {
    let mut p = Parser { tokens, pos: 0 };
    let mut statements = Vec::new();

    p.skip_newlines();
    while !p.at_eof() {
        statements.push(p.parse_statement());
        p.skip_newlines();
    }

    Program { statements }
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.pos += 1;
        }
    }

    fn parse_statement(&mut self) -> Stmt {
        let name = self.expect_ident();
        self.expect_lparen();
        let arg = self.expect_string_lit();
        self.expect_rparen();
        Stmt::Call {
            name,
            args: vec![Expr::StringLit(arg)],
        }
    }

    fn expect_ident(&mut self) -> String {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        match t {
            Token::Ident(n) => n,
            other => panic!("parse error: expected identifier, got {:?}", other),
        }
    }

    fn expect_string_lit(&mut self) -> String {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        match t {
            Token::StringLit(s) => s,
            other => panic!("parse error: expected string literal, got {:?}", other),
        }
    }

    fn expect_lparen(&mut self) {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        if !matches!(t, Token::LParen) {
            panic!("parse error: expected '(', got {:?}", t);
        }
    }

    fn expect_rparen(&mut self) {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        if !matches!(t, Token::RParen) {
            panic!("parse error: expected ')', got {:?}", t);
        }
    }
}
