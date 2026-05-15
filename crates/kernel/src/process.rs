use core::sync::atomic::{AtomicU8, Ordering};

use hal::klog;

use crate::{
    memory::{PAGE_SIZE, k_alloc},
    scheduler::CPU,
    uart_logger::logger,
    virtual_memory::{k_stack, proc_page_table},
};

pub const NUMBER_OF_PROCESS: usize = 10;

pub const EMPTY_PROCESS: Process = Process {
    pid: 0,
    state: ProcessState::UNUSED,
    k_stack: 0,
    page_table: 0,
    context: Context { x: [0; 31], sp: 0 },
    trap_frame: 0,
};

#[unsafe(no_mangle)]
pub extern "C" fn forkret() {
    unsafe {
        let cpu = core::ptr::addr_of_mut!(CPU);
        let process = (*cpu).process;

        if process.is_null() {
            panic!("forkret: no current process");
        }

        klog!(logger(), "pid = {}, return\n", (*process).pid);
    }
    hal::halt::halt();
}

#[derive(Copy, Clone)]
pub struct Process {
    pub pid: u8,             // Process ID
    pub state: ProcessState, // Process state
    pub k_stack: u64,        // Virtual address of kernel stack
    pub page_table: u64,     // User page table
    pub context: Context,    // swtch() here to run process
    pub trap_frame: u64,     // data page for trampoline.S
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct Context {
    pub x: [u64; 31], // x0..x30
    pub sp: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcessState {
    RUNNABLE,
    RUNNING,
    USED,
    UNUSED,
}

pub static mut PROC: [Process; NUMBER_OF_PROCESS] = [EMPTY_PROCESS; NUMBER_OF_PROCESS];

static NEXT_PID: AtomicU8 = AtomicU8::new(1);

pub fn proc_init() {
    for i in 0..NUMBER_OF_PROCESS {
        let p = unsafe { &mut PROC[i] };
        p.state = ProcessState::UNUSED;
        p.k_stack = k_stack(i);
    }
}

pub fn user_init() {
    let p = &mut alloc_proc();

    p.state = ProcessState::RUNNABLE;
}

#[inline]
fn fn_addr(f: extern "C" fn()) -> u64 {
    f as *const () as usize as u64
}

fn alloc_proc() -> &'static mut Process {
    let p = first_unused_proc().unwrap();

    let tf = k_alloc();
    if tf.is_null() {
        panic!("k_alloc");
    }
    p.pid = alloc_pid();
    p.trap_frame = tf as u64;
    p.page_table = proc_page_table(p).unwrap();
    p.context = Context::default();
    p.context.sp = (p.k_stack + PAGE_SIZE as u64) & !0xF;
    p.context.x[30] = fn_addr(forkret);
    p.state = ProcessState::USED;

    p
}

fn first_unused_proc() -> Option<&'static mut Process> {
    unsafe {
        for i in 0..NUMBER_OF_PROCESS {
            let p = &mut PROC[i];
            if p.state == ProcessState::UNUSED {
                return Some(p);
            }
        }
        None
    }
}

fn alloc_pid() -> u8 {
    NEXT_PID.fetch_add(1, Ordering::Relaxed)
}

// free a proc structure and the data hanging from it,
// including user pages.
#[allow(dead_code)]
fn freeproc(p: &mut Process) {
    p.trap_frame = 0;
    p.page_table = 0;
    p.pid = 0;
    p.context = Context::default();
    p.state = ProcessState::UNUSED;
}
