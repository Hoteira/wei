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
    for stmt in &program.statements {
        g.emit_stmt(stmt);
    }
    g.emit_exit();
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
        }
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
            Stmt::Let { .. } => {}
            Stmt::Call { name, args } if name == "print" => {
                if args.len() != 1 {
                    panic!("codegen: print() takes exactly one argument");
                }
                self.emit_print(&args[0]);
            }
            Stmt::Call { name, .. } => {
                panic!("codegen: unknown function `{}`", name);
            }
        }
    }

    fn emit_print(&mut self, arg: &Expr) {
        match arg {
            Expr::Ident(name) => {
                let offset = self.lookup_symbol(name).offset_in_data;
                self.emit_print_var(offset);
            }
            _ => {
                let s = match eval_const(arg) {
                    ConstValue::Str(s) => s,
                    ConstValue::Int(n) => n.to_string(),
                };
                self.emit_print_const(s);
            }
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

    fn emit_print_var(&mut self, var_offset: u64) {
        self.emit_mov_imm64_reloc(RBX, var_offset);
        self.emit_mov_rax_from_rbx();

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
    match expr {
        Expr::StringLit(s) => ConstValue::Str(s.clone()),
        Expr::IntLit(n) => ConstValue::Int(*n),
        Expr::Ident(name) => {
            panic!("codegen: `{}` is not a constant expression", name)
        }
        Expr::BinaryOp { op, left, right } => {
            let l = eval_const(left).expect_int();
            let r = eval_const(right).expect_int();
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
            ConstValue::Int(v)
        }
    }
}
