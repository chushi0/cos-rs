use core::{
    arch::asm,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    ptr,
};

use crate::{
    bootloader::MemoryRegion,
    sync::{IrqGuard, SpinLock},
};

/// 系统可用的内存范围
static mut MEMORY_REGION: &[MemoryRegion] = &[];
/// 页帧分配器
static FRAME_ALLOCATOR: SpinLock<FrameAllocator> = SpinLock::new(FrameAllocator::const_new());

/// bootloader里定义的4级页表结构
///
/// 仅使用了 `PML4[0] = &raw const LOADER_PDPT`
/// 和 `PML4[511] = &raw const KERNEL_PDPT`
static mut PML4: Option<*const PageTable> = None;

/// bootloader里定义的3级页表结构
///
/// 仅使用了 `LOADER_PDPT[0] = &raw const LOADER_PD`
static mut LOADER_PDPT: Option<*const PageTable> = None;

/// bootloader里定义的2级页表结构
///
/// 仅使用了 `LOADER_PD[0] = &raw const LOADER_PT`
static mut LOADER_PD: Option<*const PageTable> = None;

/// bootloader里定义的1级页表结构
///
/// 仅使用了如下部分
/// 0x1000 ~ 0x2000   - 内存检测信息
/// 0x2000 ~ 0x3000   - stub
/// 0x7000 ~ 0x7FFF   - 栈空间（但我们只会用0x7c00之前的）
/// 0x8000 ~ X        - text段、bss段、rodata段
/// 0xb8000 ~ 0xb9000 - VGA 显示
static mut LOADER_PT: Option<*const PageTable> = None;

/// bootloader里定义的3级页表结构
///
/// 仅使用了 `KERNEL_PDPT[511] = &raw const KERNEL_PD`
static mut KERNEL_PDPT: Option<*const PageTable> = None;

/// bootloader里定义的2级页表结构
///
/// 仅使用了如下部分：
/// 0xFFFF_FFFF_FFC0_0000 ~ 0xFFFF_FFFF_FFDF_FFFF - 栈空间（2M）
/// 0xFFFF_FFFF_C000_0000 ~ X                     - text段、bss段、rodata段
///
/// 对于超过2M的页，直接在KERNEL_PD中使用大页分配
/// 对于不足2M的页，通过KERNEL_PT分配
static mut KERNEL_PD: Option<*const PageTable> = None;

/// bootloader里定义的1级页表结构
static mut KERNEL_PT: Option<*const PageTable> = None;

/// 初始化物理内存管理
///
/// Safety:
/// 函数当前页表（及所对应的内存）必须可读写。
pub unsafe fn init(memory_region: &'static [MemoryRegion]) {
    // 从CR3寄存器中重新获取页表信息
    // Safety: cr3可读，且页表均已经被映射到虚拟空间（物理地址与虚拟地址一致）
    unsafe {
        let pml4: u64;
        asm!(
            "mov {}, cr3",
            out(reg) pml4,
            options(nostack, preserves_flags)
        );
        let pml4 = pml4 as usize as *const PageTable;
        let loader_pdpt = (&*pml4)[0].address() as usize as *const PageTable;
        let loader_pd = (&*loader_pdpt)[0].address() as usize as *const PageTable;
        let loader_pt = (&*loader_pd)[0].address() as usize as *const PageTable;
        let kernel_pdpt = (&*pml4)[511].address() as usize as *const PageTable;
        let kernel_pd = (&*kernel_pdpt)[511].address() as usize as *const PageTable;
        let kernel_pt = (&*kernel_pd)
            .iter()
            .find(|entry| !entry.ps() && entry.present())
            .map(|entry| entry.address() as usize as *const PageTable);
        PML4 = Some(pml4);
        LOADER_PDPT = Some(loader_pdpt);
        LOADER_PD = Some(loader_pd);
        LOADER_PT = Some(loader_pt);
        KERNEL_PDPT = Some(kernel_pdpt);
        KERNEL_PD = Some(kernel_pd);
        KERNEL_PT = kernel_pt;
    };

    // 初始化页帧分配器
    unsafe {
        MEMORY_REGION = memory_region;
        FRAME_ALLOCATOR.lock().init();
    }
}

