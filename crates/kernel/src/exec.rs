use crate::{
    memory::PAGE_SIZE,
    process::{Process, TrapFrame},
    uart_logger::logger,
    virtual_memory::{
        AF, AP_EL1_RW_EL0_RW, AP_EL1_RX_EL0_RX, ATTRIDX0, KERNEL_PAGE_TABLE, PageTable,
        kvm_translate, u_vm_alloc,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecError {
    MappingFailed,
}

pub fn k_exec(_path: &str, process: &mut Process) -> Result<*mut TrapFrame, ExecError> {
    logger().log("[k_exec] starting\n");
    let bin: &[u8] = include_bytes!("../../../target/user_hello.bin");
    let bin_len = bin.len();

    // Use the core kernel root layout directly
    let proc_page_table =
        KERNEL_PAGE_TABLE.load(core::sync::atomic::Ordering::SeqCst) as *mut PageTable;

    let num_pages = (bin_len + PAGE_SIZE - 1) / PAGE_SIZE;
    let start_addr = 0x0010_0000;
    let total_size = num_pages * PAGE_SIZE;
    let end_addr = start_addr + total_size as u64;

    u_vm_alloc(
        proc_page_table,
        start_addr as usize,
        end_addr as usize,
        AF | ATTRIDX0 | AP_EL1_RX_EL0_RX,
    )
    .map_err(|_| ExecError::MappingFailed)?;

    load_seg(
        proc_page_table,
        start_addr as usize,
        bin_len,
        num_pages,
        bin,
    )?;

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
        (*tf).epc = 0x0010_0000; // Entry point matches your new binary location
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
