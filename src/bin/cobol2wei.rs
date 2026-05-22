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
struct WSItem {
    name: String,
    ty: PicType,
    value: Option<Literal>,
}

#[derive(Debug)]
enum Expr {
    Ident(String),
    Int(i64),
    Str(String),
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
struct Cond {
    op: CmpOp,
    left: Expr,
    right: Expr,
}

#[derive(Debug)]
enum Stmt {
    Display(Expr),
    Move {
        value: Expr,
        target: String,
    },
    Add {
        value: Expr,
        target: String,
    },
    Subtract {
        value: Expr,
        target: String,
    },
    Perform {
        para: String,
    },
    PerformUntil {
        cond: Cond,
        body: Vec<Stmt>,
    },
    StopRun,
}

#[derive(Debug)]
struct Paragraph {
    name: String,
    body: Vec<Stmt>,
}

#[derive(Debug)]
struct Program {
    ws_items: Vec<WSItem>,
    main_code: Vec<Stmt>,
    paragraphs: Vec<Paragraph>,
}

// Convert COBOL ident (uppercase, hyphens) to wei ident (lowercase, underscores).
fn to_wei_ident(s: &str) -> String {
    s.to_lowercase().replace('-', "_")
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
        let mut program = Program {
            ws_items: Vec::new(),
            main_code: Vec::new(),
            paragraphs: Vec::new(),
        };

        // Walk through divisions
        while !self.at_eof() {
            match self.peek() {
                Token::Word(w) if w == "DATA" => {
                    self.advance();
                    self.expect_word("DIVISION");
                    self.expect_period();
                    self.parse_data_division(&mut program);
                }
                Token::Word(w) if w == "PROCEDURE" => {
                    self.advance();
                    self.expect_word("DIVISION");
                    self.expect_period();
                    self.parse_procedure_division(&mut program);
                    break;
                }
                _ => {
                    // Skip IDENTIFICATION / ENVIRONMENT preamble token by token
                    self.advance();
                }
            }
        }

        program
    }

