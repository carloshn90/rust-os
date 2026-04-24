// QEMU `virt` PL011 UART base.
const UART0_BASE: usize = 0x0900_0000;

pub struct UartLogger;

impl UartLogger {
    #[inline(always)]
    fn mmio_write(offset: usize, val: u32) {
        unsafe { core::ptr::write_volatile((UART0_BASE + offset) as *mut u32, val) }
    }

    #[inline(always)]
    fn mmio_read(offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((UART0_BASE + offset) as *const u32) }
    }

    fn putc(c: u8) {
        // FR (0x18) bit5 = TXFF (transmit FIFO full)
        while (Self::mmio_read(0x18) & (1 << 5)) != 0 {}
        Self::mmio_write(0x00, c as u32);
    }

    pub(crate) fn puts(s: &str) {
        for &b in s.as_bytes() {
            if b == b'\n' {
                Self::putc(b'\r');
            }
            Self::putc(b);
        }
    }
}

impl hal::log::Logger for UartLogger {
    fn log(&self, s: &str) {
        UartLogger::puts(s);
    }
}
