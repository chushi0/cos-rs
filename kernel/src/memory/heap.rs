use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
};

use heap::{MemoryPageProvider, RustHeap};

use crate::{
    memory::{self, page::AllocateFrameOptions},
    sync::{int::IrqGuard, spin::SpinLock},
};

static KERNEL_HEAP: SpinLock<RustHeap<PhysicalMemoryPageProvider>> =
    SpinLock::new(RustHeap::new(PhysicalMemoryPageProvider));

struct PhysicalMemoryPageProvider;

impl MemoryPageProvider for PhysicalMemoryPageProvider {
    fn allocate_pages(&mut self, size: usize) -> Option<NonNull<u8>> {
        unsafe {
            memory::page::alloc_mapped_frame(
                memory::page::kernel_pml4(),
                size,
                AllocateFrameOptions::KERNEL_DATA,
            )
            .ok()
        }
    }

    fn deallocate_pages(&mut self, address: NonNull<u8>, size: usize) {
        unsafe {
            memory::page::free_mapped_frame(
                memory::page::kernel_pml4(),
                address.as_ptr() as usize,
                size,
            );
        }
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: GlobalAllocator = GlobalAllocator;

struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _guard = IrqGuard::cli();
        KERNEL_HEAP.lock().allocate(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _guard = IrqGuard::cli();
        unsafe {
            KERNEL_HEAP
                .lock()
                .deallocate(NonNull::new_unchecked(ptr), layout);
        }
    }
}
