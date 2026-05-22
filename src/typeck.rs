use crate::ast::{BinOp, Expr, LValue, Program, Stmt, TypeExpr};
use std::collections::HashMap;

pub fn check(program: &Program) -> Result<(), Vec<String>> {
    let mut c = Checker {
        symbols: HashMap::new(),
        records: HashMap::new(),
        errors: Vec::new(),
    };

    for stmt in &program.statements {
        if let Stmt::TypeDef { name, fields } = stmt {
            if c.records.contains_key(name) {
                c.errors.push(format!("type `{}` defined more than once", name));
            } else {
                c.records.insert(name.clone(), fields.clone());
            }
        }
        if let Stmt::FileDecl { name, mode, .. } = stmt {
            if mode != "sequential" {
                c.errors.push(format!(
                    "file `{}`: only `sequential` mode is supported in v0.3 (got `{}`)",
                    name, mode
                ));
            }
            c.symbols.insert(name.clone(), TypeExpr::File);
        }
    }

    c.check_block(&program.statements);
    if c.errors.is_empty() {
        Ok(())
    } else {
        Err(c.errors)
    }
}

struct Checker {
    symbols: HashMap<String, TypeExpr>,
    records: HashMap<String, Vec<(String, TypeExpr)>>,
    errors: Vec<String>,
}

