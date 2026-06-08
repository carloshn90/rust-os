use core::ptr::{read_volatile, write_volatile};

use hal::klog;

use crate::uart_logger::logger;

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
pub extern "C" fn rust_syscall_handler(syscall_id: u64, arg0: u64, arg1: u64) {
    let l = logger();

    match syscall_id {
        0 => {}
        99 => {
            l.log("[KERNEL CATCH] Syscall 99 triggered successfully! User code is executing!\n");
        }
        64 => {
            l.log("[KERNEL CATCH] Syscall 64 (SYS_WRITE) triggered!\n");

            let str_ptr = arg0 as *const u8;
            let str_len = arg1 as usize;

            unsafe {
                if !str_ptr.is_null() && str_len > 0 {
                    // Turn the raw user pointer and length into a safe Rust byte slice
                    let byte_slice = core::slice::from_raw_parts(str_ptr, str_len);

                    // Attempt to parse it as valid UTF-8 string data
                    if let Ok(user_str) = core::str::from_utf8(byte_slice) {
                        l.log(user_str);
                    } else {
                        l.log("[KERNEL ERROR] User string was not valid UTF-8\n");
                    }
                }
            }
        }
        _ => {
            // Log unhandled system calls
            l.log("[KERNEL CATCH] Unknown Syscall ID detected.\n");
        }
    }
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
            "msr daifset, #2", // set I bit: mask IRQs
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
                "msr daifclr, #2", // clear I bit: unmask IRQs
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
        core::arch::asm!("msr daifclr, #2", options(nostack, nomem));
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