/// 直接读取指定物理内存数据
///
/// 此函数会在页表中添加一个临时项，以允许访问指定物理内存。
/// 读取期间会临时关中断，以防对页表的临时处理被打断。
///
/// Safety:
/// 该内存必须存在
pub unsafe fn read_memory<T: Sized>(address: usize, dst: &mut T) {
    let _guard = unsafe { IrqGuard::cli() };

    let size = size_of::<T>();
    let mut src_start = address;
    let src_end = address + size;
    let mut dst_start = dst as *mut T as usize;
    while src_start < src_end {
        // Safety:
        let (start, len) = unsafe { insert_temp_page_table(src_start) };
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
/// 写入期间会临时关中断，以防对页表的临时处理被打断。
///
/// Safety:
/// 该内存必须存在
pub unsafe fn write_memory<T: Sized>(address: usize, src: &T) {
    let _guard = unsafe { IrqGuard::cli() };

    let size = size_of::<T>();
    let mut dst_start = address;
    let dst_end = address + size;
    let mut src_start = src as *const T as usize;
    while dst_start < dst_end {
        // Safety:
        let (start, len) = unsafe { insert_temp_page_table(dst_start) };
        let len = len.min(dst_end - dst_start);

        // Safety: 我们已将物理地址加入页表，并映射为虚拟地址
        unsafe {
            ptr::copy_nonoverlapping(src_start as *const u8, start as *mut u8, len);
        }

        dst_start += len;
        src_start += len;
    }
}

/// 将地址插入到临时页表中，返回对应的虚拟地址及最大长度
///
/// Safety: 必须已经关中断
unsafe fn insert_temp_page_table(address: usize) -> (usize, usize) {
    // 对齐内存
    let aligned = address & 0xFFFF_FFFF_FFFF_F000;
    // 计算虚拟地址
    let virtual_start = address - aligned + 0x3000;
    let virtual_len = aligned + 0x1000 - address;
    // 我们复用LOADER_PT结构，在0x3000~0x4000创建4k内存页
    unsafe {
        (&mut *(LOADER_PT.unwrap() as *mut PageTable))[3].0 =
            aligned as u64 | PageEntry::P_PRESENT | PageEntry::P_RW;
    }
    // 更新页表缓存
    unsafe {
        asm!(
            "invlpg [{}]",
            in(reg) virtual_start,
            options(nostack, preserves_flags)
        );
    }
    (virtual_start, virtual_len)
}

struct FrameAllocator {
    /// 待首次分配的地址，按照memory_region的位置顺序分配
    first_alloc_address: Option<NonZeroUsize>,
    /// 已经归还的内存，通过链表存储，此处仅存储链表头对应的地址
    #[allow(unused)]
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
        // 从KERNEL_PD和KERNEL_PT中查找当前内核已使用内存
        let mut start_address = 0x20_0000;
        const SIZE_2M: usize = 0x20_0000;
        const SIZE_4K: usize = 0x1000;
        unsafe {
            if let Some(pd) = KERNEL_PD {
                for entry in &**pd {
                    if entry.present() && entry.ps() {
                        start_address += SIZE_2M;
                    }
                }
            }
            if let Some(pt) = KERNEL_PT {
                for entry in &**pt {
                    if entry.present() {
                        start_address += SIZE_4K;
                    }
                }
            }
        }

        self.first_alloc_address = NonZeroUsize::new(start_address);
    }
}

#[repr(C, align(4096))]
struct PageTable([PageEntry; 512]);

#[derive(Debug)]
#[repr(transparent)]
struct PageEntry(u64);

impl Deref for PageTable {
    type Target = [PageEntry; 512];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PageTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl PageEntry {
    const P_PRESENT: u64 = 1 << 0;
    const P_RW: u64 = 1 << 1;
    const P_PS: u64 = 1 << 7;

    fn address(&self) -> u64 {
        self.0 & 0x000F_FFFF_FFFF_F000
    }

    fn ps(&self) -> bool {
        (self.0 & Self::P_PS) != 0
    }

    fn present(&self) -> bool {
        (self.0 & Self::P_PRESENT) != 0
    }
}
