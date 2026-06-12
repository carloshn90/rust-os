use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use hal::log::{LogLevel, Logger};

// QEMU `virt` PL011 UART base.
pub const UART0_BASE: u64 = 0x0900_0000;

pub struct UartLogger;

static UART_LOGGER: UartLogger = UartLogger;
static LOGGING_INITIALISED: AtomicBool = AtomicBool::new(false);
static MIN_LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

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

    fn getc() -> u8 {
        // FR (0x18) bit4 = RXFE (receive FIFO empty) — spin until a byte arrives
        while (Self::mmio_read(0x18) & (1 << 4)) != 0 {}
        (Self::mmio_read(0x00) & 0xFF) as u8
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

    fn enabled(&self, level: LogLevel) -> bool {
        (level as u8) >= MIN_LOG_LEVEL.load(Ordering::Relaxed)
    }
}

pub fn init_logging() {
    LOGGING_INITIALISED.store(true, Ordering::Relaxed);
}

pub fn logger() -> &'static dyn Logger {
    &UART_LOGGER
}

pub fn set_min_log_level(level: LogLevel) {
    MIN_LOG_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Read one byte from the PL011 UART (blocking).
pub fn uart_getc() -> u8 {
    UartLogger::getc()
}
