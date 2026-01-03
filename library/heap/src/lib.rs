#![no_std]

use core::{
    alloc::Layout,
    ptr::{self, NonNull},
};

const HEAP_SIZE_CLASSES: [usize; 9] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];
const HEAP_FREE_COUNT: [u64; 9] = [
    free_count(8),
    free_count(16),
    free_count(32),
    free_count(64),
    free_count(128),
    free_count(256),
    free_count(512),
    free_count(1024),
    free_count(2048),
];

const fn free_count(size: usize) -> u64 {
    ((0x1000 - size_of::<HeapNodeHead>()) / size) as u64
}

pub trait MemoryPageProvider {
    fn allocate_page(&mut self) -> Option<NonNull<u8>> {
        self.allocate_pages(0x1000)
    }

    fn allocate_pages(&mut self, size: usize) -> Option<NonNull<u8>>;
    fn deallocate_pages(&mut self, address: NonNull<u8>, size: usize);
}

pub struct RustHeap<P> {
    bucket: [*mut HeapNodeHead; 9],
    provider: P,
}

// 使用双向链表管理空闲内存
struct HeapNodeHead {
    prev: *mut HeapNodeHead,
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

unsafe impl<P: Send> Send for RustHeap<P> {}

impl<P: MemoryPageProvider> RustHeap<P> {
    pub const fn new(provider: P) -> Self {
        Self {
            bucket: [ptr::null_mut(); 9],
            provider,
        }
    }

    pub fn allocate(&mut self, layout: Layout) -> *mut u8 {
        let index = Self::get_bucket_index_by_layout(layout);

        // 如果桶越界了，说明申请超过4K内存，我们直接申请对应内存页
        if index >= HEAP_SIZE_CLASSES.len() {
            let mut size = layout.align().max(layout.size());
            if (size & 0xFFF) != 0 {
                size = (size & !0xFFF) + 0x1000;
            }
            return match self.provider.allocate_pages(size) {
                Some(ptr) => ptr.as_ptr(),
                None => ptr::null_mut(),
            };
        }

        // 桶内的内存不足，申请新内存页
        if self.bucket[index].is_null() {
            let new_page = match self.provider.allocate_page() {
                Some(ptr) => ptr.as_ptr(),
                None => return ptr::null_mut(),
            } as *mut HeapNodeHead;

            // 填充新页元数据
            unsafe {
                (*new_page).prev = ptr::null_mut();
                (*new_page).next = self.bucket[index];
                self.bucket[index] = new_page;

                (*new_page).free_ptr = ptr::null_mut();
                (*new_page).free_count = HEAP_FREE_COUNT[index];
                let mut free_ptr = (new_page as usize
                    + HEAP_SIZE_CLASSES[index].max(size_of::<HeapNodeHead>()))
                    as *mut NodeFreeBody;
                while (free_ptr as usize) < (new_page as usize + 0x1000) {
                    (*free_ptr).next = (*new_page).free_ptr;
                    (*new_page).free_ptr = free_ptr;
                    free_ptr = (free_ptr as usize + HEAP_SIZE_CLASSES[index]) as *mut NodeFreeBody;
                }
            }
        }

        // 从链表中取出一个元素
        unsafe {
            let page = self.bucket[index];
            let ptr = (*page).free_ptr;
            (*page).free_ptr = (*ptr).next;
            (*page).free_count -= 1;
            // 如果分配完成后，当前块已经没有空余内存，则移出链表
            if (*page).free_count == 0 {
                self.bucket[index] = (*page).next;
                if !self.bucket[index].is_null() {
                    (*self.bucket[index]).prev = ptr::null_mut();
                }
            }
            ptr as *mut u8
        }
    }

    pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
        // 如果对齐到4K，说明之前申请的完整的一页内存，直接交还给page frame
        if (ptr.as_ptr() as usize & 0xFFF) == 0 {
            let mut size = layout.align().max(layout.size());
            if (size & 0xFFF) != 0 {
                size = (size & !0xFFF) + 0x1000;
            }
            self.provider.deallocate_pages(ptr, size);
            return;
        }

        // 其余情况，对齐到4K，获取bucket元数据
        let ptr = ptr.as_ptr();
        let head = (ptr as usize & !0xFFF) as *mut HeapNodeHead;
        let index = Self::get_bucket_index_by_layout(layout);

        // 添加到free_list
        unsafe {
            let ptr = ptr as *mut NodeFreeBody;
            (*ptr).next = (*head).free_ptr;
            (*head).free_ptr = ptr;
            (*head).free_count += 1;
            // 如果当前块首次释放，则加入链表
            if (*head).free_count == 1 {
                if !self.bucket[index].is_null() {
                    (*self.bucket[index]).prev = head;
                }
                (*head).next = self.bucket[index];
                (*head).prev = ptr::null_mut();
                self.bucket[index] = head;
            }
            // 如果当前块全部释放，则返还page frame
            if (*head).free_count == HEAP_FREE_COUNT[index] {
                if (*head).prev.is_null() {
                    self.bucket[index] = (*head).next;
                    if !self.bucket[index].is_null() {
                        (*(*head).next).prev = ptr::null_mut();
                    }
                } else {
                    (*(*head).prev).next = (*head).next;
                    if !(*head).next.is_null() {
                        (*(*head).next).prev = (*head).prev;
                    }
                }
                self.provider
                    .deallocate_pages(NonNull::new_unchecked(head.cast::<u8>()), 0x1000);
            }
        }
    }

    fn get_bucket_index_by_layout(layout: Layout) -> usize {
        let effective = layout.align().max(layout.size());
        match HEAP_SIZE_CLASSES.binary_search(&effective) {
            Ok(bucket) => bucket,  // 刚好是我们预期的大小，使用这个桶
            Err(bucket) => bucket, // 不是我们预期的大小，binary_search会返回稍大的那一个，所以可以直接使用这个桶
        }
    }
}
