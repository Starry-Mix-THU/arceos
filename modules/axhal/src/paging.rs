//! Page table manipulation.

use axalloc::{UsageKind, global_allocator};
use memory_addr::{PAGE_SIZE_4K, PhysAddr, VirtAddr};
use page_table_multiarch::PagingHandler;
#[doc(no_inline)]
pub use page_table_multiarch::{MappingFlags, PageSize, PagingError, PagingResult};

use crate::mem::{phys_to_virt, virt_to_phys};

/// Implementation of [`PagingHandler`], to provide physical memory manipulation
/// to the [page_table_multiarch] crate.
pub struct PagingHandlerImpl;

impl PagingHandler for PagingHandlerImpl {
    fn alloc_frame() -> Option<PhysAddr> {
        global_allocator()
            .alloc_pages(1, PAGE_SIZE_4K, UsageKind::PageTable)
            .map(|vaddr| virt_to_phys(vaddr.into()))
            .ok()
    }

    fn dealloc_frame(paddr: PhysAddr) {
        global_allocator().dealloc_pages(phys_to_virt(paddr).as_usize(), 1, UsageKind::PageTable)
    }

    #[inline]
    fn phys_to_virt(paddr: PhysAddr) -> VirtAddr {
        phys_to_virt(paddr)
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_arch = "x86_64")] {
        /// The architecture-specific page table.
        pub type PageTable = page_table_multiarch::x86_64::X64PageTable<PagingHandlerImpl>;
    } else if #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))] {
        /// The architecture-specific page table.
        pub type PageTable = page_table_multiarch::riscv::Sv39PageTable<PagingHandlerImpl>;
    } else if #[cfg(target_arch = "aarch64")]{
        /// The architecture-specific page table.
        pub type PageTable = page_table_multiarch::aarch64::A64PageTable<PagingHandlerImpl>;
    } else if #[cfg(target_arch = "loongarch64")] {
        /// The architecture-specific page table.
        pub type PageTable = page_table_multiarch::loongarch64::LA64PageTable<PagingHandlerImpl>;
    }
}
