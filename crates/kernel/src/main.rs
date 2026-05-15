#![no_std]
#![no_main]

mod disk;
mod memory;
mod process;
pub mod scheduler;
mod uart_logger;
mod virtio_disk;
mod virtual_memory;

use core::panic::PanicInfo;

use hal::{klog, log::Logger};

core::arch::global_asm!(include_str!("boot.S"));
core::arch::global_asm!(include_str!("mmu.S"));
core::arch::global_asm!(include_str!("trampoline.S"));
core::arch::global_asm!(include_str!("switch.S"));

use crate::{
    memory::k_me_init,
    process::{proc_init, user_init},
    scheduler::schedule,
    uart_logger::{UartLogger, init_logging, logger},
    virtio_disk::virtio_disk_init,
    virtual_memory::{k_vm_init, k_vm_init_hart},
};

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    k_main();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let logger = UartLogger;
    logger.log("rustOS: PANIC: ");

    if let Some(location) = info.location() {
        klog!(
            logger,
            "at {}:{}:{}: {}\n",
            location.file(),
            location.line(),
            location.column(),
            info.message()
        );
    } else {
        klog!(logger, "{}\n", info.message());
    }
    loop {
        hal::halt::halt();
    }
}

pub fn k_main() -> ! {
    init_logging();
    logger().log("rustOS: arrch64 QEMU virt boot Ok\n");
    k_me_init();
    k_vm_init().expect("failed to map kernel pages");
    k_vm_init_hart();
    logger().log("rustOS: MMU enabled\n");

    virtio_disk_init();

    proc_init();
    user_init();

    schedule();
}
