use crate::ast::{BinOp, Expr, Program, Stmt, TypeExpr};
use std::collections::HashMap;

pub fn check(program: &Program) -> Result<(), Vec<String>> {
    let mut c = Checker {
        symbols: HashMap::new(),
        errors: Vec::new(),
    };
    c.check_block(&program.statements);
    if c.errors.is_empty() {
        Ok(())
    } else {
        Err(c.errors)
    }
}

struct Checker {
    symbols: HashMap<String, TypeExpr>,
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
            Stmt::Let { name, ty, init } => {
                if let Some(v) = try_const_int(init) {
                    self.check_fits(v, ty, &format!("initializer of `{}`", name));
                }
                self.symbols.insert(name.clone(), ty.clone());
            }
            Stmt::Assign { name, value } => {
                let var_ty = match self.symbols.get(name) {
                    Some(t) => t.clone(),
                    None => {
                        self.errors
                            .push(format!("assignment to undeclared `{}`", name));
                        return;
                    }
                };
                if let Some(v) = try_const_int(value) {
                    self.check_fits(v, &var_ty, &format!("assignment to `{}`", name));
                }
            }
            Stmt::Par { body, .. } => {
                self.check_block(body);
            }
            Stmt::While { body, .. } => {
                self.check_block(body);
            }
            Stmt::For { var, body, .. } => {
                if self.symbols.contains_key(var) {
                    self.errors.push(format!(
                        "loop variable `{}` shadows existing symbol",
                        var
                    ));
                    self.check_block(body);
                } else {
                    // Default loop counter type — wide enough for any practical range.
                    // TODO: infer from bounds once we have full expression typing.
                    self.symbols.insert(var.clone(), TypeExpr::UInt(18));
                    self.check_block(body);
                    self.symbols.remove(var);
                }
            }
            Stmt::Call { .. } => {
                // Call argument types are unchecked in step 1.
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
        }
    }
}

fn try_const_int(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::IntLit(n) => Some(*n),
        Expr::Ident(_) | Expr::StringLit(_) | Expr::Compare { .. } | Expr::Not { .. } => None,
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
