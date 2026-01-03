use core::{num::NonZeroUsize, ptr};

use crate::{
    bootloader::MemoryRegion,
    memory::page::{get_kernel_used_memory, insert_temp_page_table},
    sync::spin::SpinLock,
};

/// 系统可用的内存范围
static mut MEMORY_REGION: &[MemoryRegion] = &[];
/// 页帧分配器
pub static FRAME_ALLOCATOR: SpinLock<FrameAllocator> = SpinLock::new(FrameAllocator::const_new());

/// 初始化物理内存管理
///
/// Safety:
/// 函数当前页表（及所对应的内存）必须可读写。
pub(super) unsafe fn init(memory_region: &'static [MemoryRegion]) {
    // 初始化页帧分配器
    unsafe {
        MEMORY_REGION = memory_region;
        FRAME_ALLOCATOR.lock().init();
    }
}

/// 直接读取指定物理内存数据
///
/// 此函数会在页表中添加一个临时项，以允许访问指定物理内存。
///
/// Safety:
/// 该物理内存必须存在。对内存的访问不能违反Rust规则。
pub unsafe fn read_memory<T: Sized>(address: usize, dst: &mut T) {
    let size = size_of::<T>();
    let mut src_start = address;
    let src_end = address + size;
    let mut dst_start = dst as *mut T as usize;
    while src_start < src_end {
        let (start, len) = insert_temp_page_table(src_start);
        let len = len.min(src_end - src_start);

        // Safety: 我们已将物理地址加入页表，并映射为虚拟地址
        unsafe {
            ptr::copy_nonoverlapping(start as *const u8, dst_start as *mut u8, len);
        }

        src_start += len;
        dst_start += len;
    }
}

/// 直接写入指定物理内存数据
///
/// 此函数会在页表中添加一个临时项，以允许访问指定物理内存。
///
/// Safety:
/// 该物理内存必须存在。对内存的访问不能违反Rust规则。
pub unsafe fn write_memory<T: Sized>(address: usize, src: &T) {
    let size = size_of::<T>();
    let mut dst_start = address;
    let dst_end = address + size;
    let mut src_start = src as *const T as usize;
    while dst_start < dst_end {
        let (start, len) = insert_temp_page_table(dst_start);
        let len = len.min(dst_end - dst_start);

        // Safety: 我们已将物理地址加入页表，并映射为虚拟地址
        unsafe {
            ptr::copy_nonoverlapping(src_start as *const u8, start as *mut u8, len);
        }

        dst_start += len;
        src_start += len;
    }
}

/// 清空物理内存
///
/// 此函数会在页表中添加一个临时项，以允许访问指定物理内存。
///
/// Safety:
/// 该物理内存必须存在。对内存的访问不能违反Rust规则。
pub unsafe fn zero_memory(address: usize, size: usize) {
    let mut dst_start = address;
    let dst_end = address + size;
    while dst_start < dst_end {
        let (start, len) = insert_temp_page_table(dst_start);
        let len = len.min(dst_end - dst_start);

        // Safety: 我们已将物理地址加入页表，并映射为虚拟地址
        unsafe {
            ptr::write_bytes(start as *mut u8, 0, len);
        }

        dst_start += len;
    }
}

pub struct FrameAllocator {
    /// 下一个待首次分配的地址，按照memory_region的位置顺序分配
    first_alloc_address: Option<NonZeroUsize>,
    /// 已经归还的内存，通过链表存储，此处仅存储链表头对应的地址
    linked_free_address: Option<NonZeroUsize>,
}

impl FrameAllocator {
    const fn const_new() -> Self {
        Self {
            first_alloc_address: None,
            linked_free_address: None,
        }
    }

    fn init(&mut self) {
        // loader程序是从2M地址开始分配的，因此start_address额外加2M
        let start_address = get_kernel_used_memory() + 0x20_0000;
        self.first_alloc_address = NonZeroUsize::new(start_address);
    }

    /// 分配4K物理内存，并对齐到4K
    ///
    /// 如果分配成功，返回物理内存的起始地址
    /// 如果分配失败，返回None
    ///
    /// 注意：返回的地址是物理地址且不保证立即可用
    /// 当需要进行访问时，需要先加入页表，或使用 [`write_memory`] / [`read_memory`] 操作
    pub fn alloc_frame(&mut self) -> Option<NonZeroUsize> {
        // 首先从已经释放的链表中分配
        if let Some(linked_free_address) = self.linked_free_address {
            // 将链表中下一个节点取出
            unsafe {
                read_memory(linked_free_address.get(), &mut self.linked_free_address);
            }
            return Some(linked_free_address);
        }

        if let Some(first_alloc_address) = self.first_alloc_address {
            // 从尚未分配的内存中分配
            for memory_region in unsafe { MEMORY_REGION } {
                let region_start = memory_region.base_addr;
                let region_end = region_start + memory_region.length;

                // 如果当前区域已经分配完成，则跳过当前区域
                if region_end <= first_alloc_address.get() as u64 {
                    continue;
                }

                // 计算分配的开始地址，取region_start与first_alloc_address较大的一个，并对齐到4K
                let mut alloc_start = region_start.max(first_alloc_address.get() as u64);
                if (alloc_start & 0xFFF) != 0 {
                    alloc_start = (alloc_start & 0x1000) + 0x1000;
                }
                // 计算分配的结束地址
                let alloc_end = alloc_start + 0x1000;
                // 如果结束地址超过当前区域，则跳过当前区域
                if alloc_end > region_end {
                    continue;
                }

                // 更新分配进度
                self.first_alloc_address = NonZeroUsize::new(alloc_end as usize);

                return NonZeroUsize::new(alloc_start as usize);
            }
        }

        // 分配失败：系统已无空余内存
        self.first_alloc_address = None;
        None
    }

    /// 回收4K物理内存，address必须为对应物理内存的起始地址
    /// 回收后的物理内存将用于下次分配
    ///
    /// Safety:
    /// address必须为有效的物理内存，且不能双重释放
    pub unsafe fn delloc_frame(&mut self, address: NonZeroUsize) {
        // 在头部添加链表
        // Safety: 由调用者保证内存写入安全
        unsafe {
            write_memory(address.get(), &self.linked_free_address);
        }
        self.linked_free_address = Some(address);
    }
}
