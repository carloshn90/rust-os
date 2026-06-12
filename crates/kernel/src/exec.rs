use hal::{kdebug, klog};

use crate::{
    elf::{self, ELF_MAGIC, PT_LOAD},
    memory::PAGE_SIZE,
    process::{Process, TrapFrame},
    uart_logger::logger,
    virtual_memory::{
        AF, AP_EL1_RW_EL0_RW, AP_EL1_RX_EL0_RX, ATTRIDX0, PageTable, kvm_translate,
        proc_page_table, u_vm_alloc,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecError {
    MappingFailed,
    InvalidFormat,
    ProgramNotFound,
}

pub struct UserBinaryPaths {
    pub help: &'static str,
    pub sh: &'static str,
}

static USER_BINARY_PATHS: UserBinaryPaths = UserBinaryPaths {
    help: "../../../target/aarch64-unknown-none/debug/help",
    sh: "../../../target/aarch64-unknown-none/debug/sh",
};

const SH_BIN: &[u8] = include_bytes!("../../../target/aarch64-unknown-none/debug/sh");
const HELP_BIN: &[u8] = include_bytes!("../../../target/aarch64-unknown-none/debug/help");

fn resolve_user_binary(name: &str) -> Option<(&'static str, &'static [u8])> {
    match name {
        "sh" => Some((USER_BINARY_PATHS.sh, SH_BIN)),
        "help" => Some((USER_BINARY_PATHS.help, HELP_BIN)),
        _ => None,
    }
}

pub fn k_exec(path: &str, process: &mut Process) -> Result<*mut TrapFrame, ExecError> {
    kdebug!(logger(), "[k_exec] starting\n");
    let (resolved_path, elf_bytes) = resolve_user_binary(path).ok_or(ExecError::ProgramNotFound)?;
    kdebug!(
        logger(),
        "[k_exec] resolved {} -> {}\n",
        path,
        resolved_path
    );

    let headers = elf::get_elf_header(elf_bytes).ok_or(ExecError::InvalidFormat)?;
    kdebug!(
        logger(),
        "ELF: entry=0x{:X} phoff=0x{:X} phnum={} phentsz={}\n",
        headers.e_entry,
        headers.e_phoff,
        headers.e_phnum,
        headers.e_phentsize,
    );

    // Verify ELF Magic Number (\x7F ELF)
    if headers.e_ident[0..4] != ELF_MAGIC {
        kdebug!(logger(), "[k_exec] Error: Invalid ELF magic number\n");
        return Err(ExecError::InvalidFormat);
    }

    let ph_offset = headers.e_phoff as usize;
    let ph_count = headers.e_phnum as usize;
    let ph_size = headers.e_phentsize as usize;

    // Exec replaces the process image. Build a fresh page table rooted on
    // kernel/trampoline/trapframe mappings, then load the new user image into it.
    let user_page_table = proc_page_table(process).map_err(|_| ExecError::MappingFailed)?;
    let user_page_table = user_page_table as *mut PageTable;

    for i in 0..ph_count {
        let current_ph_offset = ph_offset + (i * ph_size);
        let ph = elf::get_program_header(elf_bytes, current_ph_offset)
            .ok_or(ExecError::InvalidFormat)?;

        kdebug!(
            logger(),
            "[k_exec] ph[{}]: type={} off=0x{:X} vaddr=0x{:X} filesz=0x{:X} memsz=0x{:X} flags=0x{:X}\n",
            i,
            ph.p_type,
            ph.p_offset,
            ph.p_vaddr,
            ph.p_filesz,
            ph.p_memsz,
            ph.p_flags,
        );

        if ph.p_type == PT_LOAD {
            let start_addr = ph.p_vaddr as usize;
            let mem_size = ph.p_memsz as usize;
            let file_size = ph.p_filesz as usize;
            let offset = ph.p_offset as usize;

            if offset > elf_bytes.len() || file_size > elf_bytes.len() - offset {
                kdebug!(
                    logger(),
                    "[k_exec] Error: PT_LOAD segment exceeds ELF buffer\n"
                );
                return Err(ExecError::InvalidFormat);
            }

            let num_page = (mem_size + PAGE_SIZE - 1) / PAGE_SIZE;
            let total_size = num_page * PAGE_SIZE;
            let end_addr = start_addr + total_size;

            u_vm_alloc(
                user_page_table,
                start_addr,
                end_addr,
                AF | ATTRIDX0 | AP_EL1_RX_EL0_RX,
            )
            .map_err(|_| ExecError::MappingFailed)?;

            let segment_data = &elf_bytes[offset..offset + file_size];

            load_seg(
                user_page_table,
                start_addr,
                file_size,
                num_page,
                segment_data,
            )?;

            // fixme Traverses the user page table to zero out a specific range of virtual memory
            // zero_user_vm_region
        }
    }

    // Allocate and map the User Stack safely in a distinct range
    let stack_virt_bottom = 0x0000_0000_8000_0000;
    u_vm_alloc(
        user_page_table,
        stack_virt_bottom,
        stack_virt_bottom + PAGE_SIZE,
        AF | ATTRIDX0 | AP_EL1_RW_EL0_RW,
    )
    .map_err(|_| ExecError::MappingFailed)?;

    let tf = process.trap_frame as *mut TrapFrame;
    unsafe {
        (*tf).epc = headers.e_entry; // Entry point matches your new binary location
        (*tf).sp = (stack_virt_bottom + PAGE_SIZE) as u64;
        (*tf).spsr = 0x0;
        (*tf).ttbr0 = user_page_table as u64;
    }

    process.page_table = user_page_table as u64;

    // PT_LOAD copied new instructions into memory. Ensure EL0 does not run
    // stale cached instructions from the previous image at the same VAs.
    flush_icache_all();

    kdebug!(logger(), "[k_exec] return tf\n");

    Ok(tf)
}

#[inline(always)]
fn flush_icache_all() {
    unsafe {
        core::arch::asm!(
            "dsb ish",
            "ic iallu",
            "dsb ish",
            "isb",
            options(nostack, preserves_flags),
        );
    }
}

fn load_seg(
    page_table: *mut PageTable,
    va_start: usize,
    bin_len: usize,
    num_pages: usize,
    bin: &[u8],
) -> Result<(), ExecError> {
    for i in 0..num_pages {
        let offset = i * PAGE_SIZE;
        let va = (va_start + offset) as u64;
        let chunk_size = if offset < bin_len {
            core::cmp::min(PAGE_SIZE, bin_len - offset)
        } else {
            0
        };

        let phys_pt = kvm_translate(page_table, va).map_err(|_| ExecError::MappingFailed)?;

        if chunk_size > 0 {
            unsafe {
                core::ptr::copy_nonoverlapping(bin.as_ptr().add(offset), phys_pt, chunk_size);
            }
        }
    }

    Ok(())
}
