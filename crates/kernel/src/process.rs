pub const NUMBER_OF_PROCESS: usize = 10;

#[derive(Copy, Clone)]
pub struct Process {
    pub process: Option<&'static dyn RunnableProcess>,
    pub pid: u8,
    pub state: ProcessState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    RUNNABLE,
    RUNNING,
    UNUSED,
}

const EMPTY_PROCESS: Process = Process {
    process: None,
    pid: 0,
    state: ProcessState::UNUSED,
};

pub static mut PROC: [Process; NUMBER_OF_PROCESS] = [EMPTY_PROCESS; NUMBER_OF_PROCESS];

pub trait RunnableProcess {
    fn run(&self);
}

pub struct ProcessManager {
    next_pid: u8,
}

#[allow(dead_code)]
impl ProcessManager {
    pub fn new() -> Self {
        Self { next_pid: 1 }
    }

    pub fn alloc_pid(&mut self) -> u8 {
        let pid = self.next_pid;
        self.next_pid = self.next_pid.wrapping_add(1);
        pid
    }

    pub fn poc_init(&mut self) -> Process {
        Process {
            process: None,
            pid: self.alloc_pid(),
            state: ProcessState::RUNNABLE,
        }
    }
}