impl Checker {
    fn check_block(&mut self, statements: &[Stmt]) {
        for stmt in statements {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::TypeDef { .. } => {}
            Stmt::Let { name, ty, init } => {
                if let TypeExpr::Record(rname) = ty {
                    if !self.records.contains_key(rname) {
                        self.errors.push(format!(
                            "let `{}` references undefined type `{}`",
                            name, rname
                        ));
                    }
                }
                if let (TypeExpr::UInt(_), Some(init)) = (ty, init) {
                    if let Some(v) = try_const_int(init) {
                        self.check_fits(v, ty, &format!("initializer of `{}`", name));
                    }
                }
                if let (TypeExpr::Str(n), Some(Expr::StringLit(s))) = (ty, init) {
                    if s.len() > *n as usize {
                        self.errors.push(format!(
                            "initializer of `{}`: string of {} bytes exceeds str({})",
                            name,
                            s.len(),
                            n
                        ));
                    }
                }
                if let (
                    TypeExpr::UDec(big_n, m) | TypeExpr::IDec(big_n, m),
                    Some(init_expr),
                ) = (ty, init)
                {
                    self.check_decimal_init(init_expr, *big_n, *m, name);
                }
                self.symbols.insert(name.clone(), ty.clone());
            }
            Stmt::Assign { target, value } => {
                let target_ty = self.resolve_lvalue_type(target);
                if let Some(t) = &target_ty {
                    if matches!(t, TypeExpr::Str(_)) {
                        self.errors.push(format!(
                            "runtime assignment to str(N) is not yet supported (only let-init)"
                        ));
                    }
                    if let TypeExpr::UDec(_, m) | TypeExpr::IDec(_, m) = t {
                        if let Expr::DecLit { scale, .. } = value {
                            if *scale != *m {
                                self.errors.push(format!(
                                    "assignment: decimal literal scale {} doesn't match target scale {}",
                                    scale, m
                                ));
                            }
                        }
                    }
                }
                if let Some(t) = target_ty {
                    if let Some(v) = try_const_int(value) {
                        self.check_fits(v, &t, &format!("assignment to {:?}", target));
                    }
                }
            }
            Stmt::Par { body, .. } => {
                self.check_block(body);
            }
            Stmt::While { body, .. } => {
                self.check_block(body);
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                self.check_block(then_body);
                self.check_block(else_body);
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    self.check_block(&arm.body);
                }
            }
            Stmt::For { var, body, .. } => {
                if self.symbols.contains_key(var) {
                    self.errors.push(format!(
                        "loop variable `{}` shadows existing symbol",
                        var
                    ));
                    self.check_block(body);
                } else {
                    self.symbols.insert(var.clone(), TypeExpr::UInt(18));
                    self.check_block(body);
                    self.symbols.remove(var);
                }
            }
            Stmt::Call { .. } => {}
            Stmt::FileDecl { .. } => {}
        }
    }

    fn resolve_lvalue_type(&mut self, lv: &LValue) -> Option<TypeExpr> {
        match lv {
            LValue::Ident(name) => match self.symbols.get(name).cloned() {
                Some(t) => Some(t),
                None => {
                    self.errors
                        .push(format!("assignment to undeclared `{}`", name));
                    None
                }
            },
            LValue::Field { base, field } => {
                let base_ty = self.resolve_lvalue_type(base)?;
                match base_ty {
                    TypeExpr::Record(rname) => {
                        let fields = self.records.get(&rname)?;
                        match fields.iter().find(|(n, _)| n == field) {
                            Some((_, t)) => Some(t.clone()),
                            None => {
                                self.errors.push(format!(
                                    "record `{}` has no field `{}`",
                                    rname, field
                                ));
                                None
                            }
                        }
                    }
                    _ => {
                        self.errors
                            .push(format!("cannot access field `{}` on non-record", field));
                        None
                    }
                }
            }
        }
    }

    fn check_fits(&mut self, value: i64, ty: &TypeExpr, context: &str) {
        match ty {
            TypeExpr::UInt(n) => {
                if value < 0 {
                    self.errors.push(format!(
                        "{}: value {} is negative, but type is uint({})",
                        context, value, n
                    ));
                    return;
                }
                let max = 10i64.checked_pow(*n).map(|x| x - 1).unwrap_or(i64::MAX);
                if value > max {
                    self.errors.push(format!(
                        "{}: value {} exceeds maximum {} for uint({})",
                        context, value, max, n
                    ));
                }
            }
            TypeExpr::UDec(_, _) | TypeExpr::IDec(_, _) => {
                self.errors.push(format!(
                    "{}: cannot assign a bare integer to a decimal at runtime — use a decimal literal",
                    context
                ));
            }
            TypeExpr::Str(_) | TypeExpr::Record(_) | TypeExpr::File => {
                self.errors
                    .push(format!("{}: cannot assign a number to this type", context));
            }
        }
    }

    fn check_decimal_init(&mut self, init: &Expr, big_n: u32, m: u32, var_name: &str) {
        let max_scaled = 10i64.checked_pow(big_n + m).map(|x| x - 1).unwrap_or(i64::MAX);
        match init {
            Expr::IntLit(v) => {
                let scale_factor = 10i64.checked_pow(m).unwrap_or(i64::MAX);
                let scaled = v.checked_mul(scale_factor);
                match scaled {
                    Some(s) if s >= 0 && s <= max_scaled => {}
                    Some(s) => self.errors.push(format!(
                        "initializer of `{}`: value {} (scaled {}) exceeds range for decimal({},{})",
                        var_name, v, s, big_n, m
                    )),
                    None => self.errors.push(format!(
                        "initializer of `{}`: value {} overflows when scaled to decimal({},{})",
                        var_name, v, big_n, m
                    )),
                }
            }
            Expr::DecLit { scaled, scale } => {
                if *scale != m {
                    self.errors.push(format!(
                        "initializer of `{}`: literal has scale {} but declared scale is {}",
                        var_name, scale, m
                    ));
                } else if *scaled < 0 || *scaled > max_scaled {
                    self.errors.push(format!(
                        "initializer of `{}`: scaled value {} exceeds range for decimal({},{})",
                        var_name, scaled, big_n, m
                    ));
                }
            }
            _ => self.errors.push(format!(
                "initializer of `{}`: decimal init must be a literal",
                var_name
            )),
        }
    }
}

fn try_const_int(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::IntLit(n) => Some(*n),
        Expr::DecLit { .. }
        | Expr::Ident(_)
        | Expr::StringLit(_)
        | Expr::Compare { .. }
        | Expr::Not { .. }
        | Expr::And { .. }
        | Expr::Or { .. }
        | Expr::FieldAccess { .. }
        | Expr::Call { .. } => None,
        Expr::BinaryOp { op, left, right } => {
            let l = try_const_int(left)?;
            let r = try_const_int(right)?;
            match op {
                BinOp::Add => l.checked_add(r),
                BinOp::Sub => l.checked_sub(r),
                BinOp::Mul => l.checked_mul(r),
                BinOp::Div => {
                    if r != 0 {
                        l.checked_div(r)
                    } else {
                        None
                    }
                }
                BinOp::Mod => {
                    if r != 0 {
                        l.checked_rem(r)
                    } else {
                        None
                    }
                }
            }
        }
    }
}
