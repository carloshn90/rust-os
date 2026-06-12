use crate::system_call::{SYS_EXEC, SYS_EXIT, SYS_FORK, SYS_READ, SYS_WAIT, SYS_WRITE};

pub fn fprintf(s: &str) {
    sys_write(s);
}

pub fn sys_write(s: &str) {
    let ptr = s.as_ptr() as u64;
    let len = s.len() as u64;

    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") SYS_WRITE,
            in("x0") ptr,
            in("x1") len,
            lateout("x8") _,
            lateout("x0") _,
            lateout("x1") _,
            options(nostack)
        );
    }
}

/// Read one character from the console (blocking).
pub fn sys_read_char() -> u8 {
    let c: u64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") SYS_READ,
            lateout("x0") c,
            options(nostack),
        );
    }
    c as u8
}

/// Terminate the calling process.
pub fn sys_exit() -> ! {
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") SYS_EXIT,
            options(nostack, noreturn),
        );
    }
}

/// Fork the current process.
/// Returns the child PID in the parent, 0 in the child, or -1 on error.
pub fn sys_fork() -> i64 {
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") SYS_FORK,
            lateout("x0") ret,
            options(nostack),
        );
    }
    ret as i64
}

/// Replace the calling process image with the named program (e.g. "help").
/// Returns -1 on error; on success the process is replaced and never returns.
pub fn sys_exec(name: &str) -> i64 {
    let ptr = name.as_ptr() as u64;
    let len = name.len() as u64;
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") SYS_EXEC,
            in("x0") ptr,
            in("x1") len,
            lateout("x0") ret,
            lateout("x1") _,
            options(nostack),
        );
    }
    ret as i64
}

/// Wait for any child process to exit. Returns the child PID, or -1 on error.
pub fn sys_wait() -> i64 {
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") SYS_WAIT,
            lateout("x0") ret,
            options(nostack),
        );
    }
    ret as i64
}
