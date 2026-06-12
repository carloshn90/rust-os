use core::ptr::{read_volatile, write_volatile};

use common::system_call::{SYS_EXEC, SYS_EXIT, SYS_FORK, SYS_READ, SYS_WAIT, SYS_WRITE};
use hal::klog;

use crate::{
    exec::k_exec,
    process::{exit_current, fork_current, user_trap_return, wait_current},
    scheduler::my_proc,
    uart_logger::{logger, uart_getc},
};

const GICD_BASE: *mut u32 = 0x0800_0000 as *mut u32;
const GICC_BASE: *mut u32 = 0x0801_0000 as *mut u32;
const IRQ_VIRTIO0: usize = 48; // often 48 on qemu virt

const GICC_IAR: usize = 0x000C; // Interrupt Acknowledge Register
const GICC_EOIR: usize = 0x0010; // End of Interrupt Register

const GICD_IPRIORITYR: usize = 0x400;
const GICD_ITARGETSR: usize = 0x800;

#[derive(Copy, Clone)]
pub struct IrqState {
    daif: u64,
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_syscall_handler(syscall_id: u64, arg0: u64, arg1: u64, ctx: *mut u64) {
    let l = logger();

    match syscall_id {
        0 => {}
        99 => {
            l.log("[KERNEL CATCH] Syscall 99 triggered successfully! User code is executing!\n");
        }
        SYS_WRITE => {
            let str_ptr = arg0 as *const u8;
            let str_len = arg1 as usize;

            unsafe {
                if !str_ptr.is_null() && str_len > 0 {
                    let byte_slice = core::slice::from_raw_parts(str_ptr, str_len);
                    if let Ok(user_str) = core::str::from_utf8(byte_slice) {
                        l.log(user_str);
                    } else {
                        l.log("[KERNEL ERROR] User string was not valid UTF-8\n");
                    }
                }
            }
        }
        SYS_READ => {
            // Block until one character is available on the UART, then return it in x0.
            let c = uart_getc();
            unsafe { *ctx = c as u64 };
        }
        SYS_EXIT => {
            klog!(l, "[KERNEL] SYS_EXIT called\n");
            exit_current();
        }
        SYS_FORK => {
            let user_sp: u64;
            unsafe {
                core::arch::asm!("mrs {0}, sp_el0", out(reg) user_sp, options(nomem, nostack, preserves_flags));
            }
            match fork_current(ctx as *const u64, user_sp) {
                Ok(pid) => unsafe { *ctx = pid as u64 },
                Err(err) => {
                    klog!(l, "[KERNEL] SYS_FORK failed: {}\n", err);
                    unsafe { *ctx = u64::MAX };
                }
            }
        }
        SYS_EXEC => {
            let name_ptr = arg0 as *const u8;
            let name_len = arg1 as usize;

            unsafe {
                if name_ptr.is_null() || name_len == 0 {
                    *ctx = u64::MAX;
                    return;
                }

                let name_bytes = core::slice::from_raw_parts(name_ptr, name_len);
                let Ok(name) = core::str::from_utf8(name_bytes) else {
                    *ctx = u64::MAX;
                    return;
                };

                let process = my_proc();
                if process.is_null() {
                    *ctx = u64::MAX;
                    return;
                }

                match k_exec(name, &mut *process) {
                    Ok(tf) => user_trap_return(tf),
                    Err(err) => {
                        klog!(l, "[KERNEL] SYS_EXEC failed for {}: {:?}\n", name, err);
                        *ctx = u64::MAX;
                    }
                }
            }
        }
        SYS_WAIT => match wait_current() {
            Ok(pid) => unsafe { *ctx = pid as u64 },
            Err(err) => {
                klog!(l, "[KERNEL] SYS_WAIT failed: {}\n", err);
                unsafe { *ctx = u64::MAX };
            }
        },
        _ => {
            klog!(l, "[KERNEL CATCH] Unknown Syscall ID {}\n", syscall_id);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_user_sync_fault(esr: u64, elr: u64, far: u64) {
    let ec = (esr >> 26) & 0x3F;
    klog!(
        logger(),
        "[KERNEL FAULT] EL0 sync fault: ESR=0x{:X} EC=0x{:X} ELR=0x{:X} FAR=0x{:X}\n",
        esr,
        ec,
        elr,
        far
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_kernel_sync_fault(esr: u64, elr: u64, far: u64) {
    let ec = (esr >> 26) & 0x3F;
    klog!(
        logger(),
        "[KERNEL FAULT] EL1 sync fault: ESR=0x{:X} EC=0x{:X} ELR=0x{:X} FAR=0x{:X}\n",
        esr,
        ec,
        elr,
        far
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_irq_handler(current_context: *mut usize) -> *mut usize {
    // 1. Acknowledge interrupt (tells GIC we are servicing it)
    let iar = gicc_read(GICC_IAR);
    let interrupt_id = (iar as usize) & 0x3FF; // ID is in bits [9:0]

    // Spurious interrupts can happen (ID 1023)
    if interrupt_id < 1022 {
        klog!(logger(), "Handling irq = {}\n", interrupt_id);

        if interrupt_id == IRQ_VIRTIO0 {
            // virtio_disk_intr();
        }

        // Optional: Route internally if you want to handle individual IDs
        // if interrupt_id == 5 { ... }
    }

    // 2. Clear/Signal completion to the GIC
    gicc_write(GICC_EOIR, iar);

    current_context
}

#[inline(always)]
#[allow(dead_code)]
pub fn push_off() -> IrqState {
    let daif: u64;
    unsafe {
        core::arch::asm!(
            "mrs {0}, daif",
            "msr daifset, 2", // set I bit: mask IRQs
            out(reg) daif,
            options(nomem, nostack, preserves_flags),
        );
    }
    IrqState { daif }
}

#[inline(always)]
#[allow(dead_code)]
pub fn pop_off(state: IrqState) {
    // Restore only the IRQ mask state to what it was before.
    if (state.daif & (1 << 7)) == 0 {
        unsafe {
            core::arch::asm!(
                "msr daifclr, 2", // clear I bit: unmask IRQs
                options(nomem, nostack, preserves_flags),
            );
        }
    }
}

#[inline(always)]
fn gicd_write(offset_bytes: usize, value: u32) {
    unsafe {
        write_volatile(GICD_BASE.add(offset_bytes / 4), value);
    }
}

#[inline(always)]
fn gicc_write(offset_bytes: usize, value: u32) {
    unsafe {
        write_volatile(GICC_BASE.add(offset_bytes / 4), value);
    }
}

#[inline(always)]
fn gicd_read(offset_bytes: usize) -> u32 {
    unsafe { read_volatile(GICD_BASE.add(offset_bytes / 4)) }
}

#[inline(always)]
fn gicc_read(offset_bytes: usize) -> u32 {
    unsafe { read_volatile(GICC_BASE.add(offset_bytes / 4)) }
}

pub fn init_irq() {
    // 1. Configure the CPU Interface
    gicc_write(0x0004, 0xFF); // GICC_PMR: Accept all priorities
    gicc_write(0x0000, 0x01); // GICC_CTLR: Enabled CPU interface

    enable_interrupt(IRQ_VIRTIO0, 0x02, 0x01); // 0x00 high priority and 0x<FF lower priority 0x01 core 0

    gicd_write(0x0, 0x01);

    // Unmask IRQs (clear DAIF.I)
    unsafe {
        core::arch::asm!("msr daifclr, 2", options(nostack, nomem));
    }
}

fn enable_interrupt(int_id: usize, priority: u8, target_cpu_mask: u8) {
    // 2. Set Priority (Each GICD_IPRIORITYR holds 4 fields)
    let pri_reg_offset: usize = GICD_IPRIORITYR + (int_id & !3);
    let pri_shift = (int_id % 4) * 8;

    // Clear and set the specific byte
    gicd_write(pri_reg_offset, (priority as u32) << pri_shift);

    // 3. Set Target Processor (Each GICD_ITARGETSR holds 4 fields)
    // SPIs only
    if int_id >= 32 {
        let target_reg_offset = GICD_ITARGETSR + (int_id & !3);
        let target_shift = (int_id % 4) * 8;

        let mut target_val = gicd_read(target_reg_offset);
        target_val &= !(0xFF << target_shift); // Clear old target byte
        target_val |= (target_cpu_mask as u32) << target_shift; // Inset new target mask
        gicd_write(target_reg_offset, target_val);
    }

    // 4. Enable the specific ID in GICD_ISENABLER
    let enable_reg_offset = 0x100 + ((int_id / 32) * 4);
    let enable_bit = 1 << (int_id % 32);
    gicd_write(enable_reg_offset, enable_bit);
}

#[allow(dead_code)]
pub fn pend_interrupt(int_id: usize) {
    let pend_reg_offset = 0x200 + ((int_id / 32) * 4);
    let pend_bit = 1u32 << (int_id % 32);
    gicd_write(pend_reg_offset, pend_bit);
}
