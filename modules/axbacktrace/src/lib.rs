#![no_std]

extern crate alloc;

use core::{
    fmt,
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(feature = "dwarf")]
mod dwarf;

#[cfg(feature = "dwarf")]
pub use dwarf::{DwarfReader, FrameIter, init};

#[cfg(not(feature = "dwarf"))]
pub fn init() {}

/// Represents a single stack frame in the unwound stack.
#[repr(C)]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct Frame {
    /// The frame pointer of the previous stack frame.
    pub fp: usize,
    /// The instruction pointer (program counter) after the function call.
    pub ip: usize,
}

impl Frame {
    // See https://github.com/rust-lang/backtrace-rs/blob/b65ab935fb2e0d59dba8966ffca09c9cc5a5f57c/src/symbolize/mod.rs#L145
    pub fn adjust_ip(&self) -> usize {
        self.ip.wrapping_sub(1)
    }
}

/// Unwind the current thread's stack and call the provided visitor function for
/// each stack frame. The visitor function receives a reference to a
/// `StackFrame` and should return `true` to continue unwinding or `false` to
/// stop unwinding.
pub fn unwind_stack(mut fp: usize, mut visitor: impl FnMut(&Frame) -> bool) {
    let offset = if cfg!(target_arch = "x86_64") || cfg!(target_arch = "aarch64") {
        0
    } else {
        1
    };

    unsafe extern "C" {
        safe static _stext: [u8; 0];
        safe static _etext: [u8; 0];
        safe static _edata: [u8; 0];
    }

    let ip_range = Range {
        start: _stext.as_ptr() as usize,
        end: _etext.as_ptr() as usize,
    };

    use axconfig::plat::{PHYS_MEMORY_BASE, PHYS_MEMORY_SIZE, PHYS_VIRT_OFFSET};
    let fp_range = Range {
        start: _edata.as_ptr() as usize,
        end: PHYS_MEMORY_BASE + PHYS_MEMORY_SIZE + PHYS_VIRT_OFFSET,
    };

    while fp > 0 && fp % align_of::<usize>() == 0 && fp_range.contains(&fp) {
        let frame: &Frame = unsafe { &*(fp as *const Frame).sub(offset) };

        if !ip_range.contains(&frame.ip) {
            break;
        }

        if !visitor(frame) {
            break;
        }

        if let Some(large_stack_end) = fp.checked_add(8 * 1024 * 1024) {
            if frame.fp >= large_stack_end {
                break;
            }
        }
        fp = frame.fp;
    }
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
    cfg!(feature = "dwarf")
}

#[allow(dead_code)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
enum Inner {
    Unsupported,
    Disabled,
    #[cfg(feature = "dwarf")]
    Captured(alloc::vec::Vec<Frame>),
}

/// A captured OS thread stack backtrace.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
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
        #[cfg(not(feature = "dwarf"))]
        {
            return Self {
                inner: Inner::Disabled,
            };
        }
        #[cfg(feature = "dwarf")]
        {
            use core::arch::asm;

            let mut fp: usize;
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "x86_64")] {
                    unsafe { asm!("mov {ptr}, rbp", ptr = out(reg) fp) };
                } else if #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))] {
                    unsafe { asm!("addi {ptr}, s0, 0", ptr = out(reg) fp) };
                } else if #[cfg(target_arch = "aarch64")] {
                    unsafe { asm!("mov {ptr}, x29", ptr = out(reg) fp) };
                } else if #[cfg(target_arch = "loongarch64")] {
                    unsafe { asm!("move {ptr}, $fp", ptr = out(reg) fp) };
                } else {
                    return Self {
                        inner: Inner::Unsupported,
                    };
                }
            }

            let mut frames = alloc::vec![];
            let mut depth = 0;
            let max_depth = max_depth();
            unwind_stack(fp, |frame| {
                depth += 1;
                if depth > max_depth {
                    return false;
                }
                frames.push(*frame);
                true
            });

            Self {
                inner: Inner::Captured(frames),
            }
        }
    }

    /// Visit each stack frame in the captured backtrace in order.
    ///
    /// Returns `None` if the backtrace is not captured.
    #[cfg(feature = "dwarf")]
    pub fn frames<'a>(&'a self) -> Option<FrameIter<'a>> {
        let Inner::Captured(capture) = &self.inner else {
            return None;
        };

        Some(FrameIter::new(capture))
    }
}

impl fmt::Display for Backtrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            Inner::Unsupported => {
                writeln!(f, "<unwinding unsupported>")
            }
            Inner::Disabled => {
                writeln!(f, "<backtrace disabled>")
            }
            #[cfg(feature = "dwarf")]
            Inner::Captured(frames) => {
                writeln!(f, "Backtrace:")?;
                dwarf::fmt_frames(f, frames)
            }
        }
    }
}

impl fmt::Debug for Backtrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
