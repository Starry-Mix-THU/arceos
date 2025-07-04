//! Memory mapping backends.

use core::ops::Deref;

use ::alloc::{sync::Arc, vec::Vec};
use axhal::paging::{MappingFlags, PageSize, PageTable};
use memory_addr::{PhysAddr, VirtAddr};
use memory_set::MappingBackend;

mod alloc;
mod linear;
mod shared;

#[allow(unused_imports)]
pub(crate) use alloc::{alloc_frame, dealloc_frame};

pub struct SharedPages {
    pub phys_pages: Vec<PhysAddr>,
    pub align: PageSize,
}

impl Deref for SharedPages {
    type Target = [PhysAddr];

    fn deref(&self) -> &Self::Target {
        &self.phys_pages
    }
}

impl Drop for SharedPages {
    fn drop(&mut self) {
        for frame in &self.phys_pages {
            alloc::dealloc_frame(*frame, self.align);
        }
    }
}

/// A unified enum type for different memory mapping backends.
///
/// Currently, two backends are implemented:
///
/// - **Linear**: used for linear mappings. The target physical frames are
///   contiguous and their addresses should be known when creating the mapping.
/// - **Allocation**: used in general, or for lazy mappings. The target physical
///   frames are obtained from the global allocator.
#[derive(Clone)]
pub enum Backend {
    /// Linear mapping backend.
    ///
    /// The offset between the virtual address and the physical address is
    /// constant, which is specified by `pa_va_offset`. For example, the virtual
    /// address `vaddr` is mapped to the physical address `vaddr - pa_va_offset`.
    Linear {
        /// `vaddr - paddr`.
        pa_va_offset: usize,
        /// Alignment parameters for the starting address and memory range.
        align: PageSize,
    },
    /// Allocation mapping backend.
    ///
    /// If `populate` is `true`, all physical frames are allocated when the
    /// mapping is created, and no page faults are triggered during the memory
    /// access. Otherwise, the physical frames are allocated on demand (by
    /// handling page faults).
    Alloc {
        /// Whether to populate the physical frames when creating the mapping.
        populate: bool,
        /// Alignment parameters for the starting address and memory range.
        align: PageSize,
    },
    Shared {
        pages: Arc<SharedPages>,
    },
}

impl MappingBackend for Backend {
    type Addr = VirtAddr;
    type Flags = MappingFlags;
    type PageTable = PageTable;
    fn map(&self, start: VirtAddr, size: usize, flags: MappingFlags, pt: &mut PageTable) -> bool {
        match *self {
            Self::Linear {
                pa_va_offset,
                align: _,
            } => Self::map_linear(start, size, flags, pt, pa_va_offset),
            Self::Alloc { populate, align } => {
                Self::map_alloc(start, size, flags, pt, populate, align)
            }
            Self::Shared { ref pages } => Self::map_shared(start, pages, flags, pt),
        }
    }

    fn unmap(&self, start: VirtAddr, size: usize, pt: &mut PageTable) -> bool {
        match *self {
            Self::Linear {
                pa_va_offset,
                align: _,
            } => Self::unmap_linear(start, size, pt, pa_va_offset),
            Self::Alloc { populate, align } => Self::unmap_alloc(start, size, pt, populate, align),
            Self::Shared { ref pages } => Self::unmap_shared(start, pages, pt),
        }
    }

    fn protect(
        &self,
        start: Self::Addr,
        size: usize,
        new_flags: Self::Flags,
        page_table: &mut Self::PageTable,
    ) -> bool {
        page_table
            .protect_region(start, size, new_flags, true, true)
            .map(|tlb| tlb.ignore())
            .is_ok()
    }
}

impl Backend {
    pub(crate) fn handle_page_fault(
        &self,
        vaddr: VirtAddr,
        orig_flags: MappingFlags,
        page_table: &mut PageTable,
    ) -> bool {
        match *self {
            // Linear & shared mappings should not trigger page faults.
            Self::Linear { .. } | Self::Shared { .. } => false,
            Self::Alloc { populate, align } => {
                Self::handle_page_fault_alloc(vaddr, orig_flags, page_table, populate, align)
            }
        }
    }
}
