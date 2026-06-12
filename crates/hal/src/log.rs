use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

pub trait Logger {
    fn log(&self, s: &str);

    fn enabled(&self, _level: LogLevel) -> bool {
        true
    }

    fn log_fmt(&self, args: fmt::Arguments) {
        // Default implementation: write formatted output into the logger
        // Implementers can override for efficiency.
        struct Adapter<'a, L: ?Sized>(&'a L);

        impl<'a, L: Logger + ?Sized> fmt::Write for Adapter<'a, L> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.0.log(s);
                Ok(())
            }
        }

        let mut a = Adapter(self);
        let _ = fmt::write(&mut a, args);
    }

    fn log_level_fmt(&self, level: LogLevel, args: fmt::Arguments) {
        if self.enabled(level) {
            self.log_fmt(args);
        }
    }
}

/// Convenience macro (like println!, but for your Logger)
#[macro_export]
macro_rules! klog {
    ($logger:expr, $($arg:tt)*) => {{
        $logger.log_level_fmt($crate::log::LogLevel::Info, core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! kdebug {
    ($logger:expr, $($arg:tt)*) => {{
        $logger.log_level_fmt($crate::log::LogLevel::Debug, core::format_args!($($arg)*));
    }};
}

pub struct GroupedBin {
    value: usize,
    width: usize,
    group: usize,
}

impl GroupedBin {
    pub const fn new(value: usize, width: usize) -> Self {
        Self {
            value,
            width,
            group: 4,
        }
    }

    pub const fn with_group(value: usize, width: usize, group: usize) -> Self {
        Self {
            value,
            width,
            group,
        }
    }
}

impl fmt::Display for GroupedBin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.width == 0 || self.group == 0 {
            return Ok(());
        }

        for i in (0..self.width).rev() {
            if i != self.width - 1 && (i + 1) % self.group == 0 {
                f.write_str("_")?;
            }

            let bit = (self.value >> i) & 1;
            let ch = if bit == 1 { '1' } else { '0' };
            f.write_str(if ch == '1' { "1" } else { "0" })?;
        }

        Ok(())
    }
}

impl fmt::Binary for GroupedBin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
