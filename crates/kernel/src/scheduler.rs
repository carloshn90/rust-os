use core::ptr::{addr_of, addr_of_mut, null_mut};

use hal::klog;

use crate::{
    process::{Context, NUMBER_OF_PROCESS, PROC, Process, ProcessState},
    uart_logger::logger,
};

unsafe extern "C" {
    fn switch(old: *mut Context, new: *const Context);
}

pub struct Cpu {
    pub context: Context,
    pub process: *mut Process,
}

pub static mut CPU: Cpu = Cpu {
    process: null_mut(),
    context: Context { x: [0; 31], sp: 0 },
};

pub fn schedule() -> ! {
    logger().log("rustOS: Starting scheduler\n");
    loop {
        let mut found = false;
        for i in 0..NUMBER_OF_PROCESS {
            let p = unsafe { &mut PROC[i] };
            if p.state == ProcessState::RUNNABLE {
                found = true;
                klog!(
                    logger(),
                    "Process pid = {} in state = {:?}\n",
                    p.pid,
                    p.state
                );

                p.state = ProcessState::RUNNING;

                unsafe {
                    let cpu = addr_of_mut!(CPU);
                    (*cpu).process = p;
                    let old = addr_of_mut!((*cpu).context);
                    let new = addr_of!(p.context);
                    switch(old, new);
                }
            }
        }

        if !found {
            hal::halt::halt();
        }
    }
}

