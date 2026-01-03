#![no_std]

use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    ptr::NonNull,
};

#[macro_export]
macro_rules! default_heap {
    () => {
        #[global_allocator]
        static DEFAULT_HEAP: $crate::CosGlobalAllocator = $crate::CosGlobalAllocator::new();
    };
}

pub use heap::RustHeap;

pub struct SyscallMemoryProvider;

unsafe impl heap::MemoryPageProvider for SyscallMemoryProvider {
    unsafe fn allocate_pages(&mut self, size: usize) -> Option<core::ptr::NonNull<u8>> {
        cos_sys::memory::alloc_page((size / 0x1000) as u64).ok()
    }

    unsafe fn deallocate_pages(&mut self, address: core::ptr::NonNull<u8>, size: usize) {
        unsafe { cos_sys::memory::free_page(address, (size / 0x1000) as u64).unwrap() }
    }
}

pub struct CosGlobalAllocator {
    heap: UnsafeCell<RustHeap<SyscallMemoryProvider>>,
}

// TODO: 当前内核不支持锁，在支持后使用锁实现同步。当前实现非线程安全
unsafe impl Sync for CosGlobalAllocator {}

impl CosGlobalAllocator {
    pub const fn new() -> Self {
        Self {
            heap: UnsafeCell::new(RustHeap::new(SyscallMemoryProvider)),
        }
    }
}

unsafe impl GlobalAlloc for CosGlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { (&mut *self.heap.get()).allocate(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (&mut *self.heap.get()).deallocate(NonNull::new(ptr).unwrap(), layout) }
    }
}
