#![no_std]

pub mod scheduler;

use hal::log::Logger;

use crate::scheduler::{Process, RunnableProcess, Scheduler, States};

struct AProcess;
struct BProcess;

impl RunnableProcess for AProcess {
    fn run(&self, logger: &dyn Logger) {
        logger.log("A\n");
    }
}

impl RunnableProcess for BProcess {
    fn run(&self, logger: &dyn Logger) {
        logger.log("B\n");
    }
}

pub fn kmain(logger: &dyn Logger) -> ! {
    logger.log("rustOS: kernel online\n");

    let p_a = Process {
        process: &AProcess,
        pid: 1,
        state: States::RUNNABLE,
    };
    let mut sch = Scheduler::init(p_a);

    let p_b = Process {
        process: &BProcess,
        pid: 2,
        state: States::RUNNABLE,
    };
    sch.add(p_b);

    sch.scheduler(logger);
}
