use core::{arch::asm, mem::offset_of};

use alloc::boxed::Box;

#[derive(Default)]
pub struct PerCpuStruct {
    // 当前的线程ID
    pub current_thread_id: u64,
    // 当前线程syscall使用的栈地址（高地址）
    pub syscall_stack: u64,
    // 用于调用syscall时，暂存用户态rsp
    pub syscall_user_stack: u64,
    // IDLE线程id
    pub idle_thread_id: u64,
    // kernel async 线程id
    pub kernel_async_thread_id: u64,
}

macro_rules! per_cpu_data {
    ($ident:ident, $offset_const:ident, $setter:ident, $getter:ident) => {
        pub const $offset_const: usize = offset_of!(PerCpuStruct, $ident);

        #[inline(never)]
        pub fn $setter(val: u64) {
            unsafe {
                asm!(
                    "mov qword ptr gs:[{offset}], {val}",
                    offset = const $offset_const,
                    val = in(reg) val,
                )
            }
        }

        pub fn $getter() -> u64 {
            let mut data: u64;
            unsafe {
                asm!(
                    "mov {val}, qword ptr gs:[{offset}]",
                    offset = const $offset_const,
                    val = out(reg) data,
                )
            }
            data
        }
    };
}

per_cpu_data!(
    current_thread_id,
    OFFSET_CURRENT_THREAD_ID,
    set_current_thread_id,
    get_current_thread_id
);

per_cpu_data!(
    syscall_stack,
    OFFSET_SYSCALL_STACK,
    set_syscall_stack,
    get_syscall_stack
);

per_cpu_data!(
    syscall_user_stack,
    OFFSET_SYSCALL_USER_STACK,
    set_syscall_user_stack,
    get_syscall_user_stack
);

per_cpu_data!(
    idle_thread_id,
    OFFSET_IDLE_THREAD_ID,
    set_idle_thread_id,
    get_idle_thread_id
);

per_cpu_data!(
    kernel_async_thread_id,
    OFFSET_KERNEL_ASYNC_THREAD_ID,
    set_kernel_async_thread_id,
    get_kernel_async_thread_id
);

const IA32_KERNEL_GS_BASE: u64 = 0xC0000102;

pub unsafe fn init() {
    let per_cpu_struct = Box::new(PerCpuStruct::default());
    let per_cpu_struct = Box::leak(per_cpu_struct) as *mut PerCpuStruct as usize as u64;
    set_k_gs_base(per_cpu_struct);
    unsafe {
        asm!("swapgs", options(nostack, preserves_flags));
    }
}

fn set_k_gs_base(data: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") IA32_KERNEL_GS_BASE,
            in("eax") (data & 0xFFFF_FFFF) as u32,
            in("edx") ((data >> 32) & 0xFFFF_FFFF) as u32,
            options(nostack, preserves_flags)
        )
    }
}

#[allow(unused)]
fn get_k_gs_base() -> u64 {
    let mut low: u32;
    let mut high: u32;
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") IA32_KERNEL_GS_BASE,
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags)
        );
    }
    low as u64 | ((high as u64) << 32)
}