    fn parse_data_division(&mut self, program: &mut Program) {
        // Skip until WORKING-STORAGE SECTION
        loop {
            if self.at_eof() {
                return;
            }
            match self.peek() {
                Token::Word(w) if w == "WORKING-STORAGE" => {
                    self.advance();
                    self.expect_word("SECTION");
                    self.expect_period();
                    self.parse_working_storage(program);
                    return;
                }
                Token::Word(w) if w == "PROCEDURE" => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_working_storage(&mut self, program: &mut Program) {
        loop {
            match self.peek() {
                Token::IntLit(1) => {
                    self.advance();
                    let name = self.expect_word_any();
                    self.expect_word("PIC");
                    let ty = self.parse_pic();
                    let value = if matches!(self.peek(), Token::Word(w) if w == "VALUE") {
                        self.advance();
                        Some(self.parse_literal())
                    } else {
                        None
                    };
                    self.expect_period();
                    program.ws_items.push(WSItem {
                        name: to_wei_ident(&name),
                        ty,
                        value,
                    });
                }
                Token::Word(w) if w == "PROCEDURE" => return,
                Token::Eof => return,
                other => panic!(
                    "cobol2wei: unexpected token in WORKING-STORAGE: {:?}",
                    other
                ),
            }
        }
    }

    fn parse_pic(&mut self) -> PicType {
        // PIC X, PIC X(N), PIC 9, PIC 9(N), PIC 9(N)V99
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
                self.advance(); // consume the leading 9-pattern digit literal
                let n = if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let n = self.expect_intlit() as u32;
                    self.expect_rparen();
                    n
                } else {
                    1
                };
                // Optional V99 fractional part
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
            let s = self.parse_statement();
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

    fn parse_statement(&mut self) -> Stmt {
        match self.peek().clone() {
            Token::Word(w) if w == "DISPLAY" => {
                self.advance();
                let e = self.parse_expr();
                self.expect_period();
                Stmt::Display(e)
            }
            Token::Word(w) if w == "MOVE" => {
                self.advance();
                let value = self.parse_expr();
                self.expect_word("TO");
                let target = self.expect_word_any();
                self.expect_period();
                Stmt::Move {
                    value,
                    target: to_wei_ident(&target),
                }
            }
            Token::Word(w) if w == "ADD" => {
                self.advance();
                let value = self.parse_expr();
                self.expect_word("TO");
                let target = self.expect_word_any();
                self.expect_period();
                Stmt::Add {
                    value,
                    target: to_wei_ident(&target),
                }
            }
            Token::Word(w) if w == "SUBTRACT" => {
                self.advance();
                let value = self.parse_expr();
                self.expect_word("FROM");
                let target = self.expect_word_any();
                self.expect_period();
                Stmt::Subtract {
                    value,
                    target: to_wei_ident(&target),
                }
            }
            Token::Word(w) if w == "PERFORM" => {
                self.advance();
                // PERFORM UNTIL cond ... END-PERFORM, or PERFORM paragraph-name.
                if matches!(self.peek(), Token::Word(w) if w == "UNTIL") {
                    self.advance();
                    let cond = self.parse_condition();
                    let mut body = Vec::new();
                    while !matches!(self.peek(), Token::Word(w) if w == "END-PERFORM") {
                        if self.at_eof() {
                            panic!("cobol2wei: unterminated PERFORM UNTIL (missing END-PERFORM)");
                        }
                        body.push(self.parse_statement_no_period());
                    }
                    self.expect_word("END-PERFORM");
                    self.expect_period();
                    Stmt::PerformUntil { cond, body }
                } else {
                    let para = self.expect_word_any();
                    self.expect_period();
                    Stmt::Perform {
                        para: to_wei_ident(&para),
                    }
                }
            }
            Token::Word(w) if w == "STOP" => {
                self.advance();
                self.expect_word("RUN");
                self.expect_period();
                Stmt::StopRun
            }
            other => panic!("cobol2wei: unsupported statement starting with {:?}", other),
        }
    }

    // Inside PERFORM UNTIL ... END-PERFORM, the inner statements don't need their own
    // periods (COBOL convention — periods only outside the block). We accept either form.
    fn parse_statement_no_period(&mut self) -> Stmt {
        // Backup approach: just call parse_statement. The trailing period is optional
        // for inline statements. For simplicity, require it for now.
        // COBOL allows the inner statements without periods inside PERFORM ... END-PERFORM.
        match self.peek().clone() {
            Token::Word(w) if w == "ADD" => {
                self.advance();
                let value = self.parse_expr();
                self.expect_word("TO");
                let target = self.expect_word_any();
                self.consume_period();
                Stmt::Add {
                    value,
                    target: to_wei_ident(&target),
                }
            }
            Token::Word(w) if w == "SUBTRACT" => {
                self.advance();
                let value = self.parse_expr();
                self.expect_word("FROM");
                let target = self.expect_word_any();
                self.consume_period();
                Stmt::Subtract {
                    value,
                    target: to_wei_ident(&target),
                }
            }
            Token::Word(w) if w == "MOVE" => {
                self.advance();
                let value = self.parse_expr();
                self.expect_word("TO");
                let target = self.expect_word_any();
                self.consume_period();
                Stmt::Move {
                    value,
                    target: to_wei_ident(&target),
                }
            }
            Token::Word(w) if w == "DISPLAY" => {
                self.advance();
                let e = self.parse_expr();
                self.consume_period();
                Stmt::Display(e)
            }
            Token::Word(w) if w == "PERFORM" => {
                self.advance();
                if matches!(self.peek(), Token::Word(w) if w == "UNTIL") {
                    self.advance();
                    let cond = self.parse_condition();
                    let mut body = Vec::new();
                    while !matches!(self.peek(), Token::Word(w) if w == "END-PERFORM") {
                        if self.at_eof() {
                            panic!("cobol2wei: unterminated nested PERFORM UNTIL");
                        }
                        body.push(self.parse_statement_no_period());
                    }
                    self.expect_word("END-PERFORM");
                    self.consume_period();
                    Stmt::PerformUntil { cond, body }
                } else {
                    let para = self.expect_word_any();
                    self.consume_period();
                    Stmt::Perform {
                        para: to_wei_ident(&para),
                    }
                }
            }
            other => panic!(
                "cobol2wei: unsupported statement inside block: {:?}",
                other
            ),
        }
    }

    fn parse_condition(&mut self) -> Cond {
        let left = self.parse_expr();
        // Check for NOT prefix on operator
        let mut negated = false;
        if matches!(self.peek(), Token::Word(w) if w == "NOT") {
            self.advance();
            negated = true;
        }
        let op = match self.advance() {
            Token::Eq => CmpOp::Eq,
            Token::Lt => CmpOp::Lt,
            Token::Gt => CmpOp::Gt,
            Token::LtEq => CmpOp::Le,
            Token::GtEq => CmpOp::Ge,
            other => panic!("cobol2wei: expected comparison operator, got {:?}", other),
        };
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
        Cond {
            op: final_op,
            left,
            right,
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

    fn consume_period(&mut self) {
        if matches!(self.peek(), Token::Period) {
            self.advance();
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
            | "IF"
            | "EVALUATE"
            | "COMPUTE"
            | "SET"
    )
}

fn emit(program: &Program) -> String {
    let mut out = String::new();

    // WORKING-STORAGE → top-level lets
    for item in &program.ws_items {
        let ty_str = match &item.ty {
            PicType::Str(n) => format!("str({})", n),
            PicType::UInt(n) => format!("uint({})", n),
            PicType::UDec(n, m) => format!("udec({},{})", n, m),
        };
        out.push_str(&format!("// COBOL: 01 {} PIC ...\n", item.name.to_uppercase()));
        match &item.value {
            Some(Literal::Int(v)) => {
                out.push_str(&format!("let {} {} = {}\n", item.name, ty_str, v));
            }
            Some(Literal::Str(s)) => {
                out.push_str(&format!("let {} {} = \"{}\"\n", item.name, ty_str, s));
            }
            None => {
                out.push_str(&format!("let {} {}\n", item.name, ty_str));
            }
        }
    }
    out.push('\n');

    // Paragraphs
    for p in &program.paragraphs {
        out.push_str(&format!("// COBOL: {}.\n", p.name.to_uppercase().replace('_', "-")));
        out.push_str(&format!("par {}:\n", p.name));
        for s in &p.body {
            emit_stmt(&mut out, s, 1);
        }
        out.push('\n');
    }

    // Main code (either top-level statements, or call to the first paragraph)
    if !program.main_code.is_empty() {
        out.push_str("// COBOL: (procedure body / main)\n");
        for s in &program.main_code {
            emit_stmt(&mut out, s, 0);
        }
    } else if !program.paragraphs.is_empty() {
        // No anonymous main, call first paragraph as the implicit entry
        let first = &program.paragraphs[0];
        out.push_str(&format!("{}()\n", first.name));
    }

    out
}

fn emit_stmt(out: &mut String, s: &Stmt, indent: usize) {
    let pad = "    ".repeat(indent);
    match s {
        Stmt::Display(e) => {
            out.push_str(&format!("{}// COBOL: DISPLAY {}.\n", pad, expr_cobol_text(e)));
            out.push_str(&format!("{}print({})\n", pad, expr_wei_text(e)));
        }
        Stmt::Move { value, target } => {
            out.push_str(&format!(
                "{}// COBOL: MOVE {} TO {}.\n",
                pad,
                expr_cobol_text(value),
                target.to_uppercase().replace('_', "-")
            ));
            out.push_str(&format!(
                "{}{} = {}\n",
                pad,
                target,
                expr_wei_text(value)
            ));
        }
        Stmt::Add { value, target } => {
            out.push_str(&format!(
                "{}// COBOL: ADD {} TO {}.\n",
                pad,
                expr_cobol_text(value),
                target.to_uppercase().replace('_', "-")
            ));
            out.push_str(&format!(
                "{}{} = {} + {}\n",
                pad,
                target,
                target,
                expr_wei_text(value)
            ));
        }
        Stmt::Subtract { value, target } => {
            out.push_str(&format!(
                "{}// COBOL: SUBTRACT {} FROM {}.\n",
                pad,
                expr_cobol_text(value),
                target.to_uppercase().replace('_', "-")
            ));
            out.push_str(&format!(
                "{}{} = {} - {}\n",
                pad,
                target,
                target,
                expr_wei_text(value)
            ));
        }
        Stmt::Perform { para } => {
            out.push_str(&format!(
                "{}// COBOL: PERFORM {}.\n",
                pad,
                para.to_uppercase().replace('_', "-")
            ));
            out.push_str(&format!("{}{}()\n", pad, para));
        }
        Stmt::PerformUntil { cond, body } => {
            out.push_str(&format!(
                "{}// COBOL: PERFORM UNTIL {} ...\n",
                pad,
                cond_cobol_text(cond)
            ));
            out.push_str(&format!(
                "{}while !({}):\n",
                pad,
                cond_wei_text(cond)
            ));
            for s in body {
                emit_stmt(out, s, indent + 1);
            }
        }
        Stmt::StopRun => {
            out.push_str(&format!("{}// COBOL: STOP RUN.\n", pad));
        }
    }
}

fn expr_wei_text(e: &Expr) -> String {
    match e {
        Expr::Ident(n) => n.clone(),
        Expr::Int(v) => v.to_string(),
        Expr::Str(s) => format!("\"{}\"", s),
    }
}

fn expr_cobol_text(e: &Expr) -> String {
    match e {
        Expr::Ident(n) => n.to_uppercase().replace('_', "-"),
        Expr::Int(v) => v.to_string(),
        Expr::Str(s) => format!("\"{}\"", s),
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

fn cond_wei_text(c: &Cond) -> String {
    format!(
        "{} {} {}",
        expr_wei_text(&c.left),
        cmp_wei(c.op),
        expr_wei_text(&c.right)
    )
}

fn cond_cobol_text(c: &Cond) -> String {
    format!(
        "{} {} {}",
        expr_cobol_text(&c.left),
        cmp_cobol(c.op),
        expr_cobol_text(&c.right)
    )
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
