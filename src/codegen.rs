use crate::ast::{BinOp, Expr, Program, Stmt};
use crate::elf::ENTRY_VMA;

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
    g.collect_symbols(program);
    g.register_paragraphs(program);

    for stmt in &program.statements {
        if !matches!(stmt, Stmt::Par { .. }) {
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
}

struct Reloc {
    code_pos: usize,
    data_offset: u64,
}

struct Codegen {
    code: Vec<u8>,
    data: Vec<u8>,
    symbols: Vec<Symbol>,
    relocs: Vec<Reloc>,
    paragraphs: Vec<(String, Option<usize>)>,
    par_calls: Vec<(usize, String)>,
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
            if let Stmt::Let { name, init, .. } = stmt {
                let v = eval_const(init).expect_int();
                let offset = self.data.len() as u64;
                self.data.extend_from_slice(&(v as u64).to_le_bytes());
                self.symbols.push(Symbol {
                    name: name.clone(),
                    offset_in_data: offset,
                });
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
            Stmt::Let { .. } | Stmt::Par { .. } => {}
            Stmt::Assign { name, value } => self.emit_assign(name, value),
            Stmt::For {
                var, start, end, body,
            } => self.emit_for(var, start, end, body),
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

    fn emit_for(&mut self, var: &str, start: &Expr, end: &Expr, body: &[Stmt]) {
        if self.symbols.iter().any(|s| s.name == var) {
            panic!("codegen: loop variable `{}` shadows existing symbol", var);
        }
        let var_offset = self.data.len() as u64;
        self.data.extend_from_slice(&[0u8; 8]);
        self.symbols.push(Symbol {
            name: var.to_string(),
            offset_in_data: var_offset,
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

    fn emit_assign(&mut self, name: &str, value: &Expr) {
        self.emit_expr(value);
        let offset = self.lookup_symbol(name).offset_in_data;
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
        } else {
            self.emit_expr(arg);
            self.emit_print_rax_int();
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

fn try_eval_const(expr: &Expr) -> Option<ConstValue> {
    match expr {
        Expr::StringLit(s) => Some(ConstValue::Str(s.clone())),
        Expr::IntLit(n) => Some(ConstValue::Int(*n)),
        Expr::Ident(_) => None,
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
