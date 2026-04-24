use core::mem::MaybeUninit;

use hal::{klog, log::Logger};

const MAX_NUMBER_OF_TASKS: usize = 10;

pub trait RunnableProcess {
    fn run(&self, logger: &dyn Logger);
}

#[derive(PartialEq, Eq)]
pub enum States {
    RUNNABLE,
    RUNNING,
}

pub struct Process {
    pub process: &'static dyn RunnableProcess,
    pub pid: u8,
    pub state: States,
}

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

    pub fn scheduler(&mut self, logger: &dyn Logger) -> ! {
        logger.log("rustOS: Starting scheduler\n");
        let mut found;
        loop {
            found = 0;
            for slot in &mut self.processes[..self.len] {
                let p = unsafe { slot.assume_init_mut() };
                if p.state == States::RUNNABLE {
                    klog!(logger, "rustOS: running process pid={}\n", p.pid);
                    p.process.run(logger);
                    p.state = States::RUNNING;
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
