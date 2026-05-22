#[derive(Debug)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug)]
pub enum Stmt {
    Call { name: String, args: Vec<Expr> },
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug)]
pub enum Expr {
    StringLit(String),
    IntLit(i64),
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}
