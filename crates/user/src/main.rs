#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[inline(always)]
pub fn sys_write(s: &str) {
    let ptr = s.as_ptr() as u64;
    let len = s.len() as u64;

    unsafe {
        core::arch::asm!(
            "mov x8, #64",
            "mov x0, {ptr}",
            "mov x1, {len}",
            "svc #0",
            ptr = in(reg) ptr,
            len = in(reg) len,
            out("x8") _, out("x0") _, out("x1") _,
            options(nostack)
        );
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    sys_write("Hello from User\n");
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
