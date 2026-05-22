use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;

pub const LOAD_ADDR: u64 = 0x400000;
pub const ELF_HEADER_SIZE: u64 = 64;
pub const PROGRAM_HEADER_SIZE: u64 = 56;

pub const ENTRY_VMA: u64 = LOAD_ADDR + ELF_HEADER_SIZE + PROGRAM_HEADER_SIZE;

pub fn write_elf(path: &str, segment: &[u8]) -> io::Result<()> {
    let mut buf =
        Vec::with_capacity((ELF_HEADER_SIZE + PROGRAM_HEADER_SIZE) as usize + segment.len());

    let file_size = ELF_HEADER_SIZE + PROGRAM_HEADER_SIZE + segment.len() as u64;

    buf.extend_from_slice(&[0x7f, b'E', b'L', b'F']); // EI_MAG: magic
    buf.push(2); // EI_CLASS = ELFCLASS64
    buf.push(1); // EI_DATA  = ELFDATA2LSB (little-endian)
    buf.push(1); // EI_VERSION = EV_CURRENT
    buf.push(0); // EI_OSABI = ELFOSABI_NONE (System V)
    buf.push(0); // EI_ABIVERSION
    buf.extend_from_slice(&[0u8; 7]); // EI_PAD (7 bytes)

    buf.extend_from_slice(&2u16.to_le_bytes()); // e_type    = ET_EXEC
    buf.extend_from_slice(&0x3eu16.to_le_bytes()); // e_machine = EM_X86_64
    buf.extend_from_slice(&1u32.to_le_bytes()); // e_version = EV_CURRENT
    buf.extend_from_slice(&ENTRY_VMA.to_le_bytes()); // e_entry
    buf.extend_from_slice(&ELF_HEADER_SIZE.to_le_bytes()); // e_phoff (program header offset)
    buf.extend_from_slice(&0u64.to_le_bytes()); // e_shoff (no section headers)
    buf.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    buf.extend_from_slice(&(ELF_HEADER_SIZE as u16).to_le_bytes()); // e_ehsize
    buf.extend_from_slice(&(PROGRAM_HEADER_SIZE as u16).to_le_bytes()); // e_phentsize
    buf.extend_from_slice(&1u16.to_le_bytes()); // e_phnum = 1
    buf.extend_from_slice(&0u16.to_le_bytes()); // e_shentsize
    buf.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
    buf.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx

    debug_assert_eq!(buf.len(), ELF_HEADER_SIZE as usize);

    // header
    buf.extend_from_slice(&1u32.to_le_bytes()); // p_type   = PT_LOAD
    buf.extend_from_slice(&7u32.to_le_bytes()); // p_flags  = PF_R | PF_W | PF_X
    buf.extend_from_slice(&0u64.to_le_bytes()); // p_offset (load from start of file)
    buf.extend_from_slice(&LOAD_ADDR.to_le_bytes()); // p_vaddr
    buf.extend_from_slice(&LOAD_ADDR.to_le_bytes()); // p_paddr  (irrelevant on Linux but conventionally same)
    buf.extend_from_slice(&file_size.to_le_bytes()); // p_filesz
    buf.extend_from_slice(&file_size.to_le_bytes()); // p_memsz  (no .bss, so same as filesz)
    buf.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align  = 4 KiB page

    debug_assert_eq!(buf.len(), (ELF_HEADER_SIZE + PROGRAM_HEADER_SIZE) as usize);

    //code + rodata
    buf.extend_from_slice(segment);

    let mut file = File::create(path)?;
    file.write_all(&buf)?;

    // chmod +x
    let mut perms = file.metadata()?.permissions();
    perms.set_mode(0o755);
    file.set_permissions(perms)?;

    Ok(())
}
