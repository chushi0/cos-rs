use core::{
    arch::{asm, naked_asm},
    cmp::Ordering,
};

use crate::{kprintln, memory, multitask, sync::percpu};

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
        // 我们先存一下参数
        "push rsi",
        "push rdx",
        "push r10",
        "push r8",
        "push r9",
        // 查表获取中断处理函数
        "mov rsi, rdi",
        "mov rdi, rax",
        "sub rsp, 8",
        "mov rdx, rsp",
        "call {query_syscall_handler}",
        // 调用中断处理函数
        "pop rax",
        "pop r8",
        "pop rcx",
        "pop rdx",
        "pop rsi",
        "pop rdi",
        "cmp rax, 0",
        "je 0f",
        "call rax",
        "jmp 1f",
        // 中断处理函数不存在的情况
        "0:",
        "mov rax, {syscall_not_found}",
        // 中断处理函数调用结束
        "1:",
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
        query_syscall_handler = sym query_syscall_handler,
        syscall_not_found = const cos_sys::error::ErrorKind::BadArgument as u64,
    )
}

type SyscallEntry = (u64, u64, extern "C" fn(u64, u64, u64, u64, u64) -> u64);
const SYSCALL_HANDLER: &[SyscallEntry] = &[
    (0, 0, syscall_test),
    (
        cos_sys::idx::IDX_EXIT,
        cos_sys::idx::IDX_SUB_EXIT_PROCESS,
        syscall_exit,
    ),
    (
        cos_sys::idx::IDX_EXIT,
        cos_sys::idx::IDX_SUB_EXIT_THREAD,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_THREAD,
        cos_sys::idx::IDX_SUB_THREAD_CURRENT,
        syscall_current_thread,
    ),
    (
        cos_sys::idx::IDX_THREAD,
        cos_sys::idx::IDX_SUB_THREAD_SUSPEND,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_THREAD,
        cos_sys::idx::IDX_SUB_THREAD_RESUME,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_THREAD,
        cos_sys::idx::IDX_SUB_THREAD_KILL,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_THREAD,
        cos_sys::idx::IDX_SUB_THREAD_CREATE,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_MEMORY,
        cos_sys::idx::IDX_SUB_MEMORY_ALLOC,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_MEMORY,
        cos_sys::idx::IDX_SUB_MEMORY_FREE,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_MEMORY,
        cos_sys::idx::IDX_SUB_MEMORY_TEST,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_MEMORY,
        cos_sys::idx::IDX_SUB_MEMORY_MODIFY,
        syscall_test,
    ),
];

// assert
const _: () = {
    let len = SYSCALL_HANDLER.len();
    let mut i = 0;
    while i + 1 < len {
        assert!(matches!(
            cmp_syscall_handler(&SYSCALL_HANDLER[i], &SYSCALL_HANDLER[i + 1]),
            Ordering::Less
        ));
        i += 1;
    }

    const fn cmp_syscall_handler(entry1: &SyscallEntry, entry2: &SyscallEntry) -> Ordering {
        if entry1.0 != entry2.0 {
            if entry1.0 < entry2.0 {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        } else {
            if entry1.1 < entry2.1 {
                Ordering::Less
            } else if entry1.1 > entry2.1 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
    }
};

/// 查询syscall
///
/// 查询 (id, sub_id) 对应的系统调用编号。
/// 如果存在，将地址写入ptr。如果不存在，将0写入ptr。
///
/// Safety:
/// 调用方保证ptr是一个可以写入的指针
unsafe extern "C" fn query_syscall_handler(id: u64, sub_id: u64, ptr: *mut u64) {
    let handler = SYSCALL_HANDLER
        .binary_search_by(|&(entry_id, entry_sub_id, _)| {
            (entry_id, entry_sub_id).cmp(&(id, sub_id))
        })
        .map_or(0, |index| SYSCALL_HANDLER[index].2 as u64);
    unsafe {
        *ptr = handler;
    }
}

const SYSCALL_SUCCESS: u64 = cos_sys::error::ErrorKind::Success as u64;

macro_rules! syscall_handler {
    (fn $name:ident() -> u64 { $($t:tt)* }) => {
        extern "C" fn $name(_p1: u64, _p2: u64, _p3: u64, _p4: u64, _p5: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64) -> u64 { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, _p2: u64, _p3: u64, _p4: u64, _p5: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64) -> u64 { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, $p2: u64, _p3: u64, _p4: u64, _p5: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64) -> u64 { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, _p4: u64, _p5: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64) -> u64 { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, _p5: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64, $p5:ident: u64) -> u64 { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, $p5: u64) -> u64 { $($t)* }
    };

    (fn $name:ident() { $($t:tt)* }) => {
        extern "C" fn $name(_p1: u64, _p2: u64, _p3: u64, _p4: u64, _p5: u64) -> u64 { (|| { $($t)* })(); SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64) { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, _p2: u64, _p3: u64, _p4: u64, _p5: u64) -> u64 { (|| { $($t)* })(); SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64) { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, $p2: u64, _p3: u64, _p4: u64, _p5: u64) -> u64 { (|| { $($t)* })(); SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64) { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, _p4: u64, _p5: u64) -> u64 { (|| { $($t)* })(); SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64) { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, _p5: u64) -> u64 { (|| { $($t)* })(); SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64, $p5:ident: u64) { $($t:tt)* }) => {
        extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, $p5: u64) -> u64 { (|| { $($t)* })(); SYSCALL_SUCCESS }
    };
}

syscall_handler! {
    fn syscall_test() {
        kprintln!("syscall test pass");
    }
}

syscall_handler! {
    fn syscall_exit(code: u64) {
        kprintln!("calling syscall exit {code}");

        // 在thread_yield执行前，必须释放全部临时对象
        // 因为thread_yield不会再返回，若不释放会导致内存泄漏
        {
            let current_thread = multitask::thread::current_thread().unwrap();
            let process_id = current_thread.lock().process_id.unwrap();
            let process = multitask::process::get_process(process_id.get()).unwrap();
            multitask::process::set_exit_code(&process, code);
            multitask::process::stop_all_thread(&process);
        }

        multitask::thread::thread_yield(true);

        // 当前线程已经结束，且已让出，调度器不应该再回到当前线程执行
        unreachable!()
    }
}

syscall_handler! {
    fn syscall_current_thread(thread_id_ptr: u64) -> u64 {
        let thread_id_ptr = thread_id_ptr as *mut u64;
        if !memory::physics::is_user_space_virtual_memory(thread_id_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }
        let thread_id = percpu::get_current_thread_id();
        // Safety:
        // 1. 我们已经检查指针位于用户空间范围内，写入不会影响内核数据
        // 2. 如果该页未映射或未对齐，写入会导致软中断产生，但在此处触发软中断不会有问题（与用户态程序触发软中断一致）
        unsafe {
            *thread_id_ptr = thread_id;
        }
        SYSCALL_SUCCESS
    }
}
