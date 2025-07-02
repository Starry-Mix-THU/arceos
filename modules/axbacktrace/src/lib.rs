#![no_std]

extern crate alloc;

use core::{
    arch::asm,
    fmt,
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{sync::Arc, vec::Vec};
use kspin::SpinNoIrq;

#[doc(hidden)]
pub use cfg_if::cfg_if;

/// Represents a single stack frame in the unwound stack.
#[repr(C)]
#[derive(Debug)]
pub struct StackFrame {
    /// The frame pointer of the previous stack frame.
    pub fp: usize,
    /// The instruction pointer (program counter) at the time of the function
    /// call.
    pub ip: usize,
}
impl StackFrame {
    pub fn call_pc(&self) -> usize {
        self.ip - 4
    }
}

#[macro_export]
macro_rules! read_frame_pointer {
    () => {{
        use core::arch::asm;

        let mut fp: usize;
        $crate::cfg_if! {
            if #[cfg(target_arch = "x86_64")] {
                unsafe { asm!("mov {ptr}, rbp", ptr = out(reg) fp) };
            } else if #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))] {
                unsafe { asm!("addi {ptr}, s0, 0", ptr = out(reg) fp) };
            } else if #[cfg(target_arch = "aarch64")] {
                unsafe { asm!("mov {ptr}, x29", ptr = out(reg) fp) };
            } else if #[cfg(target_arch = "loongarch64")] {
                unsafe { asm!("move {ptr}, $fp", ptr = out(reg) fp) };
            } else {
                return None;
            }
        }

        Some(fp)
    }};
}

/// Unwind the current thread's stack and call the provided visitor function for
/// each stack frame. The visitor function receives a reference to a
/// `StackFrame` and should return `true` to continue unwinding or `false` to
/// stop unwinding.
///
/// # Safety
///
/// Calling this function with unbounded visitor functions can lead to reading
/// invalid memory. It is the caller's responsibility to ensure that the visitor
/// function returns `false` when it encounters an invalid stack frame pointer.
pub unsafe fn unwind_stack(mut fp: usize, mut visitor: impl FnMut(&StackFrame) -> bool) {
    let offset = if cfg!(target_arch = "x86_64") || cfg!(target_arch = "aarch64") {
        0
    } else {
        1
    };

    while fp > 0 {
        if fp % align_of::<usize>() != 0 {
            break;
        }

        let stack: *const StackFrame = unsafe { (fp as *const StackFrame).sub(offset) };
        let frame = unsafe { &*stack };
        if !visitor(frame) {
            break;
        }

        fp = frame.fp;
    }
}

/// Configuration for capturing a stack backtrace.
#[derive(Debug, Clone)]
pub struct BacktraceConfig {
    /// The range of stack addresses to consider valid for unwinding.
    pub fp_range: Range<usize>,
    /// The range of instruction addresses to consider valid for unwinding.
    pub ip_range: Range<usize>,
}

static GLOBAL_CONFIG: SpinNoIrq<Option<Arc<BacktraceConfig>>> = SpinNoIrq::new(None);

/// Sets the global configuration for capturing stack backtraces.
///
/// See [`Backtrace::capture`].
pub fn set_global_config(config: BacktraceConfig) {
    *GLOBAL_CONFIG.lock() = Some(Arc::new(config));
}

static MAX_DEPTH: AtomicUsize = AtomicUsize::new(16);

/// Sets the maximum depth for stack unwinding.
pub fn set_max_depth(depth: usize) {
    if depth > 0 {
        MAX_DEPTH.store(depth, Ordering::Relaxed);
    }
}
/// Returns the maximum depth for stack unwinding.
pub fn max_depth() -> usize {
    MAX_DEPTH.load(Ordering::Relaxed)
}

/// Returns whether the backtrace feature is enabled.
pub const fn is_enabled() -> bool {
    cfg!(feature = "enable")
}

#[allow(dead_code)]
enum Inner {
    Unsupported,
    Disabled,
    Captured(Vec<usize>),
}

/// A captured OS thread stack backtrace.
pub struct Backtrace {
    inner: Inner,
}

impl Backtrace {
    /// Capture the current thread's stack backtrace.
    ///
    /// When global configuration is not set, a dummy backtrace is returned.
    ///
    /// # Safety
    ///
    /// It's the caller's responsibility to ensure that a proper configuration is set
    /// by calling [`set_global_config`].
    pub fn capture() -> Self {
        if !is_enabled() {
            return Self {
                inner: Inner::Disabled,
            };
        }

        let Some(config) = GLOBAL_CONFIG.lock().clone() else {
            return Self {
                inner: Inner::Disabled,
            };
        };
        unsafe { Self::capture_with(&config) }
    }

    /// Capture the current thread's stack backtrace with given configuration.
    pub unsafe fn capture_with(config: &BacktraceConfig) -> Self {
        if !is_enabled() {
            return Self {
                inner: Inner::Disabled,
            };
        }

        let Some(fp) = read_frame_pointer!() else {
            return Self {
                inner: Inner::Unsupported,
            };
        };

        let mut ips = Vec::new();
        let mut depth = 0;
        let max_depth = max_depth();
        unsafe {
            unwind_stack(fp, |frame| {
                depth += 1;
                if depth > max_depth
                    || !config.fp_range.contains(&frame.fp)
                    || !config.ip_range.contains(&frame.ip)
                {
                    return false;
                }
                ips.push(frame.call_pc());
                true
            });
        }
        Self {
            inner: Inner::Captured(ips),
        }
    }
}

impl fmt::Display for Backtrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            Inner::Unsupported => {
                writeln!(f, "<backtrace unsupported>")
            }
            Inner::Disabled => {
                writeln!(f, "<unwinding disabled>")
            }
            Inner::Captured(capture) => {
                writeln!(f, "Backtrace:")?;
                for ip in capture {
                    writeln!(f, "  0x{ip:x}")?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Debug for Backtrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
