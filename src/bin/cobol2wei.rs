use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::process;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    StringLit(String),
    IntLit(i64),
    LParen,
    RParen,
    Period,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Eof,
}

fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
        } else if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if b == b'.' {
            tokens.push(Token::Period);
            i += 1;
        } else if b == b',' {
            tokens.push(Token::Comma);
            i += 1;
        } else if b == b'(' {
            tokens.push(Token::LParen);
            i += 1;
        } else if b == b')' {
            tokens.push(Token::RParen);
            i += 1;
        } else if b == b'+' {
            tokens.push(Token::Plus);
            i += 1;
        } else if b == b'-' {
            tokens.push(Token::Minus);
            i += 1;
        } else if b == b'*' {
            tokens.push(Token::Star);
            i += 1;
        } else if b == b'/' {
            tokens.push(Token::Slash);
            i += 1;
        } else if b == b'<' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                tokens.push(Token::LtEq);
                i += 2;
            } else {
                tokens.push(Token::Lt);
                i += 1;
            }
        } else if b == b'>' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                tokens.push(Token::GtEq);
                i += 2;
            } else {
                tokens.push(Token::Gt);
                i += 1;
            }
        } else if b == b'=' {
            tokens.push(Token::Eq);
            i += 1;
        } else if b == b'"' || b == b'\'' {
            let quote = b;
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i >= bytes.len() {
                panic!("cobol2wei: unterminated string literal");
            }
            let content = std::str::from_utf8(&bytes[start..i])
                .expect("cobol2wei: invalid UTF-8 in string")
                .to_string();
            tokens.push(Token::StringLit(content));
            i += 1;
        } else if b.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let v: i64 = std::str::from_utf8(&bytes[start..i])
                .unwrap()
                .parse()
                .expect("cobol2wei: invalid integer");
            tokens.push(Token::IntLit(v));
        } else if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
            {
                i += 1;
            }
            let word = std::str::from_utf8(&bytes[start..i])
                .expect("cobol2wei: invalid UTF-8 in word")
                .to_uppercase();
            tokens.push(Token::Word(word));
        } else {
            panic!(
                "cobol2wei: unexpected character {:?} at byte {}",
                b as char, i
            );
        }
    }

    tokens.push(Token::Eof);
    tokens
}

#[derive(Debug, Clone)]
enum PicType {
    Str(u32),
    UInt(u32),
    UDec(u32, u32),
}

#[derive(Debug, Clone)]
enum Literal {
    Str(String),
    Int(i64),
}

#[derive(Debug)]
struct FileDecl {
    name: String,
    path: String,
    org: String,
    key: Option<String>,
}

#[derive(Debug)]
struct RecordType {
    type_name: String,
    var_name: String,
    fields: Vec<RecField>,
}

#[derive(Debug)]
struct RecField {
    name: String,
    kind: FieldKind,
    redefines: Option<String>,
}

#[derive(Debug)]
enum FieldKind {
    Pic(PicType),
    Sub(String),
}

#[derive(Debug)]
struct WSScalar {
    name: String,
    ty: PicType,
    value: Option<Literal>,
    eighty_eights: Vec<(String, Literal)>,
}

#[derive(Debug)]
struct FdBinding {
    file_name: String,
    record_var: String,
}

#[derive(Debug)]
enum Expr {
    Ident(String),
    Int(i64),
    Str(String),
    Bin {
        op: ArithOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy)]
enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug)]
enum Cond {
    Cmp { op: CmpOp, left: Expr, right: Expr },
    Bare(String),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
}

