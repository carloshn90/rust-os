use core::mem::MaybeUninit;

use hal::klog;

use crate::{
    process::{Process, ProcessState},
    uart_logger::logger,
};
const MAX_NUMBER_OF_TASKS: usize = 10;

pub struct Scheduler {
    processes: [MaybeUninit<Process>; MAX_NUMBER_OF_TASKS],
    len: usize,
}

impl Scheduler {
    pub fn init(process: Process) -> Self {
        let mut ps = [const { MaybeUninit::<Process>::uninit() }; MAX_NUMBER_OF_TASKS];
        ps[0].write(process);
        Self {
            processes: ps,
            len: 1,
        }
    }

    pub fn add(&mut self, process: Process) {
        self.processes[self.len].write(process);
        self.len += 1;
    }

    pub fn scheduler(&mut self) -> ! {
        logger().log("rustOS: Starting scheduler\n");
        let mut found;
        loop {
            found = 0;
            for slot in &mut self.processes[..self.len] {
                let p = unsafe { slot.assume_init_mut() };
                if p.state == ProcessState::RUNNABLE {
                    klog!(logger(), "rustOS: running process pid={}\n", p.pid);
                    p.process.unwrap().run();
                    p.state = ProcessState::RUNNING;
                    found = 1
                }
            }

            if found == 0 {
                unsafe {
                    core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
                }
            }
        }
    }
}
