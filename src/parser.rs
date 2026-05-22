use crate::ast::{BinOp, Expr, Program, Stmt};
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
        let arg = self.parse_expr();
        self.expect_rparen();
        Stmt::Call {
            name,
            args: vec![arg],
        }
    }

    fn parse_expr(&mut self) -> Expr {
        let mut left = self.parse_term();
        while let Some(op) = self.peek_add_op() {
            self.pos += 1;
            let right = self.parse_term();
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_term(&mut self) -> Expr {
        let mut left = self.parse_atom();
        while let Some(op) = self.peek_mul_op() {
            self.pos += 1;
            let right = self.parse_atom();
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_atom(&mut self) -> Expr {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        match t {
            Token::StringLit(s) => Expr::StringLit(s),
            Token::IntLit(n) => Expr::IntLit(n),
            Token::LParen => {
                let e = self.parse_expr();
                self.expect_rparen();
                e
            }
            other => panic!("parse error: expected expression, got {:?}", other),
        }
    }

    fn peek_add_op(&self) -> Option<BinOp> {
        match self.peek() {
            Token::Plus => Some(BinOp::Add),
            Token::Minus => Some(BinOp::Sub),
            _ => None,
        }
    }

    fn peek_mul_op(&self) -> Option<BinOp> {
        match self.peek() {
            Token::Star => Some(BinOp::Mul),
            Token::Slash => Some(BinOp::Div),
            Token::Percent => Some(BinOp::Mod),
            _ => None,
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
