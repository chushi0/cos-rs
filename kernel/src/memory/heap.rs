use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::{self, NonNull},
};

use crate::{
    memory::{
        self,
        physics::{AllocFrameHint, alloc_mapped_frame},
    },
    sync::{IrqGuard, SpinLock},
};

static KERNEL_HEAP: SpinLock<KernelHeap> = SpinLock::new(KernelHeap::uninit());

const HEAP_SIZE_CLASSES: [usize; 9] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];
struct KernelHeap {
    bucket: [*mut HeapNodeHead; 9],
}

struct HeapNodeHead {
    page_size: usize,
    next: *mut HeapNodeHead,
    free_ptr: *mut NodeFreeBody,
    _padding: usize,
}

const _: () = {
    assert!(size_of::<HeapNodeHead>() == 32);
};

struct NodeFreeBody {
    next: *mut NodeFreeBody,
}

unsafe impl Send for KernelHeap {}

impl KernelHeap {
    const fn uninit() -> Self {
        Self {
            bucket: [ptr::null_mut(); 9],
        }
    }

    fn allocate(&mut self, layout: Layout) -> *mut u8 {
        let index = match HEAP_SIZE_CLASSES.binary_search(&layout.size()) {
            Ok(bucket) => bucket,  // 刚好是我们预期的大小，使用这个桶
            Err(bucket) => bucket, // 不是我们预期的大小，binary_search会返回稍大的那一个，所以可以直接使用这个桶
        };

        // 如果桶越界了，说明申请超过4K内存，我们直接申请对应内存页
        if index > HEAP_SIZE_CLASSES.len() {
            let mut size = layout.size();
            if (size & 0xFFF) != 0 {
                size = (size & 0xFFF) + 0x1000;
            }
            return match memory::physics::alloc_mapped_frame(size, AllocFrameHint::KernelHeap) {
                Some(ptr) => ptr.as_ptr(),
                None => ptr::null_mut(),
            };
        }

        // 从堆中寻找空闲内存进行分配
        let mut bucket = self.bucket[index];
        while !bucket.is_null() {
            let node_head = unsafe { &mut *bucket };
            if !node_head.free_ptr.is_null() {
                let allocated_ptr = node_head.free_ptr as *mut u8;
                node_head.free_ptr = unsafe { (*node_head.free_ptr).next };
                return allocated_ptr;
            }
            bucket = node_head.next;
        }

        // 桶内的内存不足，申请新内存页
        let new_page = match alloc_mapped_frame(0x1000, AllocFrameHint::KernelHeap) {
            Some(ptr) => ptr.as_ptr(),
            None => return ptr::null_mut(),
        } as *mut HeapNodeHead;

        // 填充新页元数据
        unsafe {
            (*new_page).page_size = HEAP_SIZE_CLASSES[index];
            (*new_page).next = self.bucket[index];
            (*new_page).free_ptr = ptr::null_mut();
            let mut free_ptr = new_page.add(32.max(HEAP_SIZE_CLASSES[index])) as *mut NodeFreeBody;
            while (free_ptr as usize) < (new_page as usize + 0x1000) {
                (*free_ptr).next = (*new_page).free_ptr;
                (*new_page).free_ptr = free_ptr;
                free_ptr = free_ptr.add(HEAP_SIZE_CLASSES[index]);
            }
            self.bucket[index] = new_page;
        }

        // 从链表中取出一个元素
        let ptr = unsafe {
            let ptr = (*new_page).free_ptr;
            (*new_page).free_ptr = (*ptr).next;
            ptr as *mut u8
        };

        ptr
    }

    unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
        let ptr = ptr.as_ptr();

        // 如果对齐到4K，说明之前申请的完整的一页内存，直接交还给page frame
        if (ptr as usize & 0xFFF) == 0 {
            let mut size = layout.size();
            if (size & 0xFFF) != 0 {
                size = (size & 0x1000) + 0x1000;
            }
            unsafe {
                memory::physics::free_mapped_frame(ptr as usize, size);
            }
            return;
        }

        // 其余情况，对齐到4K，获取bucket元数据
        let head = (ptr as usize & !0xFFF) as *mut HeapNodeHead;

        // 添加到free_list
        unsafe {
            let ptr = ptr as *mut NodeFreeBody;
            (*ptr).next = (*head).free_ptr;
            (*head).free_ptr = ptr;
        }

        // TODO: 释放回page frame?
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: GlobalAllocator = GlobalAllocator;

struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _guard = unsafe { IrqGuard::cli() };
        KERNEL_HEAP.lock().allocate(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _guard = unsafe { IrqGuard::cli() };
        unsafe {
            KERNEL_HEAP
                .lock()
                .deallocate(NonNull::new_unchecked(ptr), layout);
        }
    }
}