#[derive(Debug)]
enum Stmt {
    Display(Expr),
    Move { value: Expr, target: String },
    Add { value: Expr, target: String },
    Subtract { value: Expr, target: String },
    Compute { target: String, expr: Expr },
    Perform { para: String },
    PerformUntil { cond: Cond, body: Vec<Stmt> },
    ForRange {
        var: String,
        start: Expr,
        end: Expr,
        end_inclusive: bool,
        body: Vec<Stmt>,
    },
    If {
        cond: Cond,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    Open { mode: String, file: String },
    Read { file: String, at_end_target: Option<String> },
    Write { rec: String },
    Close { file: String },
    Evaluate { expr: Expr, arms: Vec<EvalArm> },
    CallSub { name: String, args: Vec<Expr> },
    InspectTally { subject: String, counter: String, needle: String },
    Goto { label: String },
    ExitProgram,
    StopRun,
}

#[derive(Debug)]
enum EvalPattern {
    Lit(i64),
    Thru(i64, i64),
    StrLit(String),
    Other,
}

#[derive(Debug)]
struct EvalArm {
    pattern: EvalPattern,
    body: Vec<Stmt>,
}

#[derive(Debug)]
struct Paragraph {
    name: String,
    body: Vec<Stmt>,
}

#[derive(Debug)]
struct SubDef {
    name: String,
    params: Vec<(String, PicType)>,
    body: Vec<Stmt>,
}

#[derive(Debug, Default)]
struct Program {
    files: Vec<FileDecl>,
    records: Vec<RecordType>,
    fds: Vec<FdBinding>,
    ws_scalars: Vec<WSScalar>,
    main_code: Vec<Stmt>,
    paragraphs: Vec<Paragraph>,
    subs: Vec<SubDef>,
}

fn to_wei_ident(s: &str) -> String {
    s.to_lowercase().replace('-', "_")
}

fn to_cobol_name(wei: &str) -> String {
    wei.to_uppercase().replace('_', "-")
}

fn to_pascal_case(cobol: &str) -> String {
    cobol
        .split('-')
        .map(|seg| {
            let mut c = seg.chars();
            match c.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string()
                        + &c.as_str().to_ascii_lowercase()
                }
                None => String::new(),
            }
        })
        .collect()
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_n(&self, n: usize) -> &Token {
        self.tokens.get(self.pos + n).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        t
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn parse_program(&mut self) -> Program {
        let mut program = Program::default();
        self.parse_main_program(&mut program);
        while matches!(self.peek(), Token::Word(w) if w == "IDENTIFICATION") {
            let sub = self.parse_subprogram();
            program.subs.push(sub);
        }
        program
    }

    fn parse_main_program(&mut self, program: &mut Program) {
        // Skip leading IDENTIFICATION DIVISION header for the first program.
        if matches!(self.peek(), Token::Word(w) if w == "IDENTIFICATION") {
            loop {
                match self.peek() {
                    Token::Word(w)
                        if w == "ENVIRONMENT" || w == "DATA" || w == "PROCEDURE" =>
                    {
                        break
                    }
                    Token::Eof => return,
                    _ => {
                        self.advance();
                    }
                }
            }
        }
        while !self.at_eof() {
            if matches!(self.peek(), Token::Word(w) if w == "IDENTIFICATION") {
                break;
            }
            match self.peek() {
                Token::Word(w) if w == "ENVIRONMENT" => {
                    self.advance();
                    self.expect_word("DIVISION");
                    self.expect_period();
                    self.parse_environment_division(program);
                }
                Token::Word(w) if w == "DATA" => {
                    self.advance();
                    self.expect_word("DIVISION");
                    self.expect_period();
                    self.parse_data_division(program);
                }
                Token::Word(w) if w == "PROCEDURE" => {
                    self.advance();
                    self.expect_word("DIVISION");
                    self.expect_period();
                    self.parse_procedure_division(program);
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_subprogram(&mut self) -> SubDef {
        self.expect_word("IDENTIFICATION");
        self.expect_word("DIVISION");
        self.expect_period();
        // Skip until PROGRAM-ID.
        loop {
            match self.peek() {
                Token::Word(w) if w == "PROGRAM-ID" => {
                    self.advance();
                    self.expect_period();
                    break;
                }
                Token::Eof => panic!("cobol2wei: expected PROGRAM-ID in subprogram"),
                _ => {
                    self.advance();
                }
            }
        }
        let name_cobol = self.expect_word_any();
        self.expect_period();
        let sub_name = to_wei_ident(&name_cobol);

        let mut linkage: Vec<(String, PicType)> = Vec::new();

        loop {
            match self.peek().clone() {
                Token::Word(ref w) if w == "DATA" => {
                    self.advance();
                    self.expect_word("DIVISION");
                    self.expect_period();
                    loop {
                        match self.peek().clone() {
                            Token::Word(ref w) if w == "LINKAGE" => {
                                self.advance();
                                self.expect_word("SECTION");
                                self.expect_period();
                                loop {
                                    match self.peek().clone() {
                                        Token::IntLit(1) => {
                                            self.advance();
                                            let pn = self.expect_word_any();
                                            self.expect_word("PIC");
                                            let ty = self.parse_pic();
                                            self.expect_period();
                                            linkage.push((to_wei_ident(&pn), ty));
                                        }
                                        Token::Word(ref w) if w == "PROCEDURE" => break,
                                        Token::Eof => break,
                                        other => panic!(
                                            "cobol2wei: unexpected in LINKAGE: {:?}",
                                            other
                                        ),
                                    }
                                }
                                break;
                            }
                            Token::Word(ref w) if w == "PROCEDURE" => break,
                            Token::Eof => break,
                            _ => {
                                self.advance();
                            }
                        }
                    }
                }
                Token::Word(ref w) if w == "PROCEDURE" => break,
                Token::Eof => panic!("cobol2wei: subprogram missing PROCEDURE DIVISION"),
                _ => {
                    self.advance();
                }
            }
        }

        self.expect_word("PROCEDURE");
        self.expect_word("DIVISION");
        let mut params: Vec<(String, PicType)> = Vec::new();
        if matches!(self.peek(), Token::Word(w) if w == "USING") {
            self.advance();
            loop {
                let pn = self.expect_word_any();
                let key = to_wei_ident(&pn);
                let ty = linkage
                    .iter()
                    .find(|(n, _)| n == &key)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_else(|| {
                        panic!("cobol2wei: USING param `{}` not in LINKAGE", pn)
                    });
                params.push((key, ty));
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect_period();

        let mut body: Vec<Stmt> = Vec::new();
        loop {
            if self.at_eof() {
                break;
            }
            if matches!(self.peek(), Token::Word(w) if w == "IDENTIFICATION") {
                break;
            }
            if let Token::Word(w) = self.peek() {
                if !is_verb(w) && matches!(self.peek_n(1), Token::Period) {
                    self.advance();
                    self.expect_period();
                    continue;
                }
            }
            let s = self.parse_statement(false);
            if matches!(s, Stmt::ExitProgram) {
                break;
            }
            body.push(s);
        }

        SubDef {
            name: sub_name,
            params,
            body,
        }
    }

    fn parse_environment_division(&mut self, program: &mut Program) {
        loop {
            match self.peek() {
                Token::Word(w) if w == "INPUT-OUTPUT" => {
                    self.advance();
                    self.expect_word("SECTION");
                    self.expect_period();
                }
                Token::Word(w) if w == "FILE-CONTROL" => {
                    self.advance();
                    self.expect_period();
                    self.parse_file_control(program);
                }
                Token::Word(w) if w == "DATA" || w == "PROCEDURE" => return,
                Token::Eof => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_file_control(&mut self, program: &mut Program) {
        loop {
            match self.peek() {
                Token::Word(w) if w == "SELECT" => {
                    self.advance();
                    let fname = self.expect_word_any();
                    self.expect_word("ASSIGN");
                    self.expect_word("TO");
                    let path = match self.advance() {
                        Token::StringLit(s) => s,
                        other => panic!(
                            "cobol2wei: SELECT ASSIGN TO expected string literal, got {:?}",
                            other
                        ),
                    };
                    let mut org = "sequential".to_string();
                    let mut key: Option<String> = None;
                    while !matches!(self.peek(), Token::Period) {
                        match self.peek().clone() {
                            Token::Word(w) if w == "ORGANIZATION" => {
                                self.advance();
                                if matches!(self.peek(), Token::Word(w) if w == "IS") {
                                    self.advance();
                                }
                                let v = self.expect_word_any();
                                org = v.to_lowercase();
                            }
                            Token::Word(w) if w == "ACCESS" => {
                                self.advance();
                                if matches!(self.peek(), Token::Word(w) if w == "MODE") {
                                    self.advance();
                                }
                                if matches!(self.peek(), Token::Word(w) if w == "IS") {
                                    self.advance();
                                }
                                let _ = self.expect_word_any();
                            }
                            Token::Word(w) if w == "RECORD" => {
                                self.advance();
                                self.expect_word("KEY");
                                if matches!(self.peek(), Token::Word(w) if w == "IS") {
                                    self.advance();
                                }
                                let k = self.expect_word_any();
                                key = Some(to_wei_ident(&k));
                            }
                            Token::Eof => break,
                            _ => {
                                self.advance();
                            }
                        }
                    }
                    self.expect_period();
                    program.files.push(FileDecl {
                        name: to_wei_ident(&fname),
                        path,
                        org,
                        key,
                    });
                }
                Token::Word(w) if w == "DATA" || w == "PROCEDURE" => return,
                Token::Eof => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_data_division(&mut self, program: &mut Program) {
        loop {
            if self.at_eof() {
                return;
            }
            match self.peek().clone() {
                Token::Word(ref w) if w == "FILE" && matches!(self.peek_n(1), Token::Word(s) if s == "SECTION") => {
                    self.advance();
                    self.advance();
                    self.expect_period();
                    self.parse_file_section(program);
                }
                Token::Word(ref w) if w == "WORKING-STORAGE" => {
                    self.advance();
                    self.expect_word("SECTION");
                    self.expect_period();
                    self.parse_working_storage(program);
                }
                Token::Word(ref w) if w == "PROCEDURE" => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_file_section(&mut self, program: &mut Program) {
        loop {
            match self.peek().clone() {
                Token::Word(ref w) if w == "FD" => {
                    self.advance();
                    let file_cobol = self.expect_word_any();
                    self.expect_period();
                    let level = self.expect_intlit();
                    if level != 1 {
                        panic!("cobol2wei: expected 01-level after FD, got {}", level);
                    }
                    let rec_cobol = self.expect_word_any();
                    self.expect_period();
                    let record_var = to_wei_ident(&rec_cobol);
                    let type_name = to_pascal_case(&rec_cobol);
                    let fields = self.parse_group_fields(program, 1, &type_name);
                    program.records.push(RecordType {
                        type_name,
                        var_name: record_var.clone(),
                        fields,
                    });
                    program.fds.push(FdBinding {
                        file_name: to_wei_ident(&file_cobol),
                        record_var,
                    });
                }
                Token::Word(ref w) if w == "WORKING-STORAGE" || w == "PROCEDURE" => return,
                Token::Eof => return,
                other => panic!("cobol2wei: unexpected token in FILE SECTION: {:?}", other),
            }
        }
    }

    fn parse_working_storage(&mut self, program: &mut Program) {
        loop {
            match self.peek().clone() {
                Token::IntLit(1) => {
                    self.advance();
                    let name = self.expect_word_any();
                    if matches!(self.peek(), Token::Period) {
                        self.advance();
                        let type_name = to_pascal_case(&name);
                        let fields = self.parse_group_fields(program, 1, &type_name);
                        program.records.push(RecordType {
                            type_name,
                            var_name: to_wei_ident(&name),
                            fields,
                        });
                        continue;
                    }
                    self.expect_word("PIC");
                    let ty = self.parse_pic();
                    let value = if matches!(self.peek(), Token::Word(w) if w == "VALUE") {
                        self.advance();
                        Some(self.parse_literal())
                    } else {
                        None
                    };
                    self.expect_period();
                    let mut scalar = WSScalar {
                        name: to_wei_ident(&name),
                        ty,
                        value,
                        eighty_eights: Vec::new(),
                    };
                    while matches!(self.peek(), Token::IntLit(88)) {
                        self.advance();
                        let cname = self.expect_word_any();
                        self.expect_word("VALUE");
                        let v = self.parse_literal();
                        self.expect_period();
                        scalar.eighty_eights.push((to_wei_ident(&cname), v));
                    }
                    program.ws_scalars.push(scalar);
                }
                Token::Word(ref w) if w == "PROCEDURE" => return,
                Token::Eof => return,
                other => panic!(
                    "cobol2wei: unexpected token in WORKING-STORAGE: {:?}",
                    other
                ),
            }
        }
    }

    fn parse_group_fields(
        &mut self,
        program: &mut Program,
        parent_level: i64,
        parent_type: &str,
    ) -> Vec<RecField> {
        let mut fields = Vec::new();
        loop {
            let level = match self.peek() {
                Token::IntLit(n) if *n > parent_level && *n != 88 && *n < 50 => *n,
                _ => break,
            };
            self.advance();
            let fname_cobol = self.expect_word_any();
            let fname = to_wei_ident(&fname_cobol);
            let redefines = if matches!(self.peek(), Token::Word(w) if w == "REDEFINES") {
                self.advance();
                let target = self.expect_word_any();
                Some(to_wei_ident(&target))
            } else {
                None
            };
            if matches!(self.peek(), Token::Word(w) if w == "PIC") {
                self.advance();
                let ty = self.parse_pic();
                self.expect_period();
                fields.push(RecField {
                    name: fname,
                    kind: FieldKind::Pic(ty),
                    redefines,
                });
            } else {
                self.expect_period();
                let sub_type = format!("{}{}", parent_type, to_pascal_case(&fname_cobol));
                let sub_fields = self.parse_group_fields(program, level, &sub_type);
                program.records.push(RecordType {
                    type_name: sub_type.clone(),
                    var_name: format!("__inner_{}", fname),
                    fields: sub_fields,
                });
                fields.push(RecField {
                    name: fname,
                    kind: FieldKind::Sub(sub_type),
                    redefines,
                });
            }
        }
        fields
    }

    fn parse_pic(&mut self) -> PicType {
        match self.peek().clone() {
            Token::Word(ref w) if w == "X" => {
                self.advance();
                let n = if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let n = self.expect_intlit() as u32;
                    self.expect_rparen();
                    n
                } else {
                    1
                };
                PicType::Str(n)
            }
            Token::IntLit(_) => {
                self.advance();
                let n = if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let n = self.expect_intlit() as u32;
                    self.expect_rparen();
                    n
                } else {
                    1
                };
                if let Token::Word(w) = self.peek() {
                    if w.starts_with('V') && w.len() > 1 {
                        let frac_digits = w[1..].chars().filter(|c| c.is_ascii_digit()).count();
                        if frac_digits > 0 && w[1..].chars().all(|c| c == '9') {
                            self.advance();
                            return PicType::UDec(n, frac_digits as u32);
                        }
                    }
                }
                PicType::UInt(n)
            }
            other => panic!("cobol2wei: PIC expected X or digit pattern, got {:?}", other),
        }
    }

    fn parse_literal(&mut self) -> Literal {
        match self.advance() {
            Token::IntLit(n) => Literal::Int(n),
            Token::StringLit(s) => Literal::Str(s),
            other => panic!("cobol2wei: expected literal value, got {:?}", other),
        }
    }

    fn parse_procedure_division(&mut self, program: &mut Program) {
        let mut current_paragraph: Option<String> = None;
        let mut current_body: Vec<Stmt> = Vec::new();
        let mut using_main = true;

        loop {
            if self.at_eof() {
                break;
            }
            if matches!(self.peek(), Token::Word(w) if w == "IDENTIFICATION") {
                break;
            }
            if let Token::Word(w) = self.peek() {
                if !is_verb(w) && matches!(self.peek_n(1), Token::Period) {
                    if using_main {
                        program.main_code = std::mem::take(&mut current_body);
                        using_main = false;
                    } else {
                        program.paragraphs.push(Paragraph {
                            name: current_paragraph.take().unwrap(),
                            body: std::mem::take(&mut current_body),
                        });
                    }
                    let name = self.advance();
                    self.expect_period();
                    let name_str = match name {
                        Token::Word(s) => to_wei_ident(&s),
                        _ => unreachable!(),
                    };
                    current_paragraph = Some(name_str);
                    continue;
                }
            }
            let s = self.parse_statement(false);
            current_body.push(s);
        }

        if using_main {
            program.main_code = current_body;
        } else if let Some(name) = current_paragraph {
            program.paragraphs.push(Paragraph {
                name,
                body: current_body,
            });
        }
    }

    fn parse_statement(&mut self, in_block: bool) -> Stmt {
        match self.peek().clone() {
            Token::Word(ref w) if w == "DISPLAY" => {
                self.advance();
                let e = self.parse_expr();
                self.end_stmt(in_block);
                Stmt::Display(e)
            }
            Token::Word(ref w) if w == "MOVE" => {
                self.advance();
                let value = self.parse_expr();
                self.expect_word("TO");
                let target = self.expect_word_any();
                self.end_stmt(in_block);
                Stmt::Move {
                    value,
                    target: to_wei_ident(&target),
                }
            }
            Token::Word(ref w) if w == "ADD" => {
                self.advance();
                let value = self.parse_expr();
                self.expect_word("TO");
                let target = self.expect_word_any();
                self.end_stmt(in_block);
                Stmt::Add {
                    value,
                    target: to_wei_ident(&target),
                }
            }
            Token::Word(ref w) if w == "SUBTRACT" => {
                self.advance();
                let value = self.parse_expr();
                self.expect_word("FROM");
                let target = self.expect_word_any();
                self.end_stmt(in_block);
                Stmt::Subtract {
                    value,
                    target: to_wei_ident(&target),
                }
            }
            Token::Word(ref w) if w == "MULTIPLY" => {
                self.advance();
                let a = self.parse_expr();
                self.expect_word("BY");
                let b = self.parse_expr();
                let (target, expr) = if matches!(self.peek(), Token::Word(w) if w == "GIVING") {
                    self.advance();
                    let c = self.expect_word_any();
                    (to_wei_ident(&c), bin(ArithOp::Mul, a, b))
                } else {
                    let tgt = match &b {
                        Expr::Ident(n) => n.clone(),
                        _ => panic!("cobol2wei: MULTIPLY BY target must be an identifier"),
                    };
                    (tgt, bin(ArithOp::Mul, a, b))
                };
                self.end_stmt(in_block);
                Stmt::Compute { target, expr }
            }
            Token::Word(ref w) if w == "DIVIDE" => {
                self.advance();
                let a = self.parse_expr();
                let by_into = self.expect_word_any();
                let b = self.parse_expr();
                let (target, expr) = match by_into.as_str() {
                    "INTO" => {
                        if matches!(self.peek(), Token::Word(w) if w == "GIVING") {
                            self.advance();
                            let c = self.expect_word_any();
                            (to_wei_ident(&c), bin(ArithOp::Div, b, a))
                        } else {
                            let tgt = match &b {
                                Expr::Ident(n) => n.clone(),
                                _ => panic!("cobol2wei: DIVIDE INTO target must be an identifier"),
                            };
                            (tgt, bin(ArithOp::Div, b, a))
                        }
                    }
                    "BY" => {
                        self.expect_word("GIVING");
                        let c = self.expect_word_any();
                        (to_wei_ident(&c), bin(ArithOp::Div, a, b))
                    }
                    other => panic!("cobol2wei: expected INTO or BY after DIVIDE, got {}", other),
                };
                self.end_stmt(in_block);
                Stmt::Compute { target, expr }
            }
            Token::Word(ref w) if w == "COMPUTE" => {
                self.advance();
                let target = self.expect_word_any();
                if !matches!(self.advance(), Token::Eq) {
                    panic!("cobol2wei: expected `=` after COMPUTE target");
                }
                let expr = self.parse_arith_expr();
                self.end_stmt(in_block);
                Stmt::Compute {
                    target: to_wei_ident(&target),
                    expr,
                }
            }
            Token::Word(ref w) if w == "PERFORM" => {
                self.advance();
                if matches!(self.peek(), Token::Word(w) if w == "UNTIL") {
                    self.advance();
                    let cond = self.parse_condition();
                    let mut body = Vec::new();
                    while !matches!(self.peek(), Token::Word(w) if w == "END-PERFORM") {
                        if self.at_eof() {
                            panic!("cobol2wei: unterminated PERFORM UNTIL (missing END-PERFORM)");
                        }
                        body.push(self.parse_statement(true));
                    }
                    self.expect_word("END-PERFORM");
                    self.end_stmt(in_block);
                    Stmt::PerformUntil { cond, body }
                } else if matches!(self.peek(), Token::Word(w) if w == "VARYING") {
                    self.parse_varying(None, in_block)
                } else {
                    let para = self.expect_word_any();
                    if matches!(self.peek(), Token::Word(w) if w == "VARYING") {
                        self.parse_varying(Some(to_wei_ident(&para)), in_block)
                    } else {
                        self.end_stmt(in_block);
                        Stmt::Perform {
                            para: to_wei_ident(&para),
                        }
                    }
                }
            }
            Token::Word(ref w) if w == "OPEN" => {
                self.advance();
                let mode_raw = self.expect_word_any();
                let mode = match mode_raw.as_str() {
                    "INPUT" => "input",
                    "OUTPUT" => "output",
                    "I-O" => "i_o",
                    "EXTEND" => "extend",
                    other => panic!("cobol2wei: unknown OPEN mode {}", other),
                }
                .to_string();
                let file = self.expect_word_any();
                self.end_stmt(in_block);
                Stmt::Open {
                    mode,
                    file: to_wei_ident(&file),
                }
            }
            Token::Word(ref w) if w == "CLOSE" => {
                self.advance();
                let file = self.expect_word_any();
                self.end_stmt(in_block);
                Stmt::Close {
                    file: to_wei_ident(&file),
                }
            }
            Token::Word(ref w) if w == "WRITE" => {
                self.advance();
                let rec_cobol = self.expect_word_any();
                self.end_stmt(in_block);
                Stmt::Write {
                    rec: to_wei_ident(&rec_cobol),
                }
            }
            Token::Word(ref w) if w == "READ" => {
                self.advance();
                let file = self.expect_word_any();
                let at_end_target = self.parse_at_end_clause();
                self.end_stmt(in_block);
                Stmt::Read {
                    file: to_wei_ident(&file),
                    at_end_target,
                }
            }
            Token::Word(ref w) if w == "IF" => {
                self.advance();
                let cond = self.parse_condition();
                if matches!(self.peek(), Token::Word(w) if w == "THEN") {
                    self.advance();
                }
                let mut then_body = Vec::new();
                loop {
                    match self.peek() {
                        Token::Word(w) if w == "ELSE" || w == "END-IF" => break,
                        Token::Eof => panic!("cobol2wei: unterminated IF (missing END-IF)"),
                        _ => then_body.push(self.parse_statement(true)),
                    }
                }
                let mut else_body = Vec::new();
                if matches!(self.peek(), Token::Word(w) if w == "ELSE") {
                    self.advance();
                    loop {
                        match self.peek() {
                            Token::Word(w) if w == "END-IF" => break,
                            Token::Eof => panic!("cobol2wei: unterminated IF (missing END-IF)"),
                            _ => else_body.push(self.parse_statement(true)),
                        }
                    }
                }
                self.expect_word("END-IF");
                self.end_stmt(in_block);
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                }
            }
            Token::Word(ref w) if w == "EVALUATE" => {
                self.advance();
                let expr = self.parse_expr();
                let mut arms: Vec<EvalArm> = Vec::new();
                loop {
                    match self.peek() {
                        Token::Word(w) if w == "WHEN" => {
                            self.advance();
                            let pattern = if matches!(self.peek(), Token::Word(w) if w == "OTHER")
                            {
                                self.advance();
                                EvalPattern::Other
                            } else if let Token::StringLit(_) = self.peek() {
                                if let Token::StringLit(s) = self.advance() {
                                    EvalPattern::StrLit(s)
                                } else {
                                    unreachable!()
                                }
                            } else {
                                let lo = self.expect_intlit();
                                if matches!(self.peek(), Token::Word(w) if w == "THRU" || w == "THROUGH")
                                {
                                    self.advance();
                                    let hi = self.expect_intlit();
                                    EvalPattern::Thru(lo, hi)
                                } else {
                                    EvalPattern::Lit(lo)
                                }
                            };
                            let mut body = Vec::new();
                            loop {
                                match self.peek() {
                                    Token::Word(w) if w == "WHEN" || w == "END-EVALUATE" => break,
                                    Token::Eof => panic!(
                                        "cobol2wei: unterminated EVALUATE (missing END-EVALUATE)"
                                    ),
                                    _ => body.push(self.parse_statement(true)),
                                }
                            }
                            arms.push(EvalArm { pattern, body });
                        }
                        Token::Word(w) if w == "END-EVALUATE" => break,
                        Token::Eof => {
                            panic!("cobol2wei: unterminated EVALUATE (missing END-EVALUATE)")
                        }
                        other => {
                            panic!("cobol2wei: expected WHEN or END-EVALUATE, got {:?}", other)
                        }
                    }
                }
                self.expect_word("END-EVALUATE");
                self.end_stmt(in_block);
                Stmt::Evaluate { expr, arms }
            }
            Token::Word(ref w) if w == "INSPECT" => {
                self.advance();
                let subject = self.expect_word_any();
                self.expect_word("TALLYING");
                let counter = self.expect_word_any();
                self.expect_word("FOR");
                self.expect_word("ALL");
                let needle = match self.advance() {
                    Token::StringLit(s) => s,
                    other => panic!(
                        "cobol2wei: INSPECT FOR ALL expected string literal, got {:?}",
                        other
                    ),
                };
                self.end_stmt(in_block);
                Stmt::InspectTally {
                    subject: to_wei_ident(&subject),
                    counter: to_wei_ident(&counter),
                    needle,
                }
            }
            Token::Word(ref w) if w == "GO" => {
                self.advance();
                self.expect_word("TO");
                let target = self.expect_word_any();
                self.end_stmt(in_block);
                Stmt::Goto {
                    label: to_wei_ident(&target),
                }
            }
            Token::Word(ref w) if w == "STOP" => {
                self.advance();
                self.expect_word("RUN");
                self.end_stmt(in_block);
                Stmt::StopRun
            }
            Token::Word(ref w) if w == "EXIT" => {
                self.advance();
                self.expect_word("PROGRAM");
                self.end_stmt(in_block);
                Stmt::ExitProgram
            }
            Token::Word(ref w) if w == "CALL" => {
                self.advance();
                let name_cobol = match self.advance() {
                    Token::StringLit(s) => s,
                    Token::Word(w) => w,
                    other => panic!("cobol2wei: CALL expected program name, got {:?}", other),
                };
                let mut args = Vec::new();
                if matches!(self.peek(), Token::Word(w) if w == "USING") {
                    self.advance();
                    loop {
                        args.push(self.parse_expr());
                        if matches!(self.peek(), Token::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.end_stmt(in_block);
                Stmt::CallSub {
                    name: to_wei_ident(&name_cobol),
                    args,
                }
            }
            other => panic!("cobol2wei: unsupported statement starting with {:?}", other),
        }
    }

    fn parse_varying(&mut self, para: Option<String>, in_block: bool) -> Stmt {
        self.expect_word("VARYING");
        let var_cobol = self.expect_word_any();
        let var = to_wei_ident(&var_cobol);
        self.expect_word("FROM");
        let start = self.parse_expr();
        self.expect_word("BY");
        let step = self.expect_intlit();
        if step != 1 {
            panic!("cobol2wei: PERFORM VARYING BY != 1 not supported");
        }
        self.expect_word("UNTIL");
        let cond = self.parse_condition();
        let (end, end_inclusive) = match cond {
            Cond::Cmp {
                op,
                left: Expr::Ident(n),
                right,
            } if n == var => match op {
                CmpOp::Gt => (right, true),
                CmpOp::Ge | CmpOp::Eq => (right, false),
                _ => panic!(
                    "cobol2wei: PERFORM VARYING UNTIL: only var > N, var >= N, var = N supported"
                ),
            },
            _ => panic!("cobol2wei: PERFORM VARYING UNTIL: condition must compare loop var"),
        };
        let body = if let Some(para_name) = para {
            self.end_stmt(in_block);
            vec![Stmt::Perform { para: para_name }]
        } else {
            let mut body = Vec::new();
            while !matches!(self.peek(), Token::Word(w) if w == "END-PERFORM") {
                if self.at_eof() {
                    panic!("cobol2wei: unterminated PERFORM VARYING (missing END-PERFORM)");
                }
                body.push(self.parse_statement(true));
            }
            self.expect_word("END-PERFORM");
            self.end_stmt(in_block);
            body
        };
        Stmt::ForRange {
            var,
            start,
            end,
            end_inclusive,
            body,
        }
    }

    fn parse_at_end_clause(&mut self) -> Option<String> {
        if !matches!(self.peek(), Token::Word(w) if w == "AT") {
            return None;
        }
        self.advance();
        self.expect_word("END");
        self.expect_word("MOVE");
        let _val = self.parse_literal();
        self.expect_word("TO");
        let target = self.expect_word_any();
        self.expect_word("END-READ");
        Some(to_wei_ident(&target))
    }

    fn parse_condition(&mut self) -> Cond {
        let mut left = self.parse_and_cond();
        while matches!(self.peek(), Token::Word(w) if w == "OR") {
            self.advance();
            let right = self.parse_and_cond();
            left = Cond::Or(Box::new(left), Box::new(right));
        }
        left
    }

    fn parse_and_cond(&mut self) -> Cond {
        let mut left = self.parse_atom_cond();
        while matches!(self.peek(), Token::Word(w) if w == "AND") {
            self.advance();
            let right = self.parse_atom_cond();
            left = Cond::And(Box::new(left), Box::new(right));
        }
        left
    }

    fn parse_atom_cond(&mut self) -> Cond {
        let left = self.parse_expr();
        let mut negated = false;
        if matches!(self.peek(), Token::Word(w) if w == "NOT") {
            self.advance();
            negated = true;
        }
        let op = match self.peek() {
            Token::Eq => Some(CmpOp::Eq),
            Token::Lt => Some(CmpOp::Lt),
            Token::Gt => Some(CmpOp::Gt),
            Token::LtEq => Some(CmpOp::Le),
            Token::GtEq => Some(CmpOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.parse_expr();
            let final_op = if negated {
                match op {
                    CmpOp::Eq => CmpOp::Ne,
                    CmpOp::Ne => CmpOp::Eq,
                    CmpOp::Lt => CmpOp::Ge,
                    CmpOp::Le => CmpOp::Gt,
                    CmpOp::Gt => CmpOp::Le,
                    CmpOp::Ge => CmpOp::Lt,
                }
            } else {
                op
            };
            return Cond::Cmp {
                op: final_op,
                left,
                right,
            };
        }
        if negated {
            panic!("cobol2wei: NOT without comparison operator");
        }
        match left {
            Expr::Ident(n) => Cond::Bare(n),
            other => panic!(
                "cobol2wei: expected comparison or 88-level name, got {:?}",
                other
            ),
        }
    }

    fn parse_expr(&mut self) -> Expr {
        match self.advance() {
            Token::IntLit(n) => Expr::Int(n),
            Token::StringLit(s) => Expr::Str(s),
            Token::Word(w) => Expr::Ident(to_wei_ident(&w)),
            other => panic!("cobol2wei: expected expression, got {:?}", other),
        }
    }

    fn parse_arith_expr(&mut self) -> Expr {
        self.parse_arith_add()
    }

    fn parse_arith_add(&mut self) -> Expr {
        let mut left = self.parse_arith_mul();
        loop {
            let op = match self.peek() {
                Token::Plus => ArithOp::Add,
                Token::Minus => ArithOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_arith_mul();
            left = bin(op, left, right);
        }
        left
    }

    fn parse_arith_mul(&mut self) -> Expr {
        let mut left = self.parse_arith_atom();
        loop {
            let op = match self.peek() {
                Token::Star => ArithOp::Mul,
                Token::Slash => ArithOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_arith_atom();
            left = bin(op, left, right);
        }
        left
    }

    fn parse_arith_atom(&mut self) -> Expr {
        match self.peek().clone() {
            Token::LParen => {
                self.advance();
                let e = self.parse_arith_expr();
                self.expect_rparen();
                e
            }
            _ => self.parse_expr(),
        }
    }

    fn end_stmt(&mut self, in_block: bool) {
        if in_block {
            if matches!(self.peek(), Token::Period) {
                self.advance();
            }
        } else {
            self.expect_period();
        }
    }

    fn expect_word(&mut self, expected: &str) {
        match self.advance() {
            Token::Word(w) if w == expected => {}
            other => panic!("cobol2wei: expected `{}`, got {:?}", expected, other),
        }
    }

    fn expect_word_any(&mut self) -> String {
        match self.advance() {
            Token::Word(w) => w,
            other => panic!("cobol2wei: expected an identifier word, got {:?}", other),
        }
    }

    fn expect_intlit(&mut self) -> i64 {
        match self.advance() {
            Token::IntLit(n) => n,
            other => panic!("cobol2wei: expected integer, got {:?}", other),
        }
    }

    fn expect_period(&mut self) {
        match self.advance() {
            Token::Period => {}
            other => panic!("cobol2wei: expected `.`, got {:?}", other),
        }
    }

    fn expect_rparen(&mut self) {
        match self.advance() {
            Token::RParen => {}
            other => panic!("cobol2wei: expected `)`, got {:?}", other),
        }
    }
}

fn is_verb(w: &str) -> bool {
    matches!(
        w,
        "DISPLAY"
            | "MOVE"
            | "ADD"
            | "SUBTRACT"
            | "MULTIPLY"
            | "DIVIDE"
            | "PERFORM"
            | "STOP"
            | "OPEN"
            | "CLOSE"
            | "READ"
            | "WRITE"
            | "REWRITE"
            | "GO"
            | "INSPECT"
            | "IF"
            | "EVALUATE"
            | "COMPUTE"
            | "SET"
            | "WHEN"
            | "CALL"
            | "EXIT"
            | "END-EVALUATE"
            | "END-IF"
            | "END-PERFORM"
            | "END-READ"
    )
}

#[derive(Default)]
struct Resolved {
    field_owner: HashMap<String, String>,
    fd_record: HashMap<String, String>,
    record_to_file: HashMap<String, String>,
    eof_flag_for_file: HashMap<String, String>,
    eighty_eight_parent: HashMap<String, String>,
    file_for_eighty_eight: HashMap<String, String>,
    suppressed_flags: HashSet<String>,
}

fn resolve(program: &Program) -> Resolved {
    let mut r = Resolved::default();
    for fd in &program.fds {
        r.fd_record.insert(fd.file_name.clone(), fd.record_var.clone());
        r.record_to_file
            .insert(fd.record_var.clone(), fd.file_name.clone());
    }
    for rec in &program.records {
        if rec.var_name.starts_with("__inner_") {
            continue;
        }
        for f in &rec.fields {
            r.field_owner.insert(f.name.clone(), rec.var_name.clone());
        }
    }
    for s in &program.ws_scalars {
        for (cn, _) in &s.eighty_eights {
            r.eighty_eight_parent.insert(cn.clone(), s.name.clone());
        }
    }
    walk(&program.main_code, &mut r);
    for p in &program.paragraphs {
        walk(&p.body, &mut r);
    }
    let mut flag_to_file: HashMap<String, String> = HashMap::new();
    for (file, flag) in &r.eof_flag_for_file {
        flag_to_file.insert(flag.clone(), file.clone());
    }
    for (eight, parent) in &r.eighty_eight_parent {
        if let Some(file) = flag_to_file.get(parent) {
            r.file_for_eighty_eight.insert(eight.clone(), file.clone());
            r.suppressed_flags.insert(parent.clone());
        }
    }
    r
}

fn walk(stmts: &[Stmt], r: &mut Resolved) {
    for s in stmts {
        match s {
            Stmt::Read {
                file,
                at_end_target: Some(t),
            } => {
                r.eof_flag_for_file
                    .entry(file.clone())
                    .or_insert_with(|| t.clone());
            }
            Stmt::PerformUntil { body, .. } => walk(body, r),
            _ => {}
        }
    }
}

fn pic_to_str(ty: &PicType) -> String {
    match ty {
        PicType::Str(n) => format!("str({})", n),
        PicType::UInt(n) => format!("uint({})", n),
        PicType::UDec(n, m) => format!("udec({},{})", n, m),
    }
}

fn qualified(name: &str, r: &Resolved) -> String {
    if let Some(rec) = r.field_owner.get(name) {
        format!("{}.{}", rec, name)
    } else {
        name.to_string()
    }
}

fn expr_wei(e: &Expr, r: &Resolved) -> String {
    match e {
        Expr::Ident(n) => qualified(n, r),
        Expr::Int(v) => v.to_string(),
        Expr::Str(s) => format!("\"{}\"", s),
        Expr::Bin { op, left, right } => {
            let s = arith_sym(*op);
            format!("({} {} {})", expr_wei(left, r), s, expr_wei(right, r))
        }
    }
}

fn expr_cobol(e: &Expr) -> String {
    match e {
        Expr::Ident(n) => to_cobol_name(n),
        Expr::Int(v) => v.to_string(),
        Expr::Str(s) => format!("\"{}\"", s),
        Expr::Bin { op, left, right } => {
            let s = arith_sym(*op);
            format!("({} {} {})", expr_cobol(left), s, expr_cobol(right))
        }
    }
}

fn arith_sym(op: ArithOp) -> &'static str {
    match op {
        ArithOp::Add => "+",
        ArithOp::Sub => "-",
        ArithOp::Mul => "*",
        ArithOp::Div => "/",
    }
}

fn bin(op: ArithOp, left: Expr, right: Expr) -> Expr {
    Expr::Bin {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn cmp_wei(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "==",
        CmpOp::Ne => "!=",
        CmpOp::Lt => "<",
        CmpOp::Le => "<=",
        CmpOp::Gt => ">",
        CmpOp::Ge => ">=",
    }
}

fn cmp_cobol(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "=",
        CmpOp::Ne => "NOT =",
        CmpOp::Lt => "<",
        CmpOp::Le => "<=",
        CmpOp::Gt => ">",
        CmpOp::Ge => ">=",
    }
}

fn cond_wei(c: &Cond, r: &Resolved) -> String {
    match c {
        Cond::Cmp { op, left, right } => format!(
            "{} {} {}",
            expr_wei(left, r),
            cmp_wei(*op),
            expr_wei(right, r)
        ),
        Cond::Bare(n) => {
            if let Some(file) = r.file_for_eighty_eight.get(n) {
                format!("at_end({})", file)
            } else if r.eighty_eight_parent.contains_key(n) {
                // General 88-level reference — wei evaluates `n` to (parent == value).
                n.clone()
            } else {
                panic!(
                    "cobol2wei: unknown bare condition `{}`",
                    to_cobol_name(n)
                );
            }
        }
        Cond::And(l, rr) => format!("({}) && ({})", cond_wei(l, r), cond_wei(rr, r)),
        Cond::Or(l, rr) => format!("({}) || ({})", cond_wei(l, r), cond_wei(rr, r)),
    }
}

fn cond_cobol(c: &Cond) -> String {
    match c {
        Cond::Cmp { op, left, right } => format!(
            "{} {} {}",
            expr_cobol(left),
            cmp_cobol(*op),
            expr_cobol(right)
        ),
        Cond::Bare(n) => to_cobol_name(n),
        Cond::And(l, r) => format!("{} AND {}", cond_cobol(l), cond_cobol(r)),
        Cond::Or(l, r) => format!("{} OR {}", cond_cobol(l), cond_cobol(r)),
    }
}

fn emit(program: &Program) -> String {
    let r = resolve(program);
    let mut out = String::new();

    for rec in &program.records {
        if rec.var_name.starts_with("__inner_") {
            out.push_str(&format!("// (synthetic sub-record)\n"));
        } else {
            out.push_str(&format!(
                "// COBOL: 01 {}.\n",
                to_cobol_name(&rec.var_name)
            ));
        }
        out.push_str(&format!("type {}:\n", rec.type_name));
        for f in &rec.fields {
            let ty_str = match &f.kind {
                FieldKind::Pic(p) => pic_to_str(p),
                FieldKind::Sub(name) => name.clone(),
            };
            let redef = if let Some(target) = &f.redefines {
                format!(" redefines {}", target)
            } else {
                String::new()
            };
            out.push_str(&format!("    {}{} {}\n", f.name, redef, ty_str));
        }
        out.push('\n');
    }

    for sub in &program.subs {
        out.push_str(&format!(
            "// COBOL: PROGRAM-ID. {}.\n",
            to_cobol_name(&sub.name)
        ));
        let plist: Vec<String> = sub
            .params
            .iter()
            .map(|(pn, pt)| format!("{} {}", pn, pic_to_str(pt)))
            .collect();
        out.push_str(&format!("sub {}({}):\n", sub.name, plist.join(", ")));
        if sub.body.is_empty() {
            out.push_str("    // empty\n");
            out.push_str("    print(0)\n");
        } else {
            for s in &sub.body {
                emit_stmt(&mut out, s, 1, &r);
            }
        }
        out.push('\n');
    }

    for f in &program.files {
        let key_suffix = if let Some(k) = &f.key {
            format!(" RECORD KEY IS {}", to_cobol_name(k))
        } else {
            String::new()
        };
        out.push_str(&format!(
            "// COBOL: SELECT {} ASSIGN TO \"{}\" ORGANIZATION IS {}{}.\n",
            to_cobol_name(&f.name),
            f.path,
            f.org.to_uppercase(),
            key_suffix
        ));
        let wei_key_suffix = if let Some(k) = &f.key {
            format!(" key {}", k)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "file {} = \"{}\" {}{}\n",
            f.name, f.path, f.org, wei_key_suffix
        ));
    }
    if !program.files.is_empty() {
        out.push('\n');
    }

    for rec in &program.records {
        if rec.var_name.starts_with("__inner_") {
            continue;
        }
        out.push_str(&format!("let {} {}\n", rec.var_name, rec.type_name));
    }

    for s in &program.ws_scalars {
        if r.suppressed_flags.contains(&s.name) {
            continue;
        }
        let ty_str = pic_to_str(&s.ty);
        out.push_str(&format!("// COBOL: 01 {} PIC ...\n", to_cobol_name(&s.name)));
        match &s.value {
            Some(Literal::Int(v)) => {
                out.push_str(&format!("let {} {} = {}\n", s.name, ty_str, v));
            }
            Some(Literal::Str(sv)) => {
                out.push_str(&format!("let {} {} = \"{}\"\n", s.name, ty_str, sv));
            }
            None => {
                out.push_str(&format!("let {} {}\n", s.name, ty_str));
            }
        }
        for (n88, v) in &s.eighty_eights {
            let v_str = match v {
                Literal::Int(n) => n.to_string(),
                Literal::Str(sv) => format!("\"{}\"", sv),
            };
            out.push_str(&format!(
                "    // COBOL: 88 {} VALUE {}.\n",
                to_cobol_name(n88),
                v_str
            ));
            out.push_str(&format!("    is {} = {}\n", n88, v_str));
        }
    }
    if !program.ws_scalars.iter().all(|s| r.suppressed_flags.contains(&s.name))
        || !program.records.is_empty()
    {
        out.push('\n');
    }

    for p in &program.paragraphs {
        out.push_str(&format!("// COBOL: {}.\n", to_cobol_name(&p.name)));
        out.push_str(&format!("par {}:\n", p.name));
        for s in &p.body {
            emit_stmt(&mut out, s, 1, &r);
        }
        out.push('\n');
    }

    if !program.main_code.is_empty() {
        out.push_str("// COBOL: (procedure body / main)\n");
        for s in &program.main_code {
            emit_stmt(&mut out, s, 0, &r);
        }
    } else if !program.paragraphs.is_empty() {
        let first = &program.paragraphs[0];
        out.push_str(&format!("{}()\n", first.name));
    }

    out
}

fn emit_stmt(out: &mut String, s: &Stmt, indent: usize, r: &Resolved) {
    let pad = "    ".repeat(indent);
    match s {
        Stmt::Display(e) => {
            out.push_str(&format!("{}// COBOL: DISPLAY {}.\n", pad, expr_cobol(e)));
            out.push_str(&format!("{}print({})\n", pad, expr_wei(e, r)));
        }
        Stmt::Move { value, target } => {
            out.push_str(&format!(
                "{}// COBOL: MOVE {} TO {}.\n",
                pad,
                expr_cobol(value),
                to_cobol_name(target)
            ));
            out.push_str(&format!(
                "{}{} = {}\n",
                pad,
                qualified(target, r),
                expr_wei(value, r)
            ));
        }
        Stmt::Add { value, target } => {
            out.push_str(&format!(
                "{}// COBOL: ADD {} TO {}.\n",
                pad,
                expr_cobol(value),
                to_cobol_name(target)
            ));
            let tgt = qualified(target, r);
            out.push_str(&format!(
                "{}{} = {} + {}\n",
                pad,
                tgt,
                tgt,
                expr_wei(value, r)
            ));
        }
        Stmt::Compute { target, expr } => {
            out.push_str(&format!(
                "{}// COBOL: COMPUTE {} = {}.\n",
                pad,
                to_cobol_name(target),
                expr_cobol(expr)
            ));
            out.push_str(&format!(
                "{}{} = {}\n",
                pad,
                qualified(target, r),
                expr_wei(expr, r)
            ));
        }
        Stmt::Subtract { value, target } => {
            out.push_str(&format!(
                "{}// COBOL: SUBTRACT {} FROM {}.\n",
                pad,
                expr_cobol(value),
                to_cobol_name(target)
            ));
            let tgt = qualified(target, r);
            out.push_str(&format!(
                "{}{} = {} - {}\n",
                pad,
                tgt,
                tgt,
                expr_wei(value, r)
            ));
        }
        Stmt::Perform { para } => {
            out.push_str(&format!(
                "{}// COBOL: PERFORM {}.\n",
                pad,
                to_cobol_name(para)
            ));
            out.push_str(&format!("{}{}()\n", pad, para));
        }
        Stmt::ForRange {
            var,
            start,
            end,
            end_inclusive,
            body,
        } => {
            let end_str = if *end_inclusive {
                format!("({} + 1)", expr_wei(end, r))
            } else {
                expr_wei(end, r)
            };
            out.push_str(&format!(
                "{}// COBOL: PERFORM VARYING {} ...\n",
                pad,
                to_cobol_name(var)
            ));
            out.push_str(&format!(
                "{}for {} in {}..{}:\n",
                pad,
                var,
                expr_wei(start, r),
                end_str
            ));
            for s in body {
                emit_stmt(out, s, indent + 1, r);
            }
        }
        Stmt::PerformUntil { cond, body } => {
            out.push_str(&format!(
                "{}// COBOL: PERFORM UNTIL {} ...\n",
                pad,
                cond_cobol(cond)
            ));
            out.push_str(&format!("{}while !({}):\n", pad, cond_wei(cond, r)));
            for s in body {
                emit_stmt(out, s, indent + 1, r);
            }
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            out.push_str(&format!(
                "{}// COBOL: IF {} ...\n",
                pad,
                cond_cobol(cond)
            ));
            out.push_str(&format!("{}if {}:\n", pad, cond_wei(cond, r)));
            for s in then_body {
                emit_stmt(out, s, indent + 1, r);
            }
            if !else_body.is_empty() {
                out.push_str(&format!("{}else:\n", pad));
                for s in else_body {
                    emit_stmt(out, s, indent + 1, r);
                }
            }
        }
        Stmt::Open { mode, file } => {
            out.push_str(&format!(
                "{}// COBOL: OPEN {} {}.\n",
                pad,
                mode.to_uppercase().replace('_', "-"),
                to_cobol_name(file)
            ));
            out.push_str(&format!("{}open({}, {})\n", pad, file, mode));
        }
        Stmt::Write { rec } => {
            let file = r.record_to_file.get(rec).unwrap_or_else(|| {
                panic!("cobol2wei: WRITE `{}` has no FD binding", to_cobol_name(rec))
            });
            out.push_str(&format!("{}// COBOL: WRITE {}.\n", pad, to_cobol_name(rec)));
            out.push_str(&format!("{}write({}, {})\n", pad, file, rec));
        }
        Stmt::Close { file } => {
            out.push_str(&format!("{}// COBOL: CLOSE {}.\n", pad, to_cobol_name(file)));
            out.push_str(&format!("{}close({})\n", pad, file));
        }
        Stmt::Read {
            file,
            at_end_target,
        } => {
            let suffix = at_end_target
                .as_ref()
                .map(|t| format!(" AT END MOVE ... TO {} END-READ", to_cobol_name(t)))
                .unwrap_or_default();
            out.push_str(&format!(
                "{}// COBOL: READ {}{}.\n",
                pad,
                to_cobol_name(file),
                suffix
            ));
            let rec = r.fd_record.get(file).unwrap_or_else(|| {
                panic!("cobol2wei: READ {} has no FD record binding", file)
            });
            out.push_str(&format!("{}read({}, {})\n", pad, file, rec));
        }
        Stmt::Evaluate { expr, arms } => {
            out.push_str(&format!(
                "{}// COBOL: EVALUATE {} ...\n",
                pad,
                expr_cobol(expr)
            ));
            out.push_str(&format!("{}match {}:\n", pad, expr_wei(expr, r)));
            let arm_pad = "    ".repeat(indent + 1);
            for arm in arms {
                match &arm.pattern {
                    EvalPattern::Lit(v) => {
                        out.push_str(&format!("{}// COBOL: WHEN {}\n", arm_pad, v));
                        out.push_str(&format!("{}{} =>\n", arm_pad, v));
                    }
                    EvalPattern::Thru(lo, hi) => {
                        out.push_str(&format!("{}// COBOL: WHEN {} THRU {}\n", arm_pad, lo, hi));
                        out.push_str(&format!("{}{}..={} =>\n", arm_pad, lo, hi));
                    }
                    EvalPattern::StrLit(s) => {
                        out.push_str(&format!("{}// COBOL: WHEN \"{}\"\n", arm_pad, s));
                        out.push_str(&format!("{}\"{}\" =>\n", arm_pad, s));
                    }
                    EvalPattern::Other => {
                        out.push_str(&format!("{}// COBOL: WHEN OTHER\n", arm_pad));
                        out.push_str(&format!("{}_ =>\n", arm_pad));
                    }
                }
                if arm.body.is_empty() {
                    out.push_str(&format!("{}    print(0)\n", arm_pad));
                } else {
                    for s in &arm.body {
                        emit_stmt(out, s, indent + 2, r);
                    }
                }
            }
        }
        Stmt::CallSub { name, args } => {
            let arg_str: Vec<String> = args.iter().map(|a| expr_wei(a, r)).collect();
            let arg_str_cobol: Vec<String> = args.iter().map(expr_cobol).collect();
            out.push_str(&format!(
                "{}// COBOL: CALL \"{}\" USING {}.\n",
                pad,
                to_cobol_name(name),
                arg_str_cobol.join(", ")
            ));
            out.push_str(&format!("{}{}({})\n", pad, name, arg_str.join(", ")));
        }
        Stmt::InspectTally {
            subject,
            counter,
            needle,
        } => {
            out.push_str(&format!(
                "{}// COBOL: INSPECT {} TALLYING {} FOR ALL \"{}\".\n",
                pad,
                to_cobol_name(subject),
                to_cobol_name(counter),
                needle
            ));
            out.push_str(&format!(
                "{}{} = count_chars({}, \"{}\")\n",
                pad,
                qualified(counter, r),
                qualified(subject, r),
                needle
            ));
        }
        Stmt::Goto { label } => {
            out.push_str(&format!("{}// COBOL: GO TO {}.\n", pad, to_cobol_name(label)));
            out.push_str(&format!("{}goto {}\n", pad, label));
        }
        Stmt::ExitProgram => {
            out.push_str(&format!("{}// COBOL: EXIT PROGRAM.\n", pad));
        }
        Stmt::StopRun => {
            out.push_str(&format!("{}// COBOL: STOP RUN.\n", pad));
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: cobol2wei <input.cbl> [-o output.wei]");
        process::exit(2);
    }
    let input_path = &args[1];
    let mut output_path = String::from("out.wei");
    let mut i = 2;
    while i < args.len() {
        if args[i] == "-o" && i + 1 < args.len() {
            output_path = args[i + 1].clone();
            i += 2;
        } else {
            i += 1;
        }
    }

    let source = fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("cobol2wei: cannot read {}: {}", input_path, e);
        process::exit(1);
    });

    let tokens = lex(&source);
    let mut p = Parser {
        tokens: &tokens,
        pos: 0,
    };
    let program = p.parse_program();
    let out = emit(&program);

    if let Err(e) = fs::write(&output_path, out) {
        eprintln!("cobol2wei: cannot write {}: {}", output_path, e);
        process::exit(1);
    }
}
