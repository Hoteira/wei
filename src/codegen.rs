use crate::ast::{BinOp, CmpOp, Expr, LValue, Program, Stmt, TypeExpr};
use crate::elf::ENTRY_VMA;
use std::collections::HashMap;

const RAX: u8 = 0;
const RCX: u8 = 1;
const RDX: u8 = 2;
const RBX: u8 = 3;
const RSI: u8 = 6;
const RDI: u8 = 7;

const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;
const FD_STDOUT: u64 = 1;

const SCRATCH_SIZE: u64 = 32;
const SCRATCH_END: u64 = SCRATCH_SIZE;

pub fn emit(program: &Program) -> Vec<u8> {
    let mut g = Codegen::new();
    g.register_typedefs(program);
    g.collect_symbols(program);
    g.register_paragraphs(program);

    for stmt in &program.statements {
        if !matches!(stmt, Stmt::Par { .. } | Stmt::TypeDef { .. }) {
            g.emit_stmt(stmt);
        }
    }
    g.emit_exit();

    for stmt in &program.statements {
        if let Stmt::Par { name, body } = stmt {
            g.start_paragraph(name);
            for s in body {
                g.emit_stmt(s);
            }
            g.emit_ret();
        }
    }

    g.finalize()
}

struct Symbol {
    name: String,
    offset_in_data: u64,
    ty: TypeExpr,
}

struct Reloc {
    code_pos: usize,
    data_offset: u64,
}

struct RecordInfo {
    fields: Vec<RecordField>,
    size: u64,
}

struct RecordField {
    name: String,
    offset: u64,
    ty: TypeExpr,
}

struct Codegen {
    code: Vec<u8>,
    data: Vec<u8>,
    symbols: Vec<Symbol>,
    relocs: Vec<Reloc>,
    paragraphs: Vec<(String, Option<usize>)>,
    par_calls: Vec<(usize, String)>,
    record_types: HashMap<String, RecordInfo>,
}

impl Codegen {
    fn new() -> Self {
        let mut data = Vec::new();
        data.resize(SCRATCH_SIZE as usize, 0);
        Self {
            code: Vec::new(),
            data,
            symbols: Vec::new(),
            relocs: Vec::new(),
            paragraphs: Vec::new(),
            par_calls: Vec::new(),
            record_types: HashMap::new(),
        }
    }

    fn register_typedefs(&mut self, program: &Program) {
        for stmt in &program.statements {
            if let Stmt::TypeDef { name, fields } = stmt {
                let mut offset = 0u64;
                let mut field_list = Vec::new();
                for (fname, ty) in fields {
                    let field_size = type_size(ty);
                    field_list.push(RecordField {
                        name: fname.clone(),
                        offset,
                        ty: ty.clone(),
                    });
                    offset += field_size;
                }
                self.record_types.insert(
                    name.clone(),
                    RecordInfo {
                        fields: field_list,
                        size: offset,
                    },
                );
            }
        }
    }

    fn register_paragraphs(&mut self, program: &Program) {
        for stmt in &program.statements {
            if let Stmt::Par { name, .. } = stmt {
                if self.paragraphs.iter().any(|(n, _)| n == name) {
                    panic!("codegen: paragraph `{}` defined more than once", name);
                }
                self.paragraphs.push((name.clone(), None));
            }
        }
    }

