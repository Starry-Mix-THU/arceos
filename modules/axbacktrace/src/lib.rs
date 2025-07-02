#![no_std]

extern crate alloc;

#[cfg(feature = "addr2line")]
mod dwarf;

#[cfg(feature = "addr2line")]
use core::{
    arch::asm,
    fmt,
    ops::Range,
    slice,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{sync::Arc, vec::Vec};
use kspin::SpinNoIrq;

#[doc(hidden)]
pub use cfg_if::cfg_if;

#[cfg(feature = "addr2line")]
pub use dwarf::{DwarfError, DwarfReader, set_dwarf_sections};

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
    // See https://github.com/rust-lang/backtrace-rs/blob/b65ab935fb2e0d59dba8966ffca09c9cc5a5f57c/src/symbolize/mod.rs#L145
    pub fn adjust_ip(&self) -> usize {
        self.ip.wrapping_sub(1)
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

        if let Some(large_stack_end) = fp.checked_add(8 * 1024 * 1024) {
            if frame.fp >= large_stack_end {
                break;
            }
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
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
enum Inner {
    Unsupported,
    Disabled,
    Captured(Vec<usize>),
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
                ips.push(frame.adjust_ip());
                true
            });
        }
        Self {
            inner: Inner::Captured(ips),
        }
    }

    /// Visit each stack frame in the captured backtrace in order.
    ///
    /// Returns `None` if DWARF is not available or the backtrace is not
    /// captured.
    #[cfg(feature = "addr2line")]
    pub fn frames<'a>(&'a self) -> Option<Frames<'a>> {
        let Inner::Captured(capture) = &self.inner else {
            return None;
        };

        let context = unsafe {
            #[allow(static_mut_refs)]
            dwarf::CONTEXT.clone()?
        };

        Some(Frames {
            context,
            inner: capture.iter(),
        })
    }
}

/// An "iterator" over the stack frames in a captured backtrace. Note that it
/// does not implement `core::iter::Iterator` because the returned frames
/// reference the iterator itself.
///
/// See [`Backtrace::frames`].
pub struct Frames<'a> {
    context: Arc<addr2line::Context<DwarfReader>>,
    inner: slice::Iter<'a, usize>,
}
impl Frames<'_> {
    /// Returns the next batch of stack frames from the captured backtrace.
    ///
    /// Each batch contains a set of frames that can be iterated over.
    pub fn next_batch(
        &mut self,
    ) -> Option<Result<addr2line::FrameIter<DwarfReader>, gimli::Error>> {
        let ip = self.inner.next()?;
        Some(self.context.find_frames(*ip as _).skip_all_loads())
    }
}

#[cfg(feature = "addr2line")]
fn fmt_frame<R: gimli::Reader>(
    f: &mut fmt::Formatter<'_>,
    frame: &addr2line::Frame<R>,
) -> fmt::Result {
    use alloc::borrow::Cow;

    let func = frame
        .function
        .as_ref()
        .and_then(|func| func.demangle().ok())
        .unwrap_or(Cow::Borrowed("<unknown>"));
    writeln!(f, ": {func}")?;

    let Some(location) = &frame.location else {
        return Ok(());
    };
    write!(f, "            at ")?;

    let Some(file) = &location.file else {
        return write!(f, "??");
    };
    write!(f, "{file}")?;
    let Some(line) = location.line else {
        return Ok(());
    };
    write!(f, ":{line}")?;
    let Some(col) = location.column else {
        return Ok(());
    };
    write!(f, ":{col}")?;

    writeln!(f)?;

    Ok(())
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
                #[cfg(feature = "addr2line")]
                if let Some(mut frames) = self.frames() {
                    let mut cnt = 0;
                    while let Some(batch) = frames.next_batch() {
                        let Ok(mut batch) = batch else {
                            continue;
                        };
                        while let Ok(Some(frame)) = batch.next() {
                            write!(f, "{cnt:>4}")?;
                            fmt_frame(f, &frame)?;
                            cnt += 1;
                        }
                    }
                    return Ok(());
                }
                for ip in capture {
                    writeln!(f, "  {ip:#x}")?;
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
