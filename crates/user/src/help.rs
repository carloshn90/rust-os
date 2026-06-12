#![no_std]
#![no_main]

use core::panic::PanicInfo;

use common::print::{fprintf, sys_exit};

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    fprintf("help: available commands will be listed here\n");
    sys_exit();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
