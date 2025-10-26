use core::arch::{asm, naked_asm};

use crate::sync::percpu;

pub(super) unsafe fn init() {
    const IA32_EFER: u32 = 0xc0000080;
    const IA32_STAR: u32 = 0xC0000081;
    const IA32_LSTAR: u32 = 0xC0000082;
    const IA32_FMASK: u32 = 0xC0000084;

    // 开启SCE
    unsafe {
        let mut efer_low: u32;
        let mut efer_high: u32;
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
    const USER_SS: u16 = 0x3B;
    const KERNEL_CS: u16 = 0x18;
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") IA32_STAR,
            in("eax") 0,
            in("edx") ((USER_SS as u32 - 0x08) << 16) | (KERNEL_CS as u32),
            options(nostack, preserves_flags)
        );
    }

    // 设置syscall入口地址
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") IA32_LSTAR,
            in("eax") ((syscall_entry as u64) & 0xFFFF_FFFF) as u32,
            in("edx") (((syscall_entry as u64) >> 32) & 0xFFFF_FFFF) as u32,
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
extern "C" fn syscall_entry() {
    naked_asm!(
        // 切到内核gs
        "swapgs",
        // 切换栈
        "mov qword ptr gs:[{syscall_user_stack_offset}], rsp",
        "mov rsp, gs:[{syscall_stack_offset}]",
        // 保存现场
        "push rcx",
        "push r11",
        "push qword ptr gs:[{syscall_user_stack_offset}]",
        "push rbp",
        "mov rbp, rsp",
        // 对齐栈
        "and rsp, 0xfffffffffffffff0",
        // 开中断
        "sti",
        // 调用中断处理函数
        "call {syscall_handler}",
        // 关中断
        "cli",
        // 恢复现场
        "mov rsp, rbp",
        "pop rbp",
        "pop qword ptr gs:[{syscall_user_stack_offset}]",
        "pop r11",
        "pop rcx",
        "mov rsp, qword ptr gs:[{syscall_user_stack_offset}]",
        // 切回用户gs
        "swapgs",
        // 返回用户态
        "sysretq",
        syscall_user_stack_offset = const percpu::OFFSET_SYSCALL_USER_STACK,
        syscall_stack_offset = const percpu::OFFSET_SYSCALL_STACK,
        syscall_handler = sym syscall_handler
    )
}

extern "C" fn syscall_handler() -> u64 {
    0
}
