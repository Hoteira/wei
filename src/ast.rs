#[derive(Debug)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug)]
pub enum Stmt {
    Let {
        name: String,
        ty: TypeExpr,
        init: Option<Expr>,
        eighty_eights: Vec<(String, Expr)>,
    },
    Assign {
        target: LValue,
        value: Expr,
    },
    Par {
        name: String,
        body: Vec<Stmt>,
    },
    For {
        var: String,
        start: Expr,
        end: Expr,
        body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    Match {
        expr: Expr,
        arms: Vec<MatchArm>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    TypeDef {
        name: String,
        fields: Vec<(String, TypeExpr)>,
        redefines: Vec<(String, String)>,
    },
    FileDecl {
        name: String,
        path: String,
        mode: String,
        key: Option<String>,
    },
    Sub {
        name: String,
        params: Vec<(String, TypeExpr)>,
        body: Vec<Stmt>,
    },
}

#[derive(Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub enum Pattern {
    Lit(i64),
    Range(i64, i64),
    StrLit(String),
    Wildcard,
}

#[derive(Debug, Clone)]
pub enum LValue {
    Ident(String),
    Field { base: Box<LValue>, field: String },
}

#[derive(Debug, Clone)]
pub enum TypeExpr {
    UInt(u32),
    Str(u32),
    UDec(u32, u32),
    IDec(u32, u32),
    Record(String),
    File,
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, Copy)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug)]
pub enum Expr {
    StringLit(String),
    IntLit(i64),
    DecLit { scaled: i64, scale: u32 },
    Ident(String),
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Compare {
        op: CmpOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Not {
        inner: Box<Expr>,
    },
    And {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Or {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    FieldAccess {
        base: Box<Expr>,
        field: String,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
}
