use core::sync::atomic::{AtomicBool, Ordering};

use hal::log::Logger;

// QEMU `virt` PL011 UART base.
pub const UART0_BASE: u64 = 0x0900_0000;

pub struct UartLogger;

static UART_LOGGER: UartLogger = UartLogger;
static LOGGING_INITIALISED: AtomicBool = AtomicBool::new(false);

impl UartLogger {
    #[inline(always)]
    fn mmio_write(offset: u64, val: u32) {
        unsafe { core::ptr::write_volatile((UART0_BASE + offset) as *mut u32, val) }
    }

    #[inline(always)]
    fn mmio_read(offset: u64) -> u32 {
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
        if LOGGING_INITIALISED.load(Ordering::Relaxed) {
            UartLogger::puts(s);
        } else {
            panic!("Error loging");
        }
    }
}

pub fn init_logging() {
    LOGGING_INITIALISED.store(true, Ordering::Relaxed);
}

pub fn logger() -> &'static dyn Logger {
    &UART_LOGGER
}
