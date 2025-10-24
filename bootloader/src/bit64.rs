use core::{
    arch::asm,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    ptr,
};

use crate::{ProjectInfo, memory::MemoryRegion};

#[derive(Debug, Clone, Copy)]
#[repr(C, align(8))]
struct GdtEntry(u64);

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

static mut GDT: [GdtEntry; 9] = [
    GdtEntry::null(),  // 0x00
    GdtEntry::null(),  // 0x08
    GdtEntry::null(),  // 0x10
    GdtEntry::code(),  // 0x18
    GdtEntry::data(),  // 0x20
    GdtEntry::null(),  // 0x28
    GdtEntry::null(),  // 0x30
    GdtEntry::ucode(), // 0x38
    GdtEntry::udata(), // 0x40
];
static mut GDTR: DescriptorTablePointer = DescriptorTablePointer::uninit();

#[repr(C, align(4096))]
struct PageTable([u64; 512]);

// 4级页表
static mut PML4: PageTable = PageTable::uninit();
// LOADER用 3级页表
static mut LOADER_PDPT: PageTable = PageTable::uninit();
// LOADER用 2级页表
static mut LOADER_PD: PageTable = PageTable::uninit();
// LOADER用 1级页表
static mut LOADER_PT: PageTable = PageTable::uninit();
// KERNEL用 3级页表
static mut KERNEL_PDPT: PageTable = PageTable::uninit();
// KERNEL用 2级页表
static mut KERNEL_PD: PageTable = PageTable::uninit();
// KERNEL用 1级页表
static mut KERNEL_PT: PageTable = PageTable::uninit();

/// 检测CPU是否支持64位
pub fn test_cpu_is_support_64bit() -> bool {
    // 1. 检查扩展功能是否存在
    let mut eax: u32 = 0x80000000;
    // Safety: cpuid会覆盖eax/ebx/ecx/edx，我们已经指定out，避免寄存器污染
    unsafe {
        asm!(
            "cpuid",
            inout("eax") eax,
            out("ebx") _,
            out("ecx") _,
            out("edx") _,
            options(nostack, preserves_flags)
        )
    };
    // cpuid返回的值是扩展功能的最大值，如果小于0x80000001，则没有长模式信息，说明不支持64位
    if eax < 0x80000001 {
        return false;
    }

    // 2. 读取 0x80000001
    eax = 0x80000001;
    let edx: u32;
    // Safety: cpuid会覆盖eax/ebx/ecx/edx，我们已经指定out，避免寄存器污染
    unsafe {
        asm!(
            "cpuid",
            in("eax") eax,
            lateout("eax") _,
            out("ebx") _,
            out("ecx") _,
            out("edx") edx,
            options(nostack, preserves_flags)
        )
    };
    // cpuid返回的edx的第29位（0-based）为1，表示支持长模式
    (edx & 1 << 29) != 0
}

