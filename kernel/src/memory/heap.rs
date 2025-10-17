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
    index: usize,
    next: *mut HeapNodeHead,
    free_ptr: *mut NodeFreeBody,
    free_count: u64,
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
        if index >= HEAP_SIZE_CLASSES.len() {
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
                node_head.free_count -= 1;
                // 如果分配完成后，当前块已经没有空余内存，则移出链表
                if node_head.free_count > 0 {
                    self.bucket[index] = bucket;
                } else {
                    self.bucket[index] = node_head.next;
                }
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
            (*new_page).index = index;
            (*new_page).next = self.bucket[index];
            (*new_page).free_ptr = ptr::null_mut();
            (*new_page).free_count = (0x1000 / HEAP_SIZE_CLASSES[index] - 1) as u64;
            let mut free_ptr =
                (new_page as usize + 32.max(HEAP_SIZE_CLASSES[index])) as *mut NodeFreeBody;
            while (free_ptr as usize) < (new_page as usize + 0x1000) {
                (*free_ptr).next = (*new_page).free_ptr;
                (*new_page).free_ptr = free_ptr;
                free_ptr = (free_ptr as usize + HEAP_SIZE_CLASSES[index]) as *mut NodeFreeBody;
            }
            self.bucket[index] = new_page;
        }

        // 从链表中取出一个元素
        unsafe {
            let ptr = (*new_page).free_ptr;
            (*new_page).free_ptr = (*ptr).next;
            (*new_page).free_count -= 1;
            // 如果分配完成后，当前块已经没有空余内存，则移出链表
            if (*new_page).free_count == 0 {
                self.bucket[index] = (*new_page).next;
            }
            ptr as *mut u8
        }
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
            (*head).free_count += 1;
            // 如果当前块首次释放，则加入链表
            if (*head).free_count == 1 {
                let index = (*head).index;
                (*head).next = self.bucket[index];
                self.bucket[index] = head;
            }
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
