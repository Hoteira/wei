use crate::ast::{BinOp, CmpOp, Expr, Program, Stmt, TypeExpr};
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
        if let Token::Ident(name) = self.peek() {
            if name == "let" {
                return self.parse_let();
            }
            if name == "par" {
                return self.parse_par();
            }
            if name == "for" {
                return self.parse_for();
            }
            if name == "while" {
                return self.parse_while();
            }
            if matches!(self.tokens.get(self.pos + 1), Some(Token::Eq)) {
                return self.parse_assign();
            }
        }
        self.parse_call()
    }

    fn parse_while(&mut self) -> Stmt {
        self.pos += 1; // consume "while"
        let cond = self.parse_expr();
        self.expect_colon();
        self.skip_newlines();
        self.expect_indent();
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::Dedent | Token::Eof) {
            body.push(self.parse_statement());
            self.skip_newlines();
        }
        self.expect_dedent();
        Stmt::While { cond, body }
    }

    fn parse_for(&mut self) -> Stmt {
        self.pos += 1; // consume "for"
        let var = self.expect_ident();
        let in_kw = self.expect_ident();
        if in_kw != "in" {
            panic!("parse error: expected 'in' after for variable, got '{}'", in_kw);
        }
        let start = self.parse_expr();
        self.expect_dotdot();
        let end = self.parse_expr();
        self.expect_colon();
        self.skip_newlines();
        self.expect_indent();
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::Dedent | Token::Eof) {
            body.push(self.parse_statement());
            self.skip_newlines();
        }
        self.expect_dedent();
        Stmt::For {
            var,
            start,
            end,
            body,
        }
    }

    fn parse_par(&mut self) -> Stmt {
        self.pos += 1; // consume "par"
        let name = self.expect_ident();
        self.expect_colon();
        self.skip_newlines();
        self.expect_indent();
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::Dedent | Token::Eof) {
            body.push(self.parse_statement());
            self.skip_newlines();
        }
        self.expect_dedent();
        Stmt::Par { name, body }
    }

    fn parse_assign(&mut self) -> Stmt {
        let name = self.expect_ident();
        self.expect_eq();
        let value = self.parse_expr();
        Stmt::Assign { name, value }
    }

    fn parse_let(&mut self) -> Stmt {
        self.pos += 1; // consume "let"
        let name = self.expect_ident();
        let ty = self.parse_type();
        self.expect_eq();
        let init = self.parse_expr();
        Stmt::Let { name, ty, init }
    }

    fn parse_type(&mut self) -> TypeExpr {
        let name = self.expect_ident();
        self.expect_lparen();
        let n = match self.tokens[self.pos].clone() {
            Token::IntLit(n) => {
                self.pos += 1;
                n
            }
            other => panic!("parse error: expected integer in type, got {:?}", other),
        };
        self.expect_rparen();
        match name.as_str() {
            "uint" => TypeExpr::UInt(n as u32),
            other => panic!("parse error: unknown type `{}`", other),
        }
    }

    fn parse_call(&mut self) -> Stmt {
        let name = self.expect_ident();
        self.expect_lparen();
        let args = if matches!(self.peek(), Token::RParen) {
            Vec::new()
        } else {
            vec![self.parse_expr()]
        };
        self.expect_rparen();
        Stmt::Call { name, args }
    }

    fn parse_expr(&mut self) -> Expr {
        let left = self.parse_additive();
        if let Some(op) = self.peek_cmp_op() {
            self.pos += 1;
            let right = self.parse_additive();
            return Expr::Compare {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_additive(&mut self) -> Expr {
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
        let mut left = self.parse_unary();
        while let Some(op) = self.peek_mul_op() {
            self.pos += 1;
            let right = self.parse_unary();
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_unary(&mut self) -> Expr {
        if matches!(self.peek(), Token::Bang) {
            self.pos += 1;
            let inner = self.parse_unary();
            return Expr::Not {
                inner: Box::new(inner),
            };
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Expr {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        match t {
            Token::StringLit(s) => Expr::StringLit(s),
            Token::IntLit(n) => Expr::IntLit(n),
            Token::Ident(n) => Expr::Ident(n),
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

    fn peek_cmp_op(&self) -> Option<CmpOp> {
        match self.peek() {
            Token::EqEq => Some(CmpOp::Eq),
            Token::BangEq => Some(CmpOp::Ne),
            Token::Lt => Some(CmpOp::Lt),
            Token::LtEq => Some(CmpOp::Le),
            Token::Gt => Some(CmpOp::Gt),
            Token::GtEq => Some(CmpOp::Ge),
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

    fn expect_eq(&mut self) {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        if !matches!(t, Token::Eq) {
            panic!("parse error: expected '=', got {:?}", t);
        }
    }

    fn expect_colon(&mut self) {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        if !matches!(t, Token::Colon) {
            panic!("parse error: expected ':', got {:?}", t);
        }
    }

    fn expect_indent(&mut self) {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        if !matches!(t, Token::Indent) {
            panic!("parse error: expected indent, got {:?}", t);
        }
    }

    fn expect_dedent(&mut self) {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        if !matches!(t, Token::Dedent) {
            panic!("parse error: expected dedent, got {:?}", t);
        }
    }

    fn expect_dotdot(&mut self) {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        if !matches!(t, Token::DotDot) {
            panic!("parse error: expected '..', got {:?}", t);
        }
    }
}
