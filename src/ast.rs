#[derive(Debug)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug)]
pub enum Stmt {
    Call { name: String, args: Vec<Expr> },
}

#[derive(Debug)]
pub enum Expr {
    StringLit(String),
    IntLit(i64),
}
