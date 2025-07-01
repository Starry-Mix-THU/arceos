#![no_std]

extern crate alloc;

use core::fmt;

use cfg_if::cfg_if;

/// A captured OS thread stack backtrace.
pub struct Backtrace {
    #[cfg(all(not(target_arch = "loongarch64"), feature = "unwinding"))]
    ips: alloc::vec::Vec<usize>,
}
impl Backtrace {
    /// Capture the current thread's stack backtrace.
    pub fn capture() -> Self {
        cfg_if! {
            if #[cfg(all(not(target_arch = "loongarch64"), feature = "unwinding"))] {
                // Use unwinding to capture the backtrace
                use alloc::vec::Vec;
                use unwinding::abi::{_Unwind_Backtrace, _Unwind_GetIP, UnwindContext, UnwindReasonCode};

                extern "C" fn callback(
                    unwind_ctx: &UnwindContext<'_>,
                    arg: *mut core::ffi::c_void,
                ) -> UnwindReasonCode {
                    let ips = unsafe { &mut *(arg as *mut Vec<usize>) };
                    ips.push(_Unwind_GetIP(unwind_ctx) - 4);
                    UnwindReasonCode::NO_REASON
                }
                let mut ips = Vec::new();
                _Unwind_Backtrace(callback, &mut ips as *mut _ as _);

                Self { ips }
            } else {
                // Fallback for platforms without unwinding support
                Self {}
            }
        }
    }
}

impl fmt::Display for Backtrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        cfg_if! {
            if #[cfg(target_arch = "loongarch64")] {
                writeln!(f, "<backtrace unsupported on this architecture>")
            } else if #[cfg(not(feature = "unwinding"))] {
                writeln!(f, "<unwinding disabled>")
            } else {
                writeln!(f, "Backtrace:")?;
                for (i, ip) in self.ips.iter().enumerate() {
                    writeln!(f, "{i}: 0x{ip:x}")?;
                }
                Ok(())
            }
        }
    }
}
