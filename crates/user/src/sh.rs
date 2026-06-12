#![no_std]
#![no_main]

use core::panic::PanicInfo;

use common::print::{fprintf, sys_exec, sys_exit, sys_fork, sys_read_char, sys_wait};

const MAX_CMD: usize = 64;

/// Read one line from the console into `buf`, echoing each character.
/// Returns the number of bytes written (excluding the newline).
fn readline(buf: &mut [u8; MAX_CMD]) -> usize {
    let mut len = 0usize;
    loop {
        let c = sys_read_char();
        match c {
            // Enter / carriage-return — end of line
            b'\r' | b'\n' => {
                fprintf("\n");
                return len;
            }
            // Backspace (BS = 8) or DEL (127)
            8 | 127 => {
                if len > 0 {
                    len -= 1;
                    fprintf("\x08 \x08"); // move back, overwrite with space, move back again
                }
            }
            // Printable ASCII — echo and store
            32..=126 if len < MAX_CMD - 1 => {
                buf[len] = c;
                len += 1;
                // Echo the single character via sys_write
                let s = unsafe { core::str::from_utf8_unchecked(&buf[len - 1..len]) };
                fprintf(s);
            }
            _ => {}
        }
    }
}

/// Fork a child process and exec `cmd` inside it; the parent waits for it to finish.
fn run(cmd: &str) {
    let pid = sys_fork();
    if pid < 0 {
        fprintf("sh: fork failed\n");
        return;
    }

    if pid == 0 {
        // ── child ──
        if sys_exec(cmd) < 0 {
            fprintf("sh: exec failed: \n");
            fprintf(cmd);
            fprintf("\n");
        }
        sys_exit();
    } else {
        // ── parent ──
        sys_wait();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    loop {
        fprintf("# ");
        let mut buf = [0u8; MAX_CMD];
        let len = readline(&mut buf);
        if len == 0 {
            continue;
        }
        // SAFETY: readline only writes printable ASCII bytes
        let line = unsafe { core::str::from_utf8_unchecked(&buf[..len]) };
        // Strip an optional "./" prefix so both "help" and "./help" work
        let cmd = line.trim_start_matches("./");
        run(cmd);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
