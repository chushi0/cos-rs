use core::arch::{asm, naked_asm};

pub(super) unsafe fn init() {
    const IA32_EFER: u32 = 0xc000_0080;
    const IA32_STAR: u32 = 0xC0000081;
    const IA32_LSTAR: u32 = 0xC0000082;
    const IA32_FMASK: u32 = 0xC0000084;

    // 开启SCE
    unsafe {
        #[allow(unused_assignments)]
        let mut efer_low = 0u32;
        #[allow(unused_assignments)]
        let mut efer_high = 0u32;
        asm!(
            "rdmsr",
            in("ecx") IA32_EFER,
            out("eax") efer_low,
            out("edx") efer_high,
            options(nostack, preserves_flags)
        );
        efer_low |= 1;
        asm!(
            "wrmsr",
            in("ecx") IA32_EFER,
            in("eax") efer_low,
            in("edx") efer_high,
            options(nostack, preserves_flags)
        )
    }

    // 设置代码段
    const USER_CS: u16 = 0x3B;
    const KERNEL_CS: u16 = 0x08;
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") IA32_STAR,
            in("eax") ((USER_CS as u32 )<< 16) | (KERNEL_CS as u32),
            in("edx") 0,
            options(nostack, preserves_flags)
        );
    }

    // 设置syscall入口地址
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") IA32_LSTAR,
            in("eax") ((syscall_enter as u64) & 0xFFFF_FFFF) as u32,
            in("edx") (((syscall_enter as u64) >> 32) & 0xFFFF_FFFF) as u32,
            options(nostack, preserves_flags)
        );
    }

    // 设置syscall后立刻关中断
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") IA32_FMASK,
            in("eax") (1 << 9),
            in("edx") 0,
            options(nostack, preserves_flags)
        )
    }
}

/// syscall入口
///
/// 在用户态调用syscall后，硬件会完成特权级切换、cs/ss切换、rip切换、rflags更新
/// 注意：syscall不会切换栈！！
///
/// 硬件执行：
/// rcx: 用户态下一条指令地址
/// r11: 当前rflags
///
/// 入参：
/// rax: 系统调用编号
/// rdi: 参数1
/// rsi: 参数2
/// rdx: 参数3
/// r10: 参数4
/// r8: 参数5
/// r9: 参数6
///
/// 返回：rax
#[unsafe(naked)]
extern "C" fn syscall_enter() {
    naked_asm!(
        // 返回用户态
        "sysretq"
    )
}
