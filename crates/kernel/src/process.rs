use core::sync::atomic::{AtomicU8, Ordering, fence};

use hal::klog;

use crate::{
    exec::k_exec,
    memory::{PAGE_SIZE, k_alloc},
    scheduler::{WaitChannel, my_proc, sched, sleep, wake_up},
    uart_logger::logger,
    virtual_memory::{KSTACK_SIZE, clone_user_space, k_stack, proc_page_table},
};

pub const NUMBER_OF_PROCESS: usize = 10;

pub const EMPTY_PROCESS: Process = Process {
    pid: 0,
    parent_pid: None,
    state: ProcessState::UNUSED,
    k_stack: 0,
    page_table: 0,
    context: Context { x: [0; 31], sp: 0 },
    trap_frame: core::ptr::null_mut(),
    chan: None,
};

unsafe extern "C" {
    pub fn user_trap_return(tf: *mut TrapFrame) -> !;
}

#[unsafe(no_mangle)]
pub extern "C" fn forkret() {
    unsafe {
        let process = my_proc();

        if process.is_null() {
            panic!("[forkret]: no current process");
        }

        klog!(logger(), "pid = {}, return\n", (*process).pid);

        let tf = k_exec("sh", &mut (*process)).expect("[forkret] k_exec fail");

        logger().log("[forkret] jumping into hello world bin\n");

        fence(Ordering::SeqCst);
        user_trap_return(tf);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fork_user_return() {
    unsafe {
        let process = my_proc();

        if process.is_null() {
            panic!("[fork_user_return]: no current process");
        }

        let tf = (*process).trap_frame;
        fence(Ordering::SeqCst);
        user_trap_return(tf);
    }
}

#[repr(C)]
pub struct TrapFrame {
    pub regs: [u64; 31], // x0 to x30 (31 * 8 = 248 bytes)
    pub spsr: u64,       // Offset 248
    pub epc: u64,        // Offset 256
    pub sp: u64,         // Offset 264
    pub ttbr0: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Process {
    pub pid: u8,                    // Process ID
    pub parent_pid: Option<u8>,     // Parent process ID
    pub state: ProcessState,        // Process state
    pub k_stack: u64,               // Virtual address of kernel stack
    pub page_table: u64,            // User page table
    pub context: Context,           // swtch() here to run process
    pub trap_frame: *mut TrapFrame, // data page for trampoline.S
    pub chan: Option<WaitChannel>,  // If some, sleeping on chan
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
    SLEEPING,
    ZOMBIE,
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

    let tf_raw = k_alloc();
    if tf_raw.is_null() {
        panic!("k_alloc");
    }

    let tf = tf_raw as *mut TrapFrame;
    unsafe {
        core::ptr::write_bytes(tf as *mut u8, 0, PAGE_SIZE);
    }
    p.pid = alloc_pid();
    p.trap_frame = tf;
    p.page_table = proc_page_table(p).unwrap();
    p.context = Context::default();
    p.context.sp = (p.k_stack + KSTACK_SIZE) & !0xF;
    p.context.x[30] = fn_addr(forkret);
    p.parent_pid = None;
    p.state = ProcessState::USED;

    p
}

pub fn fork_current(saved_ctx: *const u64, user_sp: u64) -> Result<u8, &'static str> {
    let parent = my_proc();
    if parent.is_null() {
        return Err("no current process");
    }

    let child = alloc_proc();
    child.parent_pid = Some(unsafe { (*parent).pid });

    clone_user_space(
        unsafe { (*parent).page_table as *mut _ },
        child.page_table as *mut _,
    )
    .map_err(|_| "clone_user_space failed")?;

    unsafe {
        let tf = &mut *child.trap_frame;
        for reg in 0..31 {
            tf.regs[reg] = *saved_ctx.add(reg);
        }
        tf.regs[0] = 0;
        tf.sp = user_sp;
        tf.epc = *saved_ctx.add(32);
        tf.spsr = *saved_ctx.add(33);
        tf.ttbr0 = child.page_table;
    }

    child.context = Context::default();
    child.context.sp = (child.k_stack + KSTACK_SIZE) & !0xF;
    child.context.x[30] = fn_addr(fork_user_return);
    child.state = ProcessState::RUNNABLE;

    Ok(child.pid)
}

pub fn wait_current() -> Result<u8, &'static str> {
    let current = my_proc();
    if current.is_null() {
        return Err("no current process");
    }

    let current_pid = unsafe { (*current).pid };
    loop {
        let mut has_children = false;

        unsafe {
            for index in 0..NUMBER_OF_PROCESS {
                let process = &mut PROC[index];
                if process.parent_pid != Some(current_pid) {
                    continue;
                }

                has_children = true;
                if process.state == ProcessState::ZOMBIE {
                    let pid = process.pid;
                    freeproc(process);
                    return Ok(pid);
                }
            }
        }

        if !has_children {
            return Err("no children");
        }

        sleep(WaitChannel::Child(current_pid as usize));
    }
}

pub fn exit_current() -> ! {
    let current = my_proc();
    if current.is_null() {
        panic!("[exit_current]: no current process");
    }

    unsafe {
        if let Some(parent_pid) = (*current).parent_pid {
            wake_up(WaitChannel::Child(parent_pid as usize));
        }
        (*current).state = ProcessState::ZOMBIE;
    }

    sched();
    panic!("[exit_current]: zombie resumed");
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
    p.trap_frame = core::ptr::null_mut();
    p.page_table = 0;
    p.pid = 0;
    p.parent_pid = None;
    p.context = Context::default();
    p.state = ProcessState::UNUSED;
}