// 从保护模式切换到长模式
// Safety: 调用者需确保当前处于保护模式，无并发，已经关中断，各段寄存器均指向1/2号GDT，已经加载内核
pub unsafe fn enable_64bit_mode(
    project_info: &ProjectInfo,
    memory_region: &[MemoryRegion],
    startup_disk: u8,
) -> ! {
    // 1. 设置64位GDT
    // Safety: 本段代码涉及大量unsafe行为，依次解释其Safety原因：
    //  - 访问全局变量 GDT/GDTR: 由调用者保证不会并发
    //  - 执行内联汇编 sgdt/lgdt: 当前cs/ds/es/ss指向1/2号GDT，我们保证不修改这部分数据
    //  - 解引用裸指针: 裸指针由 sgdt 指令给出，一定有效。虽然未对齐，但我们已用read_unaligned
    unsafe {
        // 保留当前的GDT
        let mut current_gdt = DescriptorTablePointer::uninit();
        asm!(
            "sgdt [{}]",
            in(reg) &raw mut current_gdt,
            options(nostack, preserves_flags)
        );
        for i in 1..=2 {
            GDT[i] = ptr::read_unaligned(
                (current_gdt.base as usize + size_of::<GdtEntry>() * i) as *const GdtEntry,
            );
        }

        // 为64bit准备GDT
        GDTR.limit = (size_of_val(&*(&raw const GDT)) - 1) as u16;
        GDTR.base = &raw const GDT as u64;
        asm!(
            "lgdt [{}]",
            in(reg) &raw const GDTR,
            options(nostack, preserves_flags)
        );
    }

    // 2. 初始化页表，设置CR3指向PML4
    let tables = [
        &raw const PML4,
        &raw const LOADER_PDPT,
        &raw const LOADER_PD,
        &raw const LOADER_PT,
        &raw const KERNEL_PDPT,
        &raw const KERNEL_PD,
        &raw const KERNEL_PT,
    ];
    for ptr in tables {
        assert!((ptr as usize & 0xFFF) == 0)
    }
    // Safety: 无并发，无中断，PDPT、PD均为4kb对齐，将0~1G内存映射
    unsafe {
        const P_PRESENT: u64 = 1 << 0;
        const P_RW: u64 = 1 << 1;
        const P_PS: u64 = 1 << 7;
        const SIZE_2M: u64 = 0x20_0000;
        const SIZE_4K: u64 = 0x1000;

        // loader 程序用页表
        // 0x1000 ~ 0x2000   - 内存检测信息
        // 0x2000 ~ 0x3000   - stub
        // 0x7000 ~ 0x7FFF   - 栈空间（但我们只会用0x7c00之前的）
        // 0x8000 ~ X        - text段、bss段、rodata段
        // 0xb8000 ~ 0xb9000 - VGA 显示
        let loader_binary_page_count =
            (project_info.loader_size as u64 * 512 + SIZE_4K - 1) / SIZE_4K;
        assert!(loader_binary_page_count + 7 < 512);
        PML4[0] = &raw const LOADER_PDPT as u64 | P_PRESENT | P_RW;
        LOADER_PDPT[0] = &raw const LOADER_PD as u64 | P_PRESENT | P_RW;
        LOADER_PD[0] = &raw const LOADER_PT as u64 | P_PRESENT | P_RW;
        // 0x1000~0x2000
        LOADER_PT[1] = 0x1000 as u64 | P_PRESENT | P_RW;
        // 0x2000~0x3000
        LOADER_PT[2] = 0x2000 as u64 | P_PRESENT | P_RW;
        // 0x7000 ~ 0x7FFF
        LOADER_PT[7] = 0x7000 as u64 | P_PRESENT | P_RW;
        // 0xb8000 ~ 0xb9000
        LOADER_PT[0xb8] = 0xb8000 as u64 | P_PRESENT | P_RW;
        // 0x8000 ~ X
        for i in 0..loader_binary_page_count {
            LOADER_PT[i as usize + 8] = (0x8000 + 0x1000 * i) as u64 | P_PRESENT | P_RW;
        }

        // kernel 程序用页表
        // 0xFFFF_FFFF_FFC0_0000 ~ 0xFFFF_FFFF_FFDF_FFFF - 栈空间（2M）
        // 0xFFFF_FFFF_C000_0000 ~ X                     - text段、bss段、rodata段
        let kernel_binary_page_count =
            (project_info.kernel_size as u64 * 512 + SIZE_4K - 1) / SIZE_4K;
        assert!(kernel_binary_page_count < 512 * 512);
        let kernel_binary_start = SIZE_2M;
        let kernel_stack_start =
            (kernel_binary_start + (kernel_binary_page_count * SIZE_4K) + SIZE_2M - 1) / SIZE_2M
                * SIZE_2M;
        PML4[511] = &raw const KERNEL_PDPT as u64 | P_PRESENT | P_RW;
        KERNEL_PDPT[511] = &raw const KERNEL_PD as u64 | P_PRESENT | P_RW;
        // 0xFFFF_FFFF_FFC0_0000 ~ 0xFFFF_FFFF_FFDF_FFFF
        KERNEL_PD[510] = kernel_stack_start | P_PS | P_PRESENT | P_RW;
        // 0xFFFF_FFFF_C000_0000 ~ X
        let mut i = 0;
        while i < kernel_binary_page_count {
            let remain = kernel_binary_page_count - i;
            // 足够2M，用大页
            if remain >= 512 {
                KERNEL_PD[(i / 512) as usize] =
                    (kernel_binary_start + SIZE_2M * (i / 512) as u64) | P_PS | P_PRESENT | P_RW;
                // 跳过整个大页
                i += 512;
                continue;
            }
            // 不足2M，首次进入时将PT挂到PD上
            if i % 512 == 0 {
                KERNEL_PD[(i / 512) as usize] = &raw const KERNEL_PT as u64 | P_PRESENT | P_RW;
            }
            // 4K页
            KERNEL_PT[(i % 512) as usize] = (kernel_binary_start + SIZE_4K * i) | P_PRESENT | P_RW;
            // 处理下一个4K页
            i += 1;
        }

        // 写入cr3
        asm!(
            "mov cr3, {}",
            in(reg) &raw const PML4,
            options(nostack, preserves_flags)
        );
    }
    // 3. 启用PAE（CR4.PAE = 1）
    // Safety: CR3已经指向PML4，开启PAE是安全的
    unsafe {
        let mut cr4: u32;
        asm!(
            "mov {}, cr4",
            out(reg) cr4,
            options(nostack, preserves_flags)
        );

        cr4 |= 1 << 5;

        asm!(
            "mov cr4, {}",
            in(reg) cr4,
            options(nostack, preserves_flags)
        );
    }
    // 4. 设置长模式标志位（EFER.LME = 1）
    // Safety: 已开启PAE，CR3与CR4均已配置
    unsafe {
        const IA32_EFER: u32 = 0xc000_0080;
        let mut low: u32;
        let high: u32;

        asm!(
            "rdmsr",
            in("ecx") IA32_EFER,
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags)
        );

        low |= 1 << 8;

        asm!(
            "wrmsr",
            in("ecx") IA32_EFER,
            in("eax") low,
            in("edx") high,
            options(nostack, preserves_flags)
        );
    }
    // 5. 启用分页
    // Safety: 我们已经配置了CR4，并且马上就要离开这里了
    unsafe {
        let mut cr0: u32;

        asm!(
            "mov {}, cr0",
            out(reg) cr0,
            options(nostack, preserves_flags)
        );

        cr0 |= 1 << 31;

        asm!(
            "mov cr0, {}",
            in(reg) cr0,
            options(nostack, preserves_flags)
        );
    }

    // 6. 远跳 jmp 64bit_code_segment:label 进入长模式
    const STUB_ASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/stub.bin"));
    const STUB_ASM_PTR: usize = 0x2000;
    // Safety: STUB_ASM是我们编译好的最终汇编指令，起始点为0x2000
    // 指令大小不会超过0x1000，不会越界
    unsafe {
        assert!(STUB_ASM.len() < 0x1000);

        ptr::copy_nonoverlapping(STUB_ASM.as_ptr(), STUB_ASM_PTR as *mut u8, STUB_ASM.len());

        asm!(
            "mov esi, {p2}",
            "jmp {addr}",
            in("edi") memory_region.as_ptr(),
            in("edx") startup_disk as u32,
            p2 = in(reg) memory_region.len(),
            addr = in(reg) STUB_ASM_PTR,
            options(noreturn)
        );
    }
}

impl GdtEntry {
    const fn null() -> Self {
        Self(0)
    }

    const fn code() -> Self {
        Self(0x00AF_9A00_0000_0000)
    }

    const fn data() -> Self {
        Self(0x00CF_9200_0000_0000)
    }

    const fn ucode() -> Self {
        Self(0x00AF_FA00_0000_0000)
    }

    const fn udata() -> Self {
        Self(0x00CF_F200_0000_0000)
    }
}

impl DescriptorTablePointer {
    const fn uninit() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

impl PageTable {
    const fn uninit() -> Self {
        // Safety: 全零填充的页表为空页表
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

impl Deref for PageTable {
    type Target = [u64; 512];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PageTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
