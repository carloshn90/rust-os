#[inline(always)]
pub fn halt() {
    unsafe {
        // Use WFI (wait-for-interrupt) so we reliably sleep until the next IRQ.
        // WFE can return immediately if an event is already latched.
        core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
    }

    loop {}
}
