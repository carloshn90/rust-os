use hal::klog;

use crate::{
    elf::{self, ELF_MAGIC, PT_LOAD},
    memory::PAGE_SIZE,
    process::{Process, TrapFrame},
    uart_logger::logger,
    virtual_memory::{
        AF, AP_EL1_RW_EL0_RW, AP_EL1_RX_EL0_RX, ATTRIDX0, PageTable, kvm_translate, u_vm_alloc,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecError {
    MappingFailed,
    InvalidFormat,
}

pub fn k_exec(_path: &'static str, process: &mut Process) -> Result<*mut TrapFrame, ExecError> {
    logger().log("[k_exec] starting\n");
    let elf_bytes: &[u8] = include_bytes!("../../../target/aarch64-unknown-none/debug/user");

    let headers = elf::get_elf_header(elf_bytes).ok_or(ExecError::InvalidFormat)?;
    klog!(logger(), "ELF headers: {:?}\n", headers);

    // Verify ELF Magic Number (\x7F ELF)
    if headers.e_ident[0..4] != ELF_MAGIC {
        logger().log("[k_exec] Error: Invalid ELF magic number\n");
        return Err(ExecError::InvalidFormat);
    }

    let ph_offset = headers.e_phoff as usize;
    let ph_count = headers.e_phnum as usize;
    let ph_size = headers.e_phentsize as usize;

    let proc_page_table = process.page_table as *mut PageTable;

    for i in 0..ph_count {
        let current_ph_offset = ph_offset + (i * ph_size);
        let ph = elf::get_program_header(elf_bytes, current_ph_offset)
            .ok_or(ExecError::InvalidFormat)?;

        if ph.p_type == PT_LOAD {
            let start_addr = ph.p_vaddr as usize;
            let mem_size = ph.p_memsz as usize;
            let file_size = ph.p_filesz as usize;

            let num_page = (mem_size + PAGE_SIZE - 1) / PAGE_SIZE;
            let total_size = num_page * PAGE_SIZE;
            let end_addr = start_addr + total_size;

            u_vm_alloc(
                proc_page_table,
                start_addr,
                end_addr,
                AF | ATTRIDX0 | AP_EL1_RX_EL0_RX,
            )
            .map_err(|_| ExecError::MappingFailed)?;

            let offset = ph.p_offset as usize;
            let segment_data = &elf_bytes[offset..offset + file_size];

            load_seg(
                proc_page_table,
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
        proc_page_table,
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
        (*tf).ttbr0 = proc_page_table as u64;
    }

    process.page_table = proc_page_table as u64;

    logger().log("[k_exec] return tf\n");

    Ok(tf)
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
        let chunk_size = core::cmp::min(PAGE_SIZE, bin_len - offset);

        let phys_pt = kvm_translate(page_table, va).map_err(|_| ExecError::MappingFailed)?;

        unsafe {
            core::ptr::copy_nonoverlapping(bin.as_ptr(), phys_pt, chunk_size);
        }
    }

    Ok(())
}
