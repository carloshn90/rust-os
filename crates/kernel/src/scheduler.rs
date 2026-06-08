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
    pub intena: i32, // Were interrupts enabled before push_off()?
}

pub static mut CPU: Cpu = Cpu {
    process: null_mut(),
    context: Context { x: [0; 31], sp: 0 },
    intena: 0,
};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WaitChannel {
    DiskFree,
    DiskBuf(usize),
    PipeRead(usize),
    PipeWrite(usize),
    Child(usize),
}

pub fn schedule() -> ! {
    loop {
        let mut found = false;
        logger().log("[schedule] cpu0: scanning process table...\n");
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
                    let cpu = my_cpu();
                    (*cpu).process = p;
                    let old = addr_of_mut!((*cpu).context);
                    let new = addr_of!(p.context);
                    switch(old, new);
                    logger().log("finishing\n");
                }
            }
        }

        if !found {
            unsafe {
                core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
            }
        }
    }
}

#[inline]
pub fn chan<T>(x: &T) -> *const () {
    (x as *const T).cast::<()>()
}

pub fn sleep(chan: WaitChannel) {
    let p = my_proc();
    unsafe {
        (*p).chan = Some(chan);
        (*p).state = ProcessState::SLEEPING;
    }

    sched();

    unsafe {
        (*p).chan = None;
    }
}

pub fn wake_up(chan: WaitChannel) {
    for i in 0..NUMBER_OF_PROCESS {
        let p = unsafe { &mut PROC[i] };

        if p.state == ProcessState::SLEEPING && p.chan == Some(chan) {
            p.state = ProcessState::RUNNABLE;
        }
    }
}

pub fn my_proc() -> *mut Process {
    let cpu = my_cpu();
    unsafe { (*cpu).process }
}

fn sched() {
    let cpu = my_cpu();
    let p = my_proc();

    if p.is_null() {
        panic!("[sched]: no current process");
    }

    unsafe {
        if (*p).state == ProcessState::RUNNING {
            panic!("[Sched] RUNNING");
        }

        let intena = (*cpu).intena;
        let old = addr_of_mut!((*p).context);
        let new = addr_of!((*cpu).context);
        switch(old, new);
        (*cpu).intena = intena;
    }
}

fn my_cpu() -> *mut Cpu {
    core::ptr::addr_of_mut!(CPU)
}