    fn start_paragraph(&mut self, name: &str) {
        let offset = self.code.len();
        let entry = self
            .paragraphs
            .iter_mut()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("codegen: paragraph `{}` not registered", name));
        entry.1 = Some(offset);
    }

    fn has_paragraph(&self, name: &str) -> bool {
        self.paragraphs.iter().any(|(n, _)| n == name)
    }

    fn emit_call_paragraph(&mut self, name: &str) {
        self.code.push(0xE8);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        self.par_calls.push((pos, name.to_string()));
    }

    fn emit_ret(&mut self) {
        self.code.push(0xC3);
    }

    fn collect_symbols(&mut self, program: &Program) {
        for stmt in &program.statements {
            if let Stmt::Let { name, ty, init } = stmt {
                let offset = self.data.len() as u64;
                match (ty, init) {
                    (TypeExpr::UInt(_), Some(init_expr)) => {
                        let v = eval_const(init_expr).expect_int();
                        self.data.extend_from_slice(&(v as u64).to_le_bytes());
                    }
                    (TypeExpr::UInt(_), None) => {
                        self.data.extend_from_slice(&[0u8; 8]);
                    }
                    (TypeExpr::Str(n), Some(init_expr)) => {
                        let s = match eval_const(init_expr) {
                            ConstValue::Str(s) => s,
                            _ => panic!("codegen: str init must be a string literal"),
                        };
                        let width = *n as usize;
                        if s.len() > width {
                            panic!(
                                "codegen: string literal of {} bytes exceeds str({})",
                                s.len(),
                                width
                            );
                        }
                        let mut bytes = s.into_bytes();
                        bytes.resize(width, b' ');
                        self.data.extend_from_slice(&bytes);
                    }
                    (TypeExpr::Str(n), None) => {
                        self.data.extend_from_slice(&vec![b' '; *n as usize]);
                    }
                    (TypeExpr::UDec(_, m), Some(init_expr))
                    | (TypeExpr::IDec(_, m), Some(init_expr)) => {
                        let scaled = decimal_init_value(init_expr, *m);
                        self.data.extend_from_slice(&(scaled as u64).to_le_bytes());
                    }
                    (TypeExpr::UDec(_, _), None) | (TypeExpr::IDec(_, _), None) => {
                        self.data.extend_from_slice(&[0u8; 8]);
                    }
                    (TypeExpr::Record(rname), _) => {
                        let info = self.record_types.get(rname).unwrap_or_else(|| {
                            panic!("codegen: undefined record type `{}`", rname)
                        });
                        self.data.extend_from_slice(&vec![0u8; info.size as usize]);
                    }
                }
                self.symbols.push(Symbol {
                    name: name.clone(),
                    offset_in_data: offset,
                    ty: ty.clone(),
                });
            }
        }
    }

    fn lvalue_data_offset(&self, lv: &LValue) -> u64 {
        match lv {
            LValue::Ident(name) => self.lookup_symbol(name).offset_in_data,
            LValue::Field { base, field } => {
                let (base_offset, base_record) = self.lvalue_record_info(base);
                let info = self
                    .record_types
                    .get(&base_record)
                    .unwrap_or_else(|| panic!("codegen: unknown record `{}`", base_record));
                let f = info
                    .fields
                    .iter()
                    .find(|f| f.name == *field)
                    .unwrap_or_else(|| {
                        panic!("codegen: no field `{}` in record `{}`", field, base_record)
                    });
                base_offset + f.offset
            }
        }
    }

    fn lvalue_record_info(&self, lv: &LValue) -> (u64, String) {
        match lv {
            LValue::Ident(name) => {
                let sym = self.lookup_symbol(name);
                let r = match &sym.ty {
                    TypeExpr::Record(rname) => rname.clone(),
                    _ => panic!("codegen: `{}` is not a record", name),
                };
                (sym.offset_in_data, r)
            }
            LValue::Field { .. } => {
                panic!("codegen: nested record field access not supported yet");
            }
        }
    }

    fn lookup_symbol(&self, name: &str) -> &Symbol {
        self.symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("codegen: unknown identifier `{}`", name))
    }

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { .. } | Stmt::Par { .. } | Stmt::TypeDef { .. } => {}
            Stmt::Assign { target, value } => self.emit_assign(target, value),
            Stmt::For {
                var, start, end, body,
            } => self.emit_for(var, start, end, body),
            Stmt::While { cond, body } => self.emit_while(cond, body),
            Stmt::Call { name, args } => {
                if name == "print" {
                    if args.len() != 1 {
                        panic!("codegen: print() takes exactly one argument");
                    }
                    self.emit_print(&args[0]);
                } else if self.has_paragraph(name) {
                    if !args.is_empty() {
                        panic!("codegen: paragraph `{}` is param-less", name);
                    }
                    self.emit_call_paragraph(name);
                } else {
                    panic!("codegen: unknown function `{}`", name);
                }
            }
        }
    }

    fn emit_while(&mut self, cond: &Expr, body: &[Stmt]) {
        let loop_start = self.code.len();
        self.emit_expr(cond);
        self.emit_test_rax_rax();
        let jz_pos = self.emit_jz_placeholder();

        for s in body {
            self.emit_stmt(s);
        }

        self.emit_jmp_back_to(loop_start);

        let loop_end = self.code.len();
        self.patch_rel32(jz_pos, loop_end);
    }

    fn emit_for(&mut self, var: &str, start: &Expr, end: &Expr, body: &[Stmt]) {
        if self.symbols.iter().any(|s| s.name == var) {
            panic!("codegen: loop variable `{}` shadows existing symbol", var);
        }
        let var_offset = self.data.len() as u64;
        self.data.extend_from_slice(&[0u8; 8]);
        self.symbols.push(Symbol {
            name: var.to_string(),
            offset_in_data: var_offset,
            ty: TypeExpr::UInt(18),
        });

        self.emit_expr(start);
        self.emit_mov_imm64_reloc(RBX, var_offset);
        self.emit_mov_at_rbx_rax();

        self.emit_expr(end);
        self.emit_push_rax();

        let loop_start = self.code.len();
        self.emit_mov_imm64_reloc(RBX, var_offset);
        self.emit_mov_rax_from_rbx();
        self.emit_mov_rbx_from_rsp_off(0);
        self.emit_cmp_rax_rbx();
        let jge_pos = self.emit_jge_placeholder();

        for s in body {
            self.emit_stmt(s);
        }

        self.emit_mov_imm64_reloc(RBX, var_offset);
        self.emit_mov_rax_from_rbx();
        self.emit_inc_rax();
        self.emit_mov_at_rbx_rax();

        self.emit_jmp_back_to(loop_start);

        let loop_end = self.code.len();
        self.patch_rel32(jge_pos, loop_end);

        self.emit_add_rsp_imm8(8);

        self.symbols.pop();
    }

    fn emit_assign(&mut self, target: &LValue, value: &Expr) {
        self.emit_expr(value);
        let offset = self.lvalue_data_offset(target);
        self.emit_mov_imm64_reloc(RBX, offset);
        self.emit_mov_at_rbx_rax();
    }

    fn emit_print(&mut self, arg: &Expr) {
        if let Some(c) = try_eval_const(arg) {
            let s = match c {
                ConstValue::Str(s) => s,
                ConstValue::Int(n) => n.to_string(),
            };
            self.emit_print_const(s);
        } else if let Some((offset, len)) = self.try_resolve_str_ident(arg) {
            self.emit_mov_imm64(RAX, SYS_WRITE);
            self.emit_mov_imm64(RDI, FD_STDOUT);
            self.emit_mov_imm64_reloc(RSI, offset);
            self.emit_mov_imm64(RDX, len);
            self.emit_syscall();
        } else if let Some(scale) = self.try_resolve_decimal_ident(arg) {
            self.emit_expr(arg);
            self.emit_print_rax_decimal(scale);
        } else {
            self.emit_expr(arg);
            self.emit_print_rax_int();
        }
    }

    fn try_resolve_decimal_ident(&self, arg: &Expr) -> Option<u32> {
        match arg {
            Expr::Ident(name) => {
                let sym = self.lookup_symbol(name);
                match &sym.ty {
                    TypeExpr::UDec(_, m) | TypeExpr::IDec(_, m) => Some(*m),
                    _ => None,
                }
            }
            Expr::FieldAccess { base, field } => {
                let base_name = match base.as_ref() {
                    Expr::Ident(n) => n,
                    _ => return None,
                };
                let sym = self.lookup_symbol(base_name);
                if let TypeExpr::Record(rname) = &sym.ty {
                    let info = self.record_types.get(rname)?;
                    let f = info.fields.iter().find(|f| f.name == *field)?;
                    if let TypeExpr::UDec(_, m) | TypeExpr::IDec(_, m) = &f.ty {
                        return Some(*m);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn try_resolve_str_ident(&self, arg: &Expr) -> Option<(u64, u64)> {
        match arg {
            Expr::Ident(name) => {
                let sym = self.lookup_symbol(name);
                if let TypeExpr::Str(n) = &sym.ty {
                    return Some((sym.offset_in_data, *n as u64));
                }
                None
            }
            Expr::FieldAccess { base, field } => {
                let base_name = match base.as_ref() {
                    Expr::Ident(n) => n,
                    _ => return None,
                };
                let sym = self.lookup_symbol(base_name);
                if let TypeExpr::Record(rname) = &sym.ty {
                    let info = self.record_types.get(rname)?;
                    let f = info.fields.iter().find(|f| f.name == *field)?;
                    if let TypeExpr::Str(n) = &f.ty {
                        return Some((sym.offset_in_data + f.offset, *n as u64));
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn emit_print_const(&mut self, s: String) {
        let bytes = s.into_bytes();
        let data_offset = self.data.len() as u64;
        let len = bytes.len() as u64;
        self.data.extend_from_slice(&bytes);

        self.emit_mov_imm64(RAX, SYS_WRITE);
        self.emit_mov_imm64(RDI, FD_STDOUT);
        self.emit_mov_imm64_reloc(RSI, data_offset);
        self.emit_mov_imm64(RDX, len);
        self.emit_syscall();
    }

    fn emit_print_rax_decimal(&mut self, scale: u32) {
        self.emit_mov_imm64_reloc(RSI, SCRATCH_END);
        self.emit_mov_imm64(RCX, 10);
        self.emit_mov_imm64(RDI, 0);

        let loop_start = self.code.len();

        self.emit_cmp_rdi_imm8(scale as u8);
        let jne_pos = self.emit_jne_placeholder();
        self.emit_dec_rsi();
        self.emit_mov_byte_at_rsi_imm(b'.');
        let skip_dot = self.code.len();
        self.patch_rel32(jne_pos, skip_dot);

        self.emit_xor_rdx_rdx();
        self.emit_div_rcx();
        self.emit_add_rdx_imm8(0x30);
        self.emit_dec_rsi();
        self.emit_mov_at_rsi_dl();
        self.emit_inc_rdi();

        self.emit_test_rax_rax();
        self.emit_jnz_rel32_back_to(loop_start);
        self.emit_cmp_rdi_imm8(scale as u8);
        self.emit_jle_rel32_back_to(loop_start);

        self.emit_mov_imm64_reloc(RDX, SCRATCH_END);
        self.emit_sub_rdx_rsi();

        self.emit_mov_imm64(RAX, SYS_WRITE);
        self.emit_mov_imm64(RDI, FD_STDOUT);
        self.emit_syscall();
    }

    fn emit_print_rax_int(&mut self) {
        self.emit_mov_imm64_reloc(RSI, SCRATCH_END);
        self.emit_mov_imm64(RCX, 10);

        let loop_start = self.code.len();
        self.emit_xor_rdx_rdx();
        self.emit_div_rcx();
        self.emit_add_rdx_imm8(0x30);
        self.emit_dec_rsi();
        self.emit_mov_at_rsi_dl();
        self.emit_test_rax_rax();
        self.emit_jnz_back_to(loop_start);

        self.emit_mov_imm64_reloc(RDX, SCRATCH_END);
        self.emit_sub_rdx_rsi();

        self.emit_mov_imm64(RAX, SYS_WRITE);
        self.emit_mov_imm64(RDI, FD_STDOUT);
        self.emit_syscall();
    }

    fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::IntLit(n) => {
                self.emit_mov_imm64(RAX, *n as u64);
            }
            Expr::DecLit { scaled, .. } => {
                self.emit_mov_imm64(RAX, *scaled as u64);
            }
            Expr::Ident(name) => {
                let offset = self.lookup_symbol(name).offset_in_data;
                self.emit_mov_imm64_reloc(RBX, offset);
                self.emit_mov_rax_from_rbx();
            }
            Expr::BinaryOp { op, left, right } => {
                self.emit_expr(left);
                self.emit_push_rax();
                self.emit_expr(right);
                self.emit_mov_rbx_rax();
                self.emit_pop_rax();
                match op {
                    BinOp::Add => self.emit_add_rax_rbx(),
                    BinOp::Sub => self.emit_sub_rax_rbx(),
                    BinOp::Mul => self.emit_imul_rax_rbx(),
                    BinOp::Div => {
                        self.emit_xor_rdx_rdx();
                        self.emit_div_rbx();
                    }
                    BinOp::Mod => {
                        self.emit_xor_rdx_rdx();
                        self.emit_div_rbx();
                        self.emit_mov_rax_rdx();
                    }
                }
            }
            Expr::Compare { op, left, right } => {
                self.emit_expr(left);
                self.emit_push_rax();
                self.emit_expr(right);
                self.emit_mov_rbx_rax();
                self.emit_pop_rax();
                self.emit_cmp_rax_rbx();
                self.emit_setcc_dl(*op);
                self.emit_movzx_eax_dl();
            }
            Expr::Not { inner } => {
                self.emit_expr(inner);
                self.emit_test_rax_rax();
                self.emit_setcc_dl_eq_zero();
                self.emit_movzx_eax_dl();
            }
            Expr::FieldAccess { base, field } => {
                let base_name = match base.as_ref() {
                    Expr::Ident(name) => name,
                    _ => panic!("codegen: only single-level field access supported"),
                };
                let sym = self.lookup_symbol(base_name);
                let base_offset = sym.offset_in_data;
                let record_name = match &sym.ty {
                    TypeExpr::Record(rname) => rname.clone(),
                    _ => panic!("codegen: `{}` is not a record", base_name),
                };
                let info = self.record_types.get(&record_name).unwrap();
                let f = info
                    .fields
                    .iter()
                    .find(|f| f.name == *field)
                    .unwrap_or_else(|| {
                        panic!("codegen: no field `{}` in record `{}`", field, record_name)
                    });
                let total_offset = base_offset + f.offset;
                self.emit_mov_imm64_reloc(RBX, total_offset);
                self.emit_mov_rax_from_rbx();
            }
            Expr::StringLit(_) => {
                panic!("codegen: string literals cannot appear in runtime expressions");
            }
        }
    }

    fn emit_exit(&mut self) {
        self.emit_mov_imm64(RAX, SYS_EXIT);
        self.emit_mov_imm64(RDI, 0);
        self.emit_syscall();
    }

    fn finalize(mut self) -> Vec<u8> {
        let code_size = self.code.len() as u64;
        for r in &self.relocs {
            let addr = ENTRY_VMA + code_size + r.data_offset;
            self.code[r.code_pos..r.code_pos + 8].copy_from_slice(&addr.to_le_bytes());
        }
        for (pos, name) in &self.par_calls {
            let target = self
                .paragraphs
                .iter()
                .find(|(n, _)| n == name)
                .and_then(|(_, addr)| *addr)
                .unwrap_or_else(|| panic!("codegen: paragraph `{}` not defined", name));
            let rel = target as i64 - (pos + 4) as i64;
            assert!(
                (i32::MIN as i64..=i32::MAX as i64).contains(&rel),
                "call displacement {} out of i32 range",
                rel
            );
            let bytes = (rel as i32).to_le_bytes();
            self.code[*pos..*pos + 4].copy_from_slice(&bytes);
        }
        let mut segment = self.code;
        segment.extend_from_slice(&self.data);
        segment
    }

    fn emit_mov_imm64(&mut self, reg: u8, imm: u64) {
        self.code.push(0x48);
        self.code.push(0xB8 + reg);
        self.code.extend_from_slice(&imm.to_le_bytes());
    }

    fn emit_mov_imm64_reloc(&mut self, reg: u8, data_offset: u64) {
        self.code.push(0x48);
        self.code.push(0xB8 + reg);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 8]);
        self.relocs.push(Reloc {
            code_pos: pos,
            data_offset,
        });
    }

    fn emit_syscall(&mut self) {
        self.code.extend_from_slice(&[0x0F, 0x05]);
    }

    fn emit_mov_rax_from_rbx(&mut self) {
        // mov rax, [rbx]
        self.code.extend_from_slice(&[0x48, 0x8B, 0x03]);
    }

    fn emit_mov_at_rbx_rax(&mut self) {
        // mov [rbx], rax
        self.code.extend_from_slice(&[0x48, 0x89, 0x03]);
    }

    fn emit_mov_rbx_rax(&mut self) {
        // mov rbx, rax
        self.code.extend_from_slice(&[0x48, 0x89, 0xC3]);
    }

    fn emit_mov_rax_rdx(&mut self) {
        // mov rax, rdx
        self.code.extend_from_slice(&[0x48, 0x89, 0xD0]);
    }

    fn emit_push_rax(&mut self) {
        // push rax
        self.code.push(0x50);
    }

    fn emit_pop_rax(&mut self) {
        // pop rax
        self.code.push(0x58);
    }

    fn emit_add_rax_rbx(&mut self) {
        // add rax, rbx
        self.code.extend_from_slice(&[0x48, 0x01, 0xD8]);
    }

    fn emit_sub_rax_rbx(&mut self) {
        // sub rax, rbx
        self.code.extend_from_slice(&[0x48, 0x29, 0xD8]);
    }

    fn emit_imul_rax_rbx(&mut self) {
        // imul rax, rbx
        self.code.extend_from_slice(&[0x48, 0x0F, 0xAF, 0xC3]);
    }

    fn emit_div_rbx(&mut self) {
        // div rbx (unsigned: rdx:rax / rbx)
        self.code.extend_from_slice(&[0x48, 0xF7, 0xF3]);
    }

    fn emit_xor_rdx_rdx(&mut self) {
        // xor rdx, rdx
        self.code.extend_from_slice(&[0x48, 0x31, 0xD2]);
    }

    fn emit_div_rcx(&mut self) {
        // div rcx nsigned: (rdx:rax) / rcx → quotient in rax, remainder in rdx
        self.code.extend_from_slice(&[0x48, 0xF7, 0xF1]);
    }

    fn emit_add_rdx_imm8(&mut self, imm: u8) {
        // add rdx, imm8 (sign-extended)
        self.code.extend_from_slice(&[0x48, 0x83, 0xC2, imm]);
    }

    fn emit_dec_rsi(&mut self) {
        // dec rsi
        self.code.extend_from_slice(&[0x48, 0xFF, 0xCE]);
    }

    fn emit_mov_at_rsi_dl(&mut self) {
        // mov [rsi], dl
        self.code.extend_from_slice(&[0x88, 0x16]);
    }

    fn emit_test_rax_rax(&mut self) {
        // test rax, rax
        self.code.extend_from_slice(&[0x48, 0x85, 0xC0]);
    }

    fn emit_sub_rdx_rsi(&mut self) {
        // sub rdx, rsi
        self.code.extend_from_slice(&[0x48, 0x29, 0xF2]);
    }

    fn emit_mov_rbx_from_rsp_off(&mut self, disp: i8) {
        // mov rbx, [rsp+disp8]
        self.code.extend_from_slice(&[0x48, 0x8B, 0x5C, 0x24, disp as u8]);
    }

    fn emit_cmp_rax_rbx(&mut self) {
        // cmp rax, rbx
        self.code.extend_from_slice(&[0x48, 0x39, 0xD8]);
    }

    fn emit_inc_rax(&mut self) {
        // inc rax
        self.code.extend_from_slice(&[0x48, 0xFF, 0xC0]);
    }

    fn emit_add_rsp_imm8(&mut self, imm: i8) {
        // add rsp, imm8 (sign-extended)
        self.code.extend_from_slice(&[0x48, 0x83, 0xC4, imm as u8]);
    }

    fn emit_jge_placeholder(&mut self) -> usize {
        // jge rel32: 0F 8D + 4-byte placeholder; returns position of rel32 field
        self.code.push(0x0F);
        self.code.push(0x8D);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        pos
    }

    fn emit_jne_placeholder(&mut self) -> usize {
        // jne rel32: 0F 85 + 4-byte placeholder
        self.code.push(0x0F);
        self.code.push(0x85);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        pos
    }

    fn emit_jnz_rel32_back_to(&mut self, target: usize) {
        // jnz rel32: 0F 85 + rel32
        self.code.push(0x0F);
        self.code.push(0x85);
        let pos = self.code.len();
        let rel = target as i64 - (pos + 4) as i64;
        assert!(
            (i32::MIN as i64..=i32::MAX as i64).contains(&rel),
            "jnz rel32 displacement {} out of range",
            rel
        );
        self.code.extend_from_slice(&(rel as i32).to_le_bytes());
    }

    fn emit_jle_rel32_back_to(&mut self, target: usize) {
        // jle rel32: 0F 8E + rel32
        self.code.push(0x0F);
        self.code.push(0x8E);
        let pos = self.code.len();
        let rel = target as i64 - (pos + 4) as i64;
        assert!(
            (i32::MIN as i64..=i32::MAX as i64).contains(&rel),
            "jle rel32 displacement {} out of range",
            rel
        );
        self.code.extend_from_slice(&(rel as i32).to_le_bytes());
    }

    fn emit_cmp_rdi_imm8(&mut self, imm: u8) {
        // cmp rdi, imm8 (sign-extended)
        self.code.extend_from_slice(&[0x48, 0x83, 0xFF, imm]);
    }

    fn emit_inc_rdi(&mut self) {
        // inc rdi
        self.code.extend_from_slice(&[0x48, 0xFF, 0xC7]);
    }

    fn emit_mov_byte_at_rsi_imm(&mut self, imm: u8) {
        // mov byte ptr [rsi], imm8
        self.code.extend_from_slice(&[0xC6, 0x06, imm]);
    }

    fn emit_jz_placeholder(&mut self) -> usize {
        // jz rel32: 0F 84 + 4-byte placeholder
        self.code.push(0x0F);
        self.code.push(0x84);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        pos
    }

    fn emit_setcc_dl(&mut self, op: CmpOp) {
        // setcc r/m8: 0F 9X + ModRM (mod=11 reg=000 rm=010 → dl) = C2
        let opcode = match op {
            CmpOp::Eq => 0x94,
            CmpOp::Ne => 0x95,
            CmpOp::Lt => 0x9C, // signed less
            CmpOp::Le => 0x9E,
            CmpOp::Gt => 0x9F,
            CmpOp::Ge => 0x9D,
        };
        self.code.extend_from_slice(&[0x0F, opcode, 0xC2]);
    }

    fn emit_setcc_dl_eq_zero(&mut self) {
        // sete dl — used to materialize "not x" after `test rax, rax`
        self.code.extend_from_slice(&[0x0F, 0x94, 0xC2]);
    }

    fn emit_movzx_eax_dl(&mut self) {
        // movzx eax, dl — zero-extend dl into rax (writing eax clears upper 32 bits)
        self.code.extend_from_slice(&[0x0F, 0xB6, 0xC2]);
    }

    fn emit_jmp_back_to(&mut self, target: usize) {
        // jmp rel32
        self.code.push(0xE9);
        let pos = self.code.len();
        let rel = target as i64 - (pos + 4) as i64;
        assert!(
            (i32::MIN as i64..=i32::MAX as i64).contains(&rel),
            "jmp rel32 displacement {} out of range",
            rel
        );
        self.code.extend_from_slice(&(rel as i32).to_le_bytes());
    }

    fn patch_rel32(&mut self, pos: usize, target: usize) {
        let rel = target as i64 - (pos + 4) as i64;
        assert!(
            (i32::MIN as i64..=i32::MAX as i64).contains(&rel),
            "rel32 displacement {} out of range",
            rel
        );
        self.code[pos..pos + 4].copy_from_slice(&(rel as i32).to_le_bytes());
    }

    fn emit_jnz_back_to(&mut self, target: usize) {
        // jnz rel8ndisplacement from RIP-after-jnz to target
        let after_jnz = self.code.len() + 2;
        let disp = target as i64 - after_jnz as i64;
        assert!(
            (-128..=127).contains(&disp),
            "jnz rel8 displacement {} out of range",
            disp
        );
        self.code.push(0x75);
        self.code.push((disp as i8) as u8);
    }
}

fn type_size(ty: &TypeExpr) -> u64 {
    match ty {
        TypeExpr::UInt(_) => 8,
        TypeExpr::Str(n) => *n as u64,
        TypeExpr::UDec(_, _) | TypeExpr::IDec(_, _) => 8,
        TypeExpr::Record(_) => panic!("codegen: nested record fields not supported yet"),
    }
}

enum ConstValue {
    Str(String),
    Int(i64),
}

impl ConstValue {
    fn expect_int(self) -> i64 {
        match self {
            ConstValue::Int(n) => n,
            _ => panic!("codegen: expected integer constant"),
        }
    }
}

fn eval_const(expr: &Expr) -> ConstValue {
    try_eval_const(expr).unwrap_or_else(|| panic!("codegen: expression is not a constant"))
}

fn decimal_init_value(init: &Expr, declared_m: u32) -> i64 {
    match init {
        Expr::IntLit(v) => {
            let scale_factor = 10i64.checked_pow(declared_m).expect("scale too large");
            v.checked_mul(scale_factor)
                .expect("integer literal overflows declared decimal range")
        }
        Expr::DecLit { scaled, scale } => {
            if *scale != declared_m {
                panic!(
                    "codegen: decimal literal scale {} doesn't match declared scale {}",
                    scale, declared_m
                );
            }
            *scaled
        }
        _ => panic!("codegen: decimal initializer must be a literal"),
    }
}

fn try_eval_const(expr: &Expr) -> Option<ConstValue> {
    match expr {
        Expr::StringLit(s) => Some(ConstValue::Str(s.clone())),
        Expr::IntLit(n) => Some(ConstValue::Int(*n)),
        Expr::DecLit { .. }
        | Expr::Ident(_)
        | Expr::Compare { .. }
        | Expr::Not { .. }
        | Expr::FieldAccess { .. } => None,
        Expr::BinaryOp { op, left, right } => {
            let l = match try_eval_const(left)? {
                ConstValue::Int(n) => n,
                _ => return None,
            };
            let r = match try_eval_const(right)? {
                ConstValue::Int(n) => n,
                _ => return None,
            };
            let v = match op {
                BinOp::Add => l + r,
                BinOp::Sub => l - r,
                BinOp::Mul => l * r,
                BinOp::Div => {
                    if r == 0 {
                        panic!("codegen: divide by zero");
                    }
                    l / r
                }
                BinOp::Mod => {
                    if r == 0 {
                        panic!("codegen: modulo by zero");
                    }
                    l % r
                }
            };
            Some(ConstValue::Int(v))
        }
    }
}
