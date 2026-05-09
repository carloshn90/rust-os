#![no_std]
#![no_main]

mod memory;
mod process;
pub mod scheduler;
mod uart_logger;
mod virtual_memory;

use core::panic::PanicInfo;

use hal::{klog, log::Logger};

core::arch::global_asm!(include_str!("boot.S"));
core::arch::global_asm!(include_str!("mmu.S"));
core::arch::global_asm!(include_str!("trampoline.S"));

use crate::{
    memory::k_me_init,
    process::{PageTableRoot, Process, ProcessManager, ProcessState, RunnableProcess},
    scheduler::Scheduler,
    uart_logger::{UartLogger, init_logging, logger},
    virtual_memory::{k_vm_init, k_vm_init_hart},
};

struct AProcess;
struct BProcess;

impl RunnableProcess for AProcess {
    fn run(&self) {
        logger().log("A\n");
    }
}

impl RunnableProcess for BProcess {
    fn run(&self) {
        logger().log("B\n");
    }
}

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

    let l0 = 0;
    let mut p = ProcessManager::new();

    let p_a = Process {
        process: Some(&AProcess),
        pid: p.alloc_pid(),
        state: ProcessState::RUNNABLE,
        user_pt: PageTableRoot { l0_phys: l0 },
    };
    let mut sch = Scheduler::init(p_a);

    let p_b = Process {
        process: Some(&BProcess),
        pid: p.alloc_pid(),
        state: ProcessState::RUNNABLE,
        user_pt: PageTableRoot { l0_phys: l0 },
    };

    sch.add(p_b);

    sch.scheduler();
}
