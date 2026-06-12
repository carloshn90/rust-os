use core::ptr::read_unaligned;

pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const PT_LOAD: u32 = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Header {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64, // Entry point virtual address
    pub e_phoff: u64, // Program header table file offset
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16, // Size of a program header table entry
    pub e_phnum: u16,     // Number of program header table entries
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Phdr {
    pub p_type: u32,   // Segment type (e.g., PT_LOAD)
    pub p_flags: u32,  // Segment flags (R, W, X)
    pub p_offset: u64, // Segment file offset
    pub p_vaddr: u64,  // Segment virtual address
    pub p_paddr: u64,  // Segment physical address
    pub p_filesz: u64, // Segment size in the file
    pub p_memsz: u64,  // Segment size in memory (can be > p_filesz for .bss)
    pub p_align: u64,
}

pub fn get_elf_header(bytes: &[u8]) -> Option<Elf64Header> {
    if bytes.len() < size_of::<Elf64Header>() {
        return None;
    }

    unsafe { Some(read_unaligned(bytes.as_ptr() as *const Elf64Header)) }
}

pub fn get_program_header(bytes: &[u8], offset: usize) -> Option<Elf64Phdr> {
    if bytes.len() < offset + size_of::<Elf64Phdr>() {
        return None;
    }

    unsafe {
        Some(read_unaligned(
            bytes.as_ptr().add(offset) as *const Elf64Phdr
        ))
    }
}
