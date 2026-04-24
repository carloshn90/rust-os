use core::fmt;

pub trait Logger {
    fn log(&self, s: &str);

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
}

/// Convenience macro (like println!, but for your Logger)
#[macro_export]
macro_rules! klog {
    ($logger:expr, $($arg:tt)*) => {{
        $logger.log_fmt(core::format_args!($($arg)*));
    }};
}
