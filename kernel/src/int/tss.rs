use core::{arch::asm, mem::MaybeUninit, slice};

use crate::sync::IrqGuard;

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct TaskStateSegment {
    _reserved1: u32,
    rsp0: u64,
    rsp1: u64,
    rsp2: u64,
    _reserved2: u64,
    ist: [u64; 7],
    _reserved3: u64,
    _reserved4: u16,
    iopb: u16,
}

const _: () = {
    assert!(size_of::<TaskStateSegment>() == 0x68);
};

static mut MAIN_CPU_TSS: TaskStateSegment = TaskStateSegment::null();

pub(super) unsafe fn init() {
    unsafe {
        // 设置iopb
        MAIN_CPU_TSS.iopb = size_of::<TaskStateSegment>() as u16;

        // 获取gdt
        let mut gdt_desc = MaybeUninit::<DescriptorTablePointer>::zeroed().assume_init();
        asm!(
            "sgdt [{}]",
            in(reg) &raw mut gdt_desc,
            options(nostack, preserves_flags),
        );

        let entry_count = ((gdt_desc.limit as usize) + 1) / size_of::<u64>();
        let gdt = slice::from_raw_parts_mut(gdt_desc.base as *mut u64, entry_count);

        // 设置gdt
        let tss_addr = &raw const MAIN_CPU_TSS as u64;
        let tss_limit = (size_of::<TaskStateSegment>() - 1) as u64;

        gdt[5] = ((tss_limit & 0xFFFF) as u64)
            | ((tss_addr & 0xFFFFFF) << 16)
            | (0x89u64 << 40)
            | ((tss_limit & 0xF0000) << 32)
            | (((tss_addr >> 24) & 0xFF) << 56);
        gdt[6] = tss_addr >> 32;

        // 重新提交给硬件
        asm!(
            "lgdt [{}]",
            in(reg) &raw const gdt_desc,
            options(nostack, preserves_flags)
        );

        // 加载TSS
        asm!(
            "ltr ax",
            in("ax") 0x28u16,
            options(nostack, preserves_flags)
        );
    }
}

pub unsafe fn set_rsp0(rsp: u64) {
    unsafe {
        let _guard = IrqGuard::cli();
        MAIN_CPU_TSS.rsp0 = rsp;
    }
}

impl TaskStateSegment {
    const fn null() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}
