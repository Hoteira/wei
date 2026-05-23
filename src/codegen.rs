use crate::ast::{BinOp, CmpOp, Expr, LValue, MatchArm, Pattern, Program, Stmt, TypeExpr};
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
    g.collect_files(program);
    g.register_paragraphs(program);
    g.register_subs(program);
    g.register_eighty_eights(program);

    for stmt in &program.statements {
        if !matches!(
            stmt,
            Stmt::Par { .. } | Stmt::Sub { .. } | Stmt::TypeDef { .. } | Stmt::FileDecl { .. }
        ) {
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
        if let Stmt::Sub { name, body, .. } = stmt {
            g.start_paragraph(name);
            for s in body {
                g.emit_stmt(s);
            }
            g.emit_ret();
        }
    }

    g.finalize()
}

#[derive(Clone)]
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
    subs: Vec<(String, Vec<(String, TypeExpr)>)>,
    eighty_eights: HashMap<String, (String, EightyEightValue)>,
    file_keys: HashMap<String, String>,
}

#[derive(Clone)]
enum EightyEightValue {
    Int(i64),
    Str(String),
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
            subs: Vec::new(),
            eighty_eights: HashMap::new(),
            file_keys: HashMap::new(),
        }
    }

    fn register_eighty_eights(&mut self, program: &Program) {
        for stmt in &program.statements {
            self.collect_eighty_eights_in(stmt);
        }
    }

    fn collect_eighty_eights_in(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                eighty_eights,
                ..
            } => {
                for (n88, v) in eighty_eights {
                    let value = match v {
                        Expr::IntLit(n) => EightyEightValue::Int(*n),
                        Expr::StringLit(s) => EightyEightValue::Str(s.clone()),
                        _ => panic!(
                            "codegen: 88-level `{}`: value must be int or string literal",
                            n88
                        ),
                    };
                    self.eighty_eights
                        .insert(n88.clone(), (name.clone(), value));
                }
            }
            Stmt::Par { body, .. } | Stmt::Sub { body, .. } => {
                for s in body {
                    self.collect_eighty_eights_in(s);
                }
            }
            _ => {}
        }
    }

    fn register_subs(&mut self, program: &Program) {
        for stmt in &program.statements {
            if let Stmt::Sub { name, params, body: _ } = stmt {
                if self.paragraphs.iter().any(|(n, _)| n == name) {
                    panic!("codegen: sub `{}` conflicts with paragraph or sub of same name", name);
                }
                self.paragraphs.push((name.clone(), None));
                self.subs.push((name.clone(), params.clone()));
                for (pname, pty) in params {
                    let size = match pty {
                        TypeExpr::UInt(_) | TypeExpr::UDec(_, _) | TypeExpr::IDec(_, _) => 8usize,
                        TypeExpr::Str(n) => *n as usize,
                        TypeExpr::Record(rname) => {
                            self.record_types
                                .get(rname)
                                .map(|r| r.size as usize)
                                .unwrap_or_else(|| {
                                    panic!(
                                        "codegen: sub param record `{}` not defined",
                                        rname
                                    )
                                })
                        }
                        other => panic!("codegen: sub param type {:?} not supported", other),
                    };
                    if self.symbols.iter().any(|s| s.name == *pname) {
                        panic!("codegen: sub param `{}` conflicts with existing symbol", pname);
                    }
                    let offset = self.data.len() as u64;
                    self.data.extend_from_slice(&vec![0u8; size]);
                    self.symbols.push(Symbol {
                        name: pname.clone(),
                        offset_in_data: offset,
                        ty: pty.clone(),
                    });
                }
            }
        }
    }

    fn is_sub(&self, name: &str) -> bool {
        self.subs.iter().any(|(n, _)| n == name)
    }

    fn emit_call_sub(&mut self, name: &str, args: &[Expr]) {
        let params: Vec<(String, TypeExpr)> = self
            .subs
            .iter()
            .find(|(n, _)| n == name)
            .unwrap()
            .1
            .clone();
        if args.len() != params.len() {
            panic!(
                "codegen: sub `{}` expects {} args, got {}",
                name,
                params.len(),
                args.len()
            );
        }
        // Copy-in
        for (i, arg) in args.iter().enumerate() {
            let (pname, pty) = &params[i];
            let param_slot = self.lookup_symbol(pname).offset_in_data;
            match pty {
                TypeExpr::UInt(_) | TypeExpr::UDec(_, _) | TypeExpr::IDec(_, _) => {
                    self.emit_expr(arg);
                    self.emit_mov_imm64_reloc(RBX, param_slot);
                    self.emit_mov_at_rbx_rax();
                }
                TypeExpr::Str(_) | TypeExpr::Record(_) => {
                    let size = self.type_size_dyn(pty);
                    let (arg_off, _) = self.resolve_expr_address(arg);
                    self.emit_mov_imm64_reloc(RSI, arg_off);
                    self.emit_mov_imm64_reloc(RDI, param_slot);
                    self.emit_mov_imm64(RCX, size);
                    self.code.push(0xFC);
                    self.code.extend_from_slice(&[0xF3, 0xA4]);
                }
                other => panic!("codegen: sub param type {:?} not supported", other),
            }
        }
        self.emit_call_paragraph(name);
        // Copy-out (for lvalue args)
        for (i, arg) in args.iter().enumerate() {
            if !matches!(arg, Expr::Ident(_) | Expr::FieldAccess { .. }) {
                continue;
            }
            let (pname, pty) = &params[i];
            let param_slot = self.lookup_symbol(pname).offset_in_data;
            let (arg_off, _) = self.resolve_expr_address(arg);
            match pty {
                TypeExpr::UInt(_) | TypeExpr::UDec(_, _) | TypeExpr::IDec(_, _) => {
                    self.emit_mov_imm64_reloc(RBX, param_slot);
                    self.emit_mov_rax_from_rbx();
                    self.emit_mov_imm64_reloc(RBX, arg_off);
                    self.emit_mov_at_rbx_rax();
                }
                TypeExpr::Str(_) | TypeExpr::Record(_) => {
                    let size = self.type_size_dyn(pty);
                    self.emit_mov_imm64_reloc(RSI, param_slot);
                    self.emit_mov_imm64_reloc(RDI, arg_off);
                    self.emit_mov_imm64(RCX, size);
                    self.code.push(0xFC);
                    self.code.extend_from_slice(&[0xF3, 0xA4]);
                }
                _ => {}
            }
        }
    }

    fn register_typedefs(&mut self, program: &Program) {
        // Pass 1: typedefs without redefines (so other typedefs they reference are defined first).
        // We do all typedefs in source order; redefines only references other fields in same type
        // (and possibly other typedefs, which must precede textually).
        for stmt in &program.statements {
            if let Stmt::TypeDef {
                name,
                fields,
                redefines,
            } = stmt
            {
                let mut cursor = 0u64;
                let mut field_list: Vec<RecordField> = Vec::new();
                for (fname, ty) in fields {
                    let field_size = self.type_size_dyn(ty);
                    let off = if let Some((_, other)) =
                        redefines.iter().find(|(f, _)| f == fname)
                    {
                        let other_off = field_list
                            .iter()
                            .find(|f| f.name == *other)
                            .map(|f| f.offset)
                            .unwrap_or_else(|| {
                                panic!(
                                    "codegen: redefines target `{}` not found in record `{}`",
                                    other, name
                                )
                            });
                        other_off
                    } else {
                        let off = cursor;
                        cursor += field_size;
                        off
                    };
                    field_list.push(RecordField {
                        name: fname.clone(),
                        offset: off,
                        ty: ty.clone(),
                    });
                }
                self.record_types.insert(
                    name.clone(),
                    RecordInfo {
                        fields: field_list,
                        size: cursor,
                    },
                );
            }
        }
    }

    fn type_size_dyn(&self, ty: &TypeExpr) -> u64 {
        match ty {
            TypeExpr::Record(rname) => self
                .record_types
                .get(rname)
                .map(|r| r.size)
                .unwrap_or_else(|| panic!("codegen: unknown record `{}`", rname)),
            TypeExpr::Array { element, length } => {
                self.type_size_dyn(element) * (*length as u64)
            }
            other => type_size(other),
        }
    }

    fn collect_files(&mut self, program: &Program) {
        for stmt in &program.statements {
            if let Stmt::FileDecl {
                name,
                path,
                mode,
                key,
            } = stmt
            {
                let offset = self.data.len() as u64;
                // 8 bytes fd, 8 bytes eof_flag, then null-terminated path
                self.data.extend_from_slice(&[0u8; 16]);
                self.data.extend_from_slice(path.as_bytes());
                self.data.push(0);
                self.symbols.push(Symbol {
                    name: name.clone(),
                    offset_in_data: offset,
                    ty: TypeExpr::File,
                });
                if mode == "indexed" {
                    let k = key.clone().unwrap_or_else(|| {
                        panic!("codegen: indexed file `{}` requires `key fieldname`", name)
                    });
                    self.file_keys.insert(name.clone(), k);
                }
            }
        }
    }

    fn file_fd_offset(&self, name: &str) -> u64 {
        let sym = self.lookup_symbol(name);
        if !matches!(sym.ty, TypeExpr::File) {
            panic!("codegen: `{}` is not a file", name);
        }
        sym.offset_in_data
    }

    fn file_eof_offset(&self, name: &str) -> u64 {
        self.file_fd_offset(name) + 8
    }

    fn file_path_offset(&self, name: &str) -> u64 {
        self.file_fd_offset(name) + 16
    }

    fn record_size_of(&self, name: &str) -> u64 {
        let sym = self.lookup_symbol(name);
        match &sym.ty {
            TypeExpr::Record(rname) => self.record_types[rname].size,
            other => panic!("codegen: `{}` is not a record (type: {:?})", name, other),
        }
    }

    fn emit_file_open(&mut self, args: &[Expr]) {
        if args.len() != 2 {
            panic!("codegen: open() takes 2 args");
        }
        let file_name = match &args[0] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: open() first arg must be a file ident"),
        };
        let mode_name = match &args[1] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: open() second arg must be a mode keyword"),
        };
        let flags: u64 = match mode_name.as_str() {
            "input" => 0,    // O_RDONLY
            "output" => 577, // O_WRONLY | O_CREAT | O_TRUNC = 1 | 64 | 512
            "i_o" => 2,      // O_RDWR
            other => panic!("codegen: unknown file mode `{}`", other),
        };
        let path_off = self.file_path_offset(&file_name);
        let fd_off = self.file_fd_offset(&file_name);

        self.emit_mov_imm64(RAX, 2); // sys_open
        self.emit_mov_imm64_reloc(RDI, path_off);
        self.emit_mov_imm64(RSI, flags);
        self.emit_mov_imm64(RDX, 0o644);
        self.emit_syscall();
        // store fd
        self.emit_mov_imm64_reloc(RBX, fd_off);
        self.emit_mov_at_rbx_rax();
    }

    fn emit_file_close(&mut self, args: &[Expr]) {
        if args.len() != 1 {
            panic!("codegen: close() takes 1 arg");
        }
        let file_name = match &args[0] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: close() arg must be a file ident"),
        };
        let fd_off = self.file_fd_offset(&file_name);
        self.emit_mov_imm64_reloc(RBX, fd_off);
        self.emit_mov_rdi_from_rbx();
        self.emit_mov_imm64(RAX, 3); // sys_close
        self.emit_syscall();
    }

    fn emit_file_rewrite(&mut self, args: &[Expr]) {
        if args.len() != 2 {
            panic!("codegen: rewrite() takes 2 args");
        }
        let file_name = match &args[0] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: rewrite() first arg must be a file ident"),
        };
        let rec_name = match &args[1] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: rewrite() second arg must be a record ident"),
        };
        let fd_off = self.file_fd_offset(&file_name);
        let rec_offset = self.lookup_symbol(&rec_name).offset_in_data;
        let rec_size = self.record_size_of(&rec_name);

        // sys_lseek(fd, -rec_size, SEEK_CUR)
        self.emit_mov_imm64_reloc(RBX, fd_off);
        self.emit_mov_rdi_from_rbx();
        self.emit_mov_imm64(RAX, 8); // sys_lseek
        self.emit_mov_imm64(RSI, rec_size);
        // neg rsi (0x48 0xF7 0xDE)
        self.code.extend_from_slice(&[0x48, 0xF7, 0xDE]);
        self.emit_mov_imm64(RDX, 1); // SEEK_CUR
        self.emit_syscall();

        // sys_write(fd, buf, count)
        self.emit_mov_imm64_reloc(RBX, fd_off);
        self.emit_mov_rdi_from_rbx();
        self.emit_mov_imm64(RAX, 1);
        self.emit_mov_imm64_reloc(RSI, rec_offset);
        self.emit_mov_imm64(RDX, rec_size);
        self.emit_syscall();
    }

    fn emit_file_write(&mut self, args: &[Expr]) {
        if args.len() != 2 {
            panic!("codegen: write() takes 2 args");
        }
        let file_name = match &args[0] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: write() first arg must be a file ident"),
        };
        let rec_name = match &args[1] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: write() second arg must be a record ident"),
        };
        let fd_off = self.file_fd_offset(&file_name);
        let rec_offset = self.lookup_symbol(&rec_name).offset_in_data;
        let rec_size = self.record_size_of(&rec_name);
        self.emit_mov_imm64_reloc(RBX, fd_off);
        self.emit_mov_rdi_from_rbx();
        self.emit_mov_imm64(RAX, 1); // sys_write
        self.emit_mov_imm64_reloc(RSI, rec_offset);
        self.emit_mov_imm64(RDX, rec_size);
        self.emit_syscall();
    }

    fn emit_file_read(&mut self, args: &[Expr]) {
        if args.len() != 2 {
            panic!("codegen: read() takes 2 args");
        }
        let file_name = match &args[0] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: read() first arg must be a file ident"),
        };
        let rec_name = match &args[1] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: read() second arg must be a record ident"),
        };
        if let Some(key_field) = self.file_keys.get(&file_name).cloned() {
            self.emit_indexed_read(&file_name, &rec_name, &key_field);
        } else {
            self.emit_sequential_read(&file_name, &rec_name);
        }
    }

    fn emit_sequential_read(&mut self, file_name: &str, rec_name: &str) {
        let fd_off = self.file_fd_offset(file_name);
        let eof_off = self.file_eof_offset(file_name);
        let rec_offset = self.lookup_symbol(rec_name).offset_in_data;
        let rec_size = self.record_size_of(rec_name);

        self.emit_mov_imm64_reloc(RBX, fd_off);
        self.emit_mov_rdi_from_rbx();
        self.emit_mov_imm64(RAX, 0);
        self.emit_mov_imm64_reloc(RSI, rec_offset);
        self.emit_mov_imm64(RDX, rec_size);
        self.emit_syscall();

        self.emit_test_rax_rax();
        let skip_eof = self.emit_jne_placeholder();
        self.emit_mov_imm64_reloc(RBX, eof_off);
        self.emit_mov_imm64(RAX, 1);
        self.emit_mov_at_rbx_rax();
        let after = self.code.len();
        self.patch_rel32(skip_eof, after);
    }

    fn emit_indexed_read(&mut self, file_name: &str, rec_name: &str, key_field: &str) {
        let fd_off = self.file_fd_offset(file_name);
        let eof_off = self.file_eof_offset(file_name);
        let rec_sym = self.lookup_symbol(rec_name).clone();
        let rec_offset = rec_sym.offset_in_data;
        let rec_size = self.record_size_of(rec_name);
        let rname = match &rec_sym.ty {
            TypeExpr::Record(n) => n.clone(),
            _ => panic!("codegen: indexed read rec must be Record-typed"),
        };
        let info = self.record_types.get(&rname).unwrap();
        let key_f = info
            .fields
            .iter()
            .find(|f| f.name == key_field)
            .unwrap_or_else(|| {
                panic!(
                    "codegen: indexed key field `{}` not in record `{}`",
                    key_field, rname
                )
            });
        let key_off_in_rec = key_f.offset;
        let key_width = match &key_f.ty {
            TypeExpr::Str(w) => *w,
            _ => panic!("codegen: indexed key field must be str(N)"),
        };
        let key_off = rec_offset + key_off_in_rec;
        let scratch_off: u64 = 0; // use SCRATCH start

        // Stash rec key into scratch
        self.emit_mov_imm64_reloc(RSI, key_off);
        self.emit_mov_imm64_reloc(RDI, scratch_off);
        self.emit_mov_imm64(RCX, key_width as u64);
        self.code.push(0xFC); // cld
        self.code.extend_from_slice(&[0xF3, 0xA4]); // rep movsb

        // Loop start
        let loop_start = self.code.len();

        // Read rec_size bytes from fd into rec buffer
        self.emit_mov_imm64_reloc(RBX, fd_off);
        self.emit_mov_rdi_from_rbx();
        self.emit_mov_imm64(RAX, 0);
        self.emit_mov_imm64_reloc(RSI, rec_offset);
        self.emit_mov_imm64(RDX, rec_size);
        self.emit_syscall();

        // If rax == 0, EOF — set eof flag and jump to end
        self.emit_test_rax_rax();
        let skip_eof_set = self.emit_jne_placeholder();
        self.emit_mov_imm64_reloc(RBX, eof_off);
        self.emit_mov_imm64(RAX, 1);
        self.emit_mov_at_rbx_rax();
        let exit_jump = self.emit_jmp_placeholder();
        let after_eof_check = self.code.len();
        self.patch_rel32(skip_eof_set, after_eof_check);

        // Compare rec's key field to scratch
        self.emit_mov_imm64_reloc(RSI, key_off);
        self.emit_mov_imm64_reloc(RDI, scratch_off);
        self.emit_mov_imm64(RCX, key_width as u64);
        self.code.push(0xFC); // cld
        self.code.extend_from_slice(&[0xF3, 0xA6]); // repe cmpsb

        // jnz back to loop_start (if not equal, keep searching)
        self.emit_jnz_rel32_back_to(loop_start);

        // Found - end
        let end = self.code.len();
        self.patch_rel32(exit_jump, end);
    }

    fn emit_string_concat(&mut self, args: &[Expr]) {
        if args.len() < 2 {
            panic!("codegen: string_concat() needs at least 2 args (target, src1, ...)");
        }
        let (target_off, target_width) = {
            let (off, ty) = self.resolve_expr_address(&args[0]);
            match ty {
                TypeExpr::Str(w) => (off, w),
                _ => panic!("codegen: string_concat target must be str(N)"),
            }
        };
        let mut cursor: u32 = 0;
        for src in &args[1..] {
            let (src_off, src_len) = match src {
                Expr::Ident(_) | Expr::FieldAccess { .. } => {
                    let (off, ty) = self.resolve_expr_address(src);
                    let w = match ty {
                        TypeExpr::Str(w) => w,
                        _ => panic!("codegen: string_concat src must be str-typed or literal"),
                    };
                    (off, w)
                }
                Expr::StringLit(s) => {
                    let bytes = s.as_bytes().to_vec();
                    let off = self.data.len() as u64;
                    let len = bytes.len() as u32;
                    self.data.extend(bytes);
                    (off, len)
                }
                _ => panic!("codegen: string_concat src must be ident or string literal"),
            };
            if cursor + src_len > target_width {
                panic!(
                    "codegen: string_concat sources ({} so far + {}) exceed target str({})",
                    cursor, src_len, target_width
                );
            }
            self.emit_mov_imm64_reloc(RSI, src_off);
            self.emit_mov_imm64_reloc(RDI, target_off + cursor as u64);
            self.emit_mov_imm64(RCX, src_len as u64);
            self.code.push(0xFC);
            self.code.extend_from_slice(&[0xF3, 0xA4]);
            cursor += src_len;
        }
        if cursor < target_width {
            let pad = target_width - cursor;
            self.emit_mov_imm64_reloc(RDI, target_off + cursor as u64);
            self.emit_mov_imm64(RCX, pad as u64);
            self.emit_mov_imm64(RAX, b' ' as u64);
            self.code.push(0xFC);
            // rep stosb (F3 AA)
            self.code.extend_from_slice(&[0xF3, 0xAA]);
        }
    }

    fn emit_unstring(&mut self, args: &[Expr]) {
        if args.len() < 3 {
            panic!("codegen: unstring() needs at least 3 args (src, delim, target1, ...)");
        }
        let (src_off, src_width) = {
            let (off, ty) = self.resolve_expr_address(&args[0]);
            match ty {
                TypeExpr::Str(w) => (off, w),
                _ => panic!("codegen: unstring src must be str(N)"),
            }
        };
        let delim = match &args[1] {
            Expr::StringLit(s) if s.len() == 1 => s.as_bytes()[0],
            _ => panic!("codegen: unstring delim must be single-char string literal"),
        };

        // Use RBX as src cursor offset (we'll initialize via lea-style)
        // Strategy: RSI = src pointer (advancing), saved between targets.
        // RDI = target pointer (per target).
        // For each target, scan: copy until delim or src_end or target full.
        // src_end = src_off + src_width.

        // Initialize RSI to src start, store src_end as immediate per check.
        self.emit_mov_imm64_reloc(RSI, src_off);
        // Use R8 for src_end? No, we don't have R8 defined. Just compute at the check.
        // Actually we use RBX for src_end.
        self.emit_mov_imm64_reloc(RBX, src_off + src_width as u64);

        for target in &args[2..] {
            let (tgt_off, tgt_width) = {
                let (off, ty) = self.resolve_expr_address(target);
                match ty {
                    TypeExpr::Str(w) => (off, w),
                    _ => panic!("codegen: unstring target must be str(N)"),
                }
            };
            self.emit_mov_imm64_reloc(RDI, tgt_off);
            self.emit_mov_imm64(RCX, tgt_width as u64);

            // Copy loop:
            //   if rsi >= rbx (src_end): pad rest of target with spaces, done.
            //   if [rsi] == delim: inc rsi (skip delim), pad rest of target with spaces, done.
            //   if rcx == 0: target full; advance rsi past next delim or end; done.
            //   else: copy byte; inc rsi; inc rdi; dec rcx; loop.

            let loop_start = self.code.len();
            // cmp rsi, rbx
            self.code.extend_from_slice(&[0x48, 0x39, 0xDE]); // cmp rsi, rbx
            let jae_eof = self.emit_jae_placeholder();
            // cmp byte [rsi], delim
            self.code.extend_from_slice(&[0x80, 0x3E, delim]);
            let je_at_delim = self.emit_je_placeholder();
            // test rcx, rcx (if target full)
            self.code.extend_from_slice(&[0x48, 0x85, 0xC9]);
            let jz_full = self.emit_jz_placeholder();
            // copy byte: mov al, [rsi]; mov [rdi], al
            self.code.extend_from_slice(&[0x8A, 0x06]); // mov al, [rsi]
            self.code.extend_from_slice(&[0x88, 0x07]); // mov [rdi], al
            // inc rsi, inc rdi, dec rcx
            self.code.extend_from_slice(&[0x48, 0xFF, 0xC6]);
            self.code.extend_from_slice(&[0x48, 0xFF, 0xC7]);
            self.code.extend_from_slice(&[0x48, 0xFF, 0xC9]);
            self.emit_jmp_back_to(loop_start);

            // at_delim: skip delim
            let at_delim = self.code.len();
            self.patch_rel32(je_at_delim, at_delim);
            self.code.extend_from_slice(&[0x48, 0xFF, 0xC6]); // inc rsi
            let after_delim_skip = self.emit_jmp_placeholder();

            // jz_full: advance rsi past next delim or end (for next target)
            let target_full = self.code.len();
            self.patch_rel32(jz_full, target_full);
            // scan rsi until [rsi] == delim or rsi == rbx
            let scan_start = self.code.len();
            self.code.extend_from_slice(&[0x48, 0x39, 0xDE]); // cmp rsi, rbx
            let jae_scan_end = self.emit_jae_placeholder();
            self.code.extend_from_slice(&[0x80, 0x3E, delim]);
            let je_found = self.emit_je_placeholder();
            self.code.extend_from_slice(&[0x48, 0xFF, 0xC6]); // inc rsi
            self.emit_jmp_back_to(scan_start);
            let scan_done = self.code.len();
            self.patch_rel32(je_found, scan_done);
            self.code.extend_from_slice(&[0x48, 0xFF, 0xC6]); // inc rsi (skip delim)
            self.patch_rel32(jae_scan_end, self.code.len());
            let after_target_full = self.emit_jmp_placeholder();

            // jae_eof: out of src, pad target rest with spaces
            let eof_label = self.code.len();
            self.patch_rel32(jae_eof, eof_label);
            // RCX is bytes remaining in target, RDI is current write pos
            // mov al, ' '; rep stosb
            self.emit_mov_imm64(RAX, b' ' as u64);
            self.code.push(0xFC);
            self.code.extend_from_slice(&[0xF3, 0xAA]);
            // Make src effectively at end (rsi = rbx) so subsequent targets get padded too.
            self.code.extend_from_slice(&[0x48, 0x89, 0xDE]); // mov rsi, rbx
            let after_eof = self.emit_jmp_placeholder();

            // Common pad block (after delim or at EOF): pad rest of target with spaces.
            let pad_block = self.code.len();
            self.patch_rel32(after_delim_skip, pad_block);
            self.patch_rel32(after_eof, pad_block);
            // RCX still has remaining bytes for this target.
            self.emit_mov_imm64(RAX, b' ' as u64);
            self.code.push(0xFC);
            self.code.extend_from_slice(&[0xF3, 0xAA]);

            // Common "next target" point.
            let next_target = self.code.len();
            self.patch_rel32(after_target_full, next_target);
        }
    }

    fn emit_je_placeholder(&mut self) -> usize {
        // je rel32: 0F 84 + 4-byte placeholder (same as jz)
        self.code.push(0x0F);
        self.code.push(0x84);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        pos
    }

    fn emit_jae_placeholder(&mut self) -> usize {
        // jae rel32 (unsigned >=): 0F 83 + 4-byte placeholder
        self.code.push(0x0F);
        self.code.push(0x83);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        pos
    }

    fn emit_replace_chars(&mut self, args: &[Expr]) {
        if args.len() != 3 {
            panic!("codegen: replace_chars() takes 3 args: str, needle, replacement");
        }
        let (str_off, str_width) = {
            let (off, ty) = self.resolve_expr_address(&args[0]);
            match ty {
                TypeExpr::Str(w) => (off, w),
                _ => panic!("codegen: replace_chars first arg must be a str-typed variable"),
            }
        };
        let needle = match &args[1] {
            Expr::StringLit(s) if s.len() == 1 => s.as_bytes()[0],
            _ => panic!("codegen: replace_chars needle must be single-char string literal"),
        };
        let replacement = match &args[2] {
            Expr::StringLit(s) if s.len() == 1 => s.as_bytes()[0],
            _ => panic!("codegen: replace_chars replacement must be single-char string literal"),
        };
        self.emit_mov_imm64_reloc(RSI, str_off);
        self.emit_mov_imm64(RCX, str_width as u64);

        let loop_start = self.code.len();
        // cmp byte [rsi], needle
        self.code.extend_from_slice(&[0x80, 0x3E, needle]);
        let jne_skip = self.emit_jne_placeholder();
        // mov byte [rsi], replacement
        self.code.extend_from_slice(&[0xC6, 0x06, replacement]);
        let skip = self.code.len();
        self.patch_rel32(jne_skip, skip);
        // inc rsi
        self.code.extend_from_slice(&[0x48, 0xFF, 0xC6]);
        // dec rcx
        self.code.extend_from_slice(&[0x48, 0xFF, 0xC9]);
        self.emit_jnz_rel32_back_to(loop_start);
    }

    fn emit_count_chars(&mut self, args: &[Expr]) {
        if args.len() != 2 {
            panic!("codegen: count_chars() takes 2 args");
        }
        let (str_off, str_width) = {
            let (off, ty) = self.resolve_expr_address(&args[0]);
            match ty {
                TypeExpr::Str(w) => (off, w),
                _ => panic!("codegen: count_chars first arg must be a str-typed variable"),
            }
        };
        let needle = match &args[1] {
            Expr::StringLit(s) if s.len() == 1 => s.as_bytes()[0],
            _ => panic!("codegen: count_chars second arg must be a single-char string literal"),
        };

        self.emit_mov_imm64_reloc(RSI, str_off);
        self.emit_mov_imm64(RCX, str_width as u64);
        self.emit_xor_rdx_rdx();

        let loop_start = self.code.len();

        // cmp byte [rsi], imm8
        self.code.extend_from_slice(&[0x80, 0x3E, needle]);
        let jne_skip = self.emit_jne_placeholder();
        // inc rdx
        self.code.extend_from_slice(&[0x48, 0xFF, 0xC2]);
        let skip = self.code.len();
        self.patch_rel32(jne_skip, skip);
        // inc rsi
        self.code.extend_from_slice(&[0x48, 0xFF, 0xC6]);
        // dec rcx
        self.code.extend_from_slice(&[0x48, 0xFF, 0xC9]);
        self.emit_jnz_rel32_back_to(loop_start);

        // mov rax, rdx
        self.emit_mov_rax_rdx();
    }

    fn emit_at_end(&mut self, args: &[Expr]) {
        if args.len() != 1 {
            panic!("codegen: at_end() takes 1 arg");
        }
        let file_name = match &args[0] {
            Expr::Ident(n) => n.clone(),
            _ => panic!("codegen: at_end() arg must be a file ident"),
        };
        let eof_off = self.file_eof_offset(&file_name);
        self.emit_mov_imm64_reloc(RBX, eof_off);
        self.emit_mov_rax_from_rbx();
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

    fn emit_goto(&mut self, name: &str) {
        if !self.paragraphs.iter().any(|(n, _)| n == name) {
            panic!("codegen: goto target `{}` is not a defined paragraph or sub", name);
        }
        self.code.push(0xE9); // jmp rel32
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        self.par_calls.push((pos, name.to_string()));
    }

    fn emit_ret(&mut self) {
        self.code.push(0xC3);
    }

    fn collect_symbols(&mut self, program: &Program) {
        for stmt in &program.statements {
            if let Stmt::Let { name, ty, init, eighty_eights: _ } = stmt {
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
                    (TypeExpr::Array { .. }, _) => {
                        let size = self.type_size_dyn(ty) as usize;
                        self.data.extend_from_slice(&vec![0u8; size]);
                    }
                    (TypeExpr::File, _) => {
                        panic!("codegen: file type cannot appear in `let` declaration");
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

    fn lvalue_type(&self, lv: &LValue) -> TypeExpr {
        match lv {
            LValue::Ident(name) => self.lookup_symbol(name).ty.clone(),
            LValue::Field { base, field } => {
                let (_, base_record) = self.lvalue_record_info(base);
                let info = self.record_types.get(&base_record).unwrap();
                info.fields
                    .iter()
                    .find(|f| f.name == *field)
                    .map(|f| f.ty.clone())
                    .unwrap_or_else(|| {
                        panic!("codegen: no field `{}` in record `{}`", field, base_record)
                    })
            }
            LValue::Index { base, .. } => {
                let base_ty = self.lvalue_type(base);
                match base_ty {
                    TypeExpr::Array { element, .. } => *element,
                    other => panic!("codegen: cannot index non-array type {:?}", other),
                }
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
            LValue::Index { .. } => {
                panic!("codegen: array element address is dynamic, use emit_lvalue_addr")
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
            LValue::Index { .. } => {
                panic!("codegen: array element record access not supported yet");
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
            Stmt::Let { .. }
            | Stmt::Par { .. }
            | Stmt::Sub { .. }
            | Stmt::TypeDef { .. }
            | Stmt::FileDecl { .. } => {}
            Stmt::Assign { target, value } => self.emit_assign(target, value),
            Stmt::For {
                var, start, end, body,
            } => self.emit_for(var, start, end, body),
            Stmt::While { cond, body } => self.emit_while(cond, body),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => self.emit_if(cond, then_body, else_body),
            Stmt::Match { expr, arms } => self.emit_match(expr, arms),
            Stmt::Goto { label } => self.emit_goto(label),
            Stmt::Call { name, args } => {
                if name == "print" {
                    if args.len() != 1 {
                        panic!("codegen: print() takes exactly one argument");
                    }
                    self.emit_print(&args[0]);
                } else if name == "print_money" {
                    if args.len() != 1 {
                        panic!("codegen: print_money() takes exactly one argument");
                    }
                    let scale = self
                        .try_resolve_decimal_ident(&args[0])
                        .unwrap_or_else(|| {
                            panic!("codegen: print_money requires a decimal-typed argument")
                        });
                    self.emit_expr(&args[0]);
                    self.emit_print_rax_money(scale);
                } else if name == "open" {
                    self.emit_file_open(args);
                } else if name == "close" {
                    self.emit_file_close(args);
                } else if name == "read" {
                    self.emit_file_read(args);
                } else if name == "write" {
                    self.emit_file_write(args);
                } else if name == "rewrite" {
                    self.emit_file_rewrite(args);
                } else if name == "replace_chars" {
                    self.emit_replace_chars(args);
                } else if name == "string_concat" {
                    self.emit_string_concat(args);
                } else if name == "unstring" {
                    self.emit_unstring(args);
                } else if self.is_sub(name) {
                    self.emit_call_sub(name, args);
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

    fn emit_match(&mut self, expr: &Expr, arms: &[MatchArm]) {
        if let Expr::Ident(name) = expr {
            let sym = self.lookup_symbol(name);
            if let TypeExpr::Str(width) = sym.ty.clone() {
                let subject_off = sym.offset_in_data;
                self.emit_str_match(subject_off, width, arms);
                return;
            }
        }
        self.emit_expr(expr);
        self.emit_push_rax();
        let mut end_jumps = Vec::new();
        let mut wildcard_body: Option<&[Stmt]> = None;
        for arm in arms {
            if matches!(arm.pattern, Pattern::Wildcard) {
                wildcard_body = Some(&arm.body);
                continue;
            }
            self.emit_mov_rbx_from_rsp_off(0);
            self.emit_mov_rax_rbx();
            let skip_positions = match arm.pattern {
                Pattern::Lit(v) => {
                    self.emit_mov_imm64(RBX, v as u64);
                    self.emit_cmp_rax_rbx();
                    vec![self.emit_jne_placeholder()]
                }
                Pattern::Range(lo, hi) => {
                    self.emit_mov_imm64(RBX, lo as u64);
                    self.emit_cmp_rax_rbx();
                    let jl = self.emit_jl_placeholder();
                    self.emit_mov_imm64(RBX, hi as u64);
                    self.emit_cmp_rax_rbx();
                    let jg = self.emit_jg_placeholder();
                    vec![jl, jg]
                }
                Pattern::Wildcard => unreachable!(),
                Pattern::StrLit(_) => {
                    panic!("codegen: string pattern only allowed when match subject is str-typed")
                }
            };
            for s in &arm.body {
                self.emit_stmt(s);
            }
            end_jumps.push(self.emit_jmp_placeholder());
            let next = self.code.len();
            for p in skip_positions {
                self.patch_rel32(p, next);
            }
        }
        if let Some(body) = wildcard_body {
            for s in body {
                self.emit_stmt(s);
            }
        }
        let end = self.code.len();
        for p in end_jumps {
            self.patch_rel32(p, end);
        }
        self.emit_add_rsp_imm8(8);
    }

    fn emit_mov_rax_rbx(&mut self) {
        // mov rax, rbx
        self.code.extend_from_slice(&[0x48, 0x89, 0xD8]);
    }

    fn emit_str_match(&mut self, subject_off: u64, width: u32, arms: &[MatchArm]) {
        let mut end_jumps = Vec::new();
        let mut wildcard_idx: Option<usize> = None;
        for (idx, arm) in arms.iter().enumerate() {
            match &arm.pattern {
                Pattern::StrLit(s) => {
                    if s.len() > width as usize {
                        panic!(
                            "codegen: str match literal of {} bytes exceeds str({})",
                            s.len(),
                            width
                        );
                    }
                    let mut bytes = s.as_bytes().to_vec();
                    bytes.resize(width as usize, b' ');
                    let lit_off = self.data.len() as u64;
                    self.data.extend(bytes);
                    self.emit_str_byte_compare(subject_off, lit_off, width);
                    let skip = self.emit_jne_placeholder();
                    for s in &arm.body {
                        self.emit_stmt(s);
                    }
                    end_jumps.push(self.emit_jmp_placeholder());
                    let next = self.code.len();
                    self.patch_rel32(skip, next);
                }
                Pattern::Wildcard => {
                    wildcard_idx = Some(idx);
                }
                _ => panic!(
                    "codegen: str-typed match subject requires string literal patterns or `_`"
                ),
            }
        }
        if let Some(idx) = wildcard_idx {
            for s in &arms[idx].body {
                self.emit_stmt(s);
            }
        }
        let end = self.code.len();
        for j in end_jumps {
            self.patch_rel32(j, end);
        }
    }

    fn resolve_expr_address(&self, e: &Expr) -> (u64, TypeExpr) {
        match e {
            Expr::Ident(name) => {
                let sym = self.lookup_symbol(name);
                (sym.offset_in_data, sym.ty.clone())
            }
            Expr::FieldAccess { base, field } => {
                let (base_off, base_ty) = self.resolve_expr_address(base);
                let rname = match base_ty {
                    TypeExpr::Record(n) => n,
                    other => panic!(
                        "codegen: cannot access field `{}` on non-record type {:?}",
                        field, other
                    ),
                };
                let info = self.record_types.get(&rname).unwrap_or_else(|| {
                    panic!("codegen: unknown record `{}`", rname)
                });
                let f = info
                    .fields
                    .iter()
                    .find(|f| f.name == *field)
                    .unwrap_or_else(|| {
                        panic!("codegen: no field `{}` in record `{}`", field, rname)
                    });
                (base_off + f.offset, f.ty.clone())
            }
            _ => panic!("codegen: cannot resolve expression to address: {:?}", e),
        }
    }

    fn str_operand_width(&self, e: &Expr) -> Option<u32> {
        match e {
            Expr::Ident(_) | Expr::FieldAccess { .. } => {
                let (_, ty) = self.resolve_expr_address(e);
                if let TypeExpr::Str(w) = ty {
                    Some(w)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn resolve_str_operand(&mut self, e: &Expr, width: u32) -> u64 {
        match e {
            Expr::Ident(_) | Expr::FieldAccess { .. } => self.resolve_expr_address(e).0,
            Expr::StringLit(s) => {
                if s.len() > width as usize {
                    panic!(
                        "codegen: string literal of {} bytes exceeds str({}) in compare",
                        s.len(),
                        width
                    );
                }
                let mut bytes = s.as_bytes().to_vec();
                bytes.resize(width as usize, b' ');
                let off = self.data.len() as u64;
                self.data.extend(bytes);
                off
            }
            _ => panic!("codegen: str compare operand must be ident or string literal"),
        }
    }

    fn emit_str_byte_compare(&mut self, left_off: u64, right_off: u64, width: u32) {
        self.emit_mov_imm64_reloc(RSI, left_off);
        self.emit_mov_imm64_reloc(RDI, right_off);
        self.emit_mov_imm64(RCX, width as u64);
        // cld
        self.code.push(0xFC);
        // repe cmpsb (F3 A6)
        self.code.extend_from_slice(&[0xF3, 0xA6]);
        // ZF=1 iff equal
    }

    fn emit_if(&mut self, cond: &Expr, then_body: &[Stmt], else_body: &[Stmt]) {
        self.emit_expr(cond);
        self.emit_test_rax_rax();
        let jz_pos = self.emit_jz_placeholder();
        for s in then_body {
            self.emit_stmt(s);
        }
        if else_body.is_empty() {
            let end = self.code.len();
            self.patch_rel32(jz_pos, end);
        } else {
            let jmp_pos = self.emit_jmp_placeholder();
            let else_start = self.code.len();
            self.patch_rel32(jz_pos, else_start);
            for s in else_body {
                self.emit_stmt(s);
            }
            let end = self.code.len();
            self.patch_rel32(jmp_pos, end);
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
        let existed = self.symbols.iter().any(|s| s.name == var);
        let var_offset = if existed {
            self.lookup_symbol(var).offset_in_data
        } else {
            let off = self.data.len() as u64;
            self.data.extend_from_slice(&[0u8; 8]);
            self.symbols.push(Symbol {
                name: var.to_string(),
                offset_in_data: off,
                ty: TypeExpr::UInt(18),
            });
            off
        };

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

        if !existed {
            self.symbols.pop();
        }
    }

    fn emit_assign(&mut self, target: &LValue, value: &Expr) {
        let target_ty = self.lvalue_type(target);
        if matches!(target, LValue::Index { .. }) {
            if let TypeExpr::Str(_) | TypeExpr::Record(_) = target_ty {
                panic!("codegen: array element of str/record not yet supported in assign");
            }
            self.emit_lvalue_addr(target);
            // RAX has the address; save it
            self.emit_push_rax();
            self.emit_expr(value);
            // Now RAX is value, RBX top-of-stack is address
            self.code.push(0x5B); // pop rbx
            self.emit_mov_at_rbx_rax();
        } else if let TypeExpr::Str(width) = target_ty {
            let target_offset = self.lvalue_data_offset(target);
            let source_offset = self.resolve_str_operand(value, width);
            self.emit_str_copy(source_offset, target_offset, width);
        } else {
            let target_offset = self.lvalue_data_offset(target);
            self.emit_expr(value);
            self.emit_mov_imm64_reloc(RBX, target_offset);
            self.emit_mov_at_rbx_rax();
        }
    }

    fn emit_lvalue_addr(&mut self, lv: &LValue) {
        // Puts the address into RAX.
        match lv {
            LValue::Ident(_) | LValue::Field { .. } => {
                let off = self.lvalue_data_offset(lv);
                self.emit_mov_imm64_reloc(RAX, off);
            }
            LValue::Index { base, idx } => {
                let base_ty = self.lvalue_type(base);
                let elem_size = match &base_ty {
                    TypeExpr::Array { element, .. } => self.type_size_dyn(element),
                    _ => panic!("not an array"),
                };
                let base_off = self.lvalue_data_offset(base);
                self.emit_expr(idx);
                // dec rax (idx -> 0-based)
                self.code.extend_from_slice(&[0x48, 0xFF, 0xC8]);
                self.emit_mov_imm64(RBX, elem_size);
                self.emit_imul_rax_rbx();
                self.emit_mov_imm64_reloc(RBX, base_off);
                self.emit_add_rax_rbx();
            }
        }
    }

    fn emit_str_copy(&mut self, src_off: u64, dst_off: u64, width: u32) {
        self.emit_mov_imm64_reloc(RSI, src_off);
        self.emit_mov_imm64_reloc(RDI, dst_off);
        self.emit_mov_imm64(RCX, width as u64);
        // cld
        self.code.push(0xFC);
        // rep movsb (F3 A4)
        self.code.extend_from_slice(&[0xF3, 0xA4]);
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
            Expr::FieldAccess { .. } => {
                let (_, ty) = self.resolve_expr_address(arg);
                if let TypeExpr::UDec(_, m) | TypeExpr::IDec(_, m) = ty {
                    Some(m)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn try_resolve_str_ident(&self, arg: &Expr) -> Option<(u64, u64)> {
        match arg {
            Expr::Ident(_) | Expr::FieldAccess { .. } => {
                let (off, ty) = self.resolve_expr_address(arg);
                if let TypeExpr::Str(n) = ty {
                    Some((off, n as u64))
                } else {
                    None
                }
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

    fn emit_print_rax_money(&mut self, scale: u32) {
        self.emit_mov_imm64_reloc(RSI, SCRATCH_END);
        self.emit_mov_imm64(RCX, 10);
        self.emit_mov_imm64(RDI, 0);
        self.emit_mov_imm64(RBX, 0);

        let loop_start = self.code.len();

        // Comma check: if rdi >= scale && rbx == 3, write ',', set rbx = 0
        self.emit_cmp_rdi_imm8(scale as u8);
        let jl_skip_comma = self.emit_jl_placeholder();
        self.emit_cmp_rbx_imm8(3);
        let jne_skip_comma = self.emit_jne_placeholder();
        self.emit_dec_rsi();
        self.emit_mov_byte_at_rsi_imm(b',');
        self.emit_xor_rbx_rbx();
        let after_comma = self.code.len();
        self.patch_rel32(jl_skip_comma, after_comma);
        self.patch_rel32(jne_skip_comma, after_comma);

        // Dot check
        self.emit_cmp_rdi_imm8(scale as u8);
        let jne_dot = self.emit_jne_placeholder();
        self.emit_dec_rsi();
        self.emit_mov_byte_at_rsi_imm(b'.');
        let after_dot = self.code.len();
        self.patch_rel32(jne_dot, after_dot);

        // Digit write
        self.emit_xor_rdx_rdx();
        self.emit_div_rcx();
        self.emit_add_rdx_imm8(0x30);
        self.emit_dec_rsi();
        self.emit_mov_at_rsi_dl();

        // If we just wrote an integer digit (rdi >= scale), inc rbx
        self.emit_cmp_rdi_imm8(scale as u8);
        let jl_no_bump = self.emit_jl_placeholder();
        self.emit_inc_rbx();
        let after_bump = self.code.len();
        self.patch_rel32(jl_no_bump, after_bump);

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

    fn emit_cmp_rbx_imm8(&mut self, imm: u8) {
        // cmp rbx, imm8 (sign-extended)
        self.code.extend_from_slice(&[0x48, 0x83, 0xFB, imm]);
    }

    fn emit_xor_rbx_rbx(&mut self) {
        // xor rbx, rbx
        self.code.extend_from_slice(&[0x48, 0x31, 0xDB]);
    }

    fn emit_inc_rbx(&mut self) {
        // inc rbx
        self.code.extend_from_slice(&[0x48, 0xFF, 0xC3]);
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
                if let Some((parent, value)) = self.eighty_eights.get(name).cloned() {
                    let parent_sym = self.lookup_symbol(&parent).clone();
                    match (value, &parent_sym.ty) {
                        (EightyEightValue::Int(v), _) => {
                            self.emit_mov_imm64_reloc(RBX, parent_sym.offset_in_data);
                            self.emit_mov_rax_from_rbx();
                            self.emit_mov_imm64(RBX, v as u64);
                            self.emit_cmp_rax_rbx();
                            self.emit_setcc_dl(CmpOp::Eq);
                            self.emit_movzx_eax_dl();
                        }
                        (EightyEightValue::Str(s), TypeExpr::Str(width)) => {
                            let mut bytes = s.as_bytes().to_vec();
                            if bytes.len() > *width as usize {
                                panic!(
                                    "codegen: 88-level `{}` literal exceeds str({})",
                                    name, width
                                );
                            }
                            bytes.resize(*width as usize, b' ');
                            let lit_off = self.data.len() as u64;
                            self.data.extend(bytes);
                            self.emit_str_byte_compare(
                                parent_sym.offset_in_data,
                                lit_off,
                                *width,
                            );
                            self.emit_setcc_dl(CmpOp::Eq);
                            self.emit_movzx_eax_dl();
                        }
                        (EightyEightValue::Str(_), other) => {
                            panic!(
                                "codegen: 88-level `{}` has string value but parent type is {:?}",
                                name, other
                            );
                        }
                    }
                } else {
                    let offset = self.lookup_symbol(name).offset_in_data;
                    self.emit_mov_imm64_reloc(RBX, offset);
                    self.emit_mov_rax_from_rbx();
                }
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
                let l_w = self.str_operand_width(left);
                let r_w = self.str_operand_width(right);
                if l_w.is_some() || r_w.is_some() {
                    if !matches!(op, CmpOp::Eq | CmpOp::Ne) {
                        panic!("codegen: string compare supports only == and !=");
                    }
                    let width = l_w.or(r_w).unwrap();
                    if let Some(lw) = l_w {
                        if lw != width {
                            panic!("codegen: str compare width mismatch ({} vs {})", lw, width);
                        }
                    }
                    if let Some(rw) = r_w {
                        if rw != width {
                            panic!("codegen: str compare width mismatch ({} vs {})", rw, width);
                        }
                    }
                    let left_off = self.resolve_str_operand(left, width);
                    let right_off = self.resolve_str_operand(right, width);
                    self.emit_str_byte_compare(left_off, right_off, width);
                    self.emit_setcc_dl(*op);
                    self.emit_movzx_eax_dl();
                } else {
                    self.emit_expr(left);
                    self.emit_push_rax();
                    self.emit_expr(right);
                    self.emit_mov_rbx_rax();
                    self.emit_pop_rax();
                    self.emit_cmp_rax_rbx();
                    self.emit_setcc_dl(*op);
                    self.emit_movzx_eax_dl();
                }
            }
            Expr::Not { inner } => {
                self.emit_expr(inner);
                self.emit_test_rax_rax();
                self.emit_setcc_dl_eq_zero();
                self.emit_movzx_eax_dl();
            }
            Expr::And { left, right } => {
                self.emit_expr(left);
                self.emit_test_rax_rax();
                let skip = self.emit_jz_placeholder();
                self.emit_expr(right);
                let end = self.code.len();
                self.patch_rel32(skip, end);
            }
            Expr::Or { left, right } => {
                self.emit_expr(left);
                self.emit_test_rax_rax();
                let skip = self.emit_jnz_placeholder();
                self.emit_expr(right);
                let end = self.code.len();
                self.patch_rel32(skip, end);
            }
            Expr::Call { name, args } => {
                if name == "at_end" {
                    self.emit_at_end(args);
                } else if name == "count_chars" {
                    self.emit_count_chars(args);
                } else {
                    panic!("codegen: `{}` not callable in expression context", name);
                }
            }
            Expr::FieldAccess { .. } => {
                let (offset, _ty) = self.resolve_expr_address(expr);
                self.emit_mov_imm64_reloc(RBX, offset);
                self.emit_mov_rax_from_rbx();
            }
            Expr::Index { base, idx } => {
                // Resolve base type to find element size.
                let base_name = match base.as_ref() {
                    Expr::Ident(n) => n.clone(),
                    _ => panic!("codegen: array index base must be a simple ident"),
                };
                let sym = self.lookup_symbol(&base_name).clone();
                let (elem_ty, base_off) = match &sym.ty {
                    TypeExpr::Array { element, .. } => {
                        (element.as_ref().clone(), sym.offset_in_data)
                    }
                    _ => panic!("codegen: `{}` is not an array", base_name),
                };
                let elem_size = self.type_size_dyn(&elem_ty);
                // Compute address
                self.emit_expr(idx);
                self.code.extend_from_slice(&[0x48, 0xFF, 0xC8]); // dec rax
                self.emit_mov_imm64(RBX, elem_size);
                self.emit_imul_rax_rbx();
                self.emit_mov_imm64_reloc(RBX, base_off);
                self.emit_add_rax_rbx();
                // Load element value
                match elem_ty {
                    TypeExpr::UInt(_) | TypeExpr::UDec(_, _) | TypeExpr::IDec(_, _) => {
                        // mov rax, [rax]
                        self.code.extend_from_slice(&[0x48, 0x8B, 0x00]);
                    }
                    _ => panic!(
                        "codegen: array element of type {:?} not loadable as expression yet",
                        elem_ty
                    ),
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

    fn emit_mov_rdi_from_rbx(&mut self) {
        // mov rdi, [rbx]
        self.code.extend_from_slice(&[0x48, 0x8B, 0x3B]);
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

    fn emit_jmp_placeholder(&mut self) -> usize {
        // jmp rel32: E9 + 4-byte placeholder
        self.code.push(0xE9);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        pos
    }

    fn emit_jnz_placeholder(&mut self) -> usize {
        // jnz rel32: 0F 85 + 4-byte placeholder
        self.code.push(0x0F);
        self.code.push(0x85);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        pos
    }

    fn emit_jl_placeholder(&mut self) -> usize {
        // jl rel32 (signed less): 0F 8C + 4-byte placeholder
        self.code.push(0x0F);
        self.code.push(0x8C);
        let pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 4]);
        pos
    }

    fn emit_jg_placeholder(&mut self) -> usize {
        // jg rel32 (signed greater): 0F 8F + 4-byte placeholder
        self.code.push(0x0F);
        self.code.push(0x8F);
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
        TypeExpr::File => panic!("codegen: file type cannot be a field"),
        TypeExpr::Array { element, length } => type_size(element) * (*length as u64),
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
        | Expr::And { .. }
        | Expr::Or { .. }
        | Expr::FieldAccess { .. }
        | Expr::Index { .. }
        | Expr::Call { .. } => None,
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
