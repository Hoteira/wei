use crate::ast::{Expr, Program, Stmt};
use crate::elf::ENTRY_VMA;

// x86-64 register encodings used by the mov-imm64 opcode (0xB8 + reg).
const RAX: u8 = 0;
const RDX: u8 = 2;
const RSI: u8 = 6;
const RDI: u8 = 7;

// Linux x86-64 syscall numbers
const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;
const FD_STDOUT: u64 = 1;

pub fn emit(program: &Program) -> Vec<u8> {
    let to_print: String = match program.statements.as_slice() {
        [Stmt::Call { name, args }] if name == "print" => match args.as_slice() {
            [Expr::StringLit(s)] => s.clone(),
            [Expr::IntLit(n)] => n.to_string(),
            _ => panic!("codegen: print() expects one string or integer argument"),
        },
        _ => panic!("codegen: only one `print(...)` statement supported"),
    };

    let str_bytes = to_print.into_bytes();
    let str_len = str_bytes.len() as u64;

    let mut code: Vec<u8> = Vec::new();

    emit_mov_imm64(&mut code, RAX, SYS_WRITE);
    emit_mov_imm64(&mut code, RDI, FD_STDOUT);

    let rsi_imm_offset = code.len() + 2;
    emit_mov_imm64(&mut code, RSI, 0);

    emit_mov_imm64(&mut code, RDX, str_len);
    emit_syscall(&mut code);

    // sys_exit(0)
    emit_mov_imm64(&mut code, RAX, SYS_EXIT);
    emit_mov_imm64(&mut code, RDI, 0);
    emit_syscall(&mut code);

    let code_size = code.len() as u64;
    let str_addr = ENTRY_VMA + code_size;

    let addr_bytes = str_addr.to_le_bytes();
    code[rsi_imm_offset..rsi_imm_offset + 8].copy_from_slice(&addr_bytes);

    let mut segment = code;
    segment.extend_from_slice(&str_bytes);
    segment
}

fn emit_mov_imm64(out: &mut Vec<u8>, reg: u8, imm: u64) {
    out.push(0x48);
    out.push(0xB8 + reg);
    out.extend_from_slice(&imm.to_le_bytes());
}

fn emit_syscall(out: &mut Vec<u8>) {
    out.push(0x0F);
    out.push(0x05);
}
