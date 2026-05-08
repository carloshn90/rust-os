pub trait RunnableProcess {
    fn run(&self);
}

#[derive(PartialEq, Eq)]
pub enum ProcessState {
    RUNNABLE,
    RUNNING,
}

#[derive(Copy, Clone)]
pub struct PageTableRoot {
    pub l0_phys: u64,
}

pub struct Process {
    pub process: Option<&'static dyn RunnableProcess>,
    pub pid: u8,
    pub state: ProcessState,
    pub user_pt: PageTableRoot,
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
            user_pt: PageTableRoot { l0_phys: 0 },
        }
    }
}
