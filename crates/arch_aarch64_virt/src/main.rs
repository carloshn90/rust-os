#![no_std]
#![no_main]

mod uart_logger;

use core::panic::PanicInfo;
use hal::log::Logger;
use kernel;

core::arch::global_asm!(include_str!("boot.S"));

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    let logger = uart_logger::UartLogger;
    logger.log("rustOS: arrch64 QEMU virt boot Ok\n");
    kernel::kmain(&logger);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let logger = uart_logger::UartLogger;
    logger.log("rustOS: PANIC\n");
    loop {
        hal::halt::halt();
    }
}
