use core::{
    arch::{asm, naked_asm},
    cmp::Ordering,
    mem::MaybeUninit,
};

use alloc::sync::Arc;

use crate::{
    io, kprint, kprintln, memory, multitask,
    sync::{int::IrqGuard, percpu},
    user::handle::{FileHandleObject, HandleObject},
};

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
    (
        cos_sys::idx::IDX_DEBUG,
        cos_sys::idx::IDX_SUB_DEBUG_INFO,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_DEBUG,
        cos_sys::idx::IDX_SUB_DEBUG_GET_CHAR,
        syscall_debug_get_char,
    ),
    (
        cos_sys::idx::IDX_DEBUG,
        cos_sys::idx::IDX_SUB_DEBUG_PUT_CHAR,
        syscall_debug_put_char,
    ),
    (
        cos_sys::idx::IDX_EXIT,
        cos_sys::idx::IDX_SUB_EXIT_PROCESS,
        syscall_exit,
    ),
    (
        cos_sys::idx::IDX_EXIT,
        cos_sys::idx::IDX_SUB_EXIT_THREAD,
        syscall_exit_thread,
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
    (
        cos_sys::idx::IDX_PROCESS,
        cos_sys::idx::IDX_SUB_PROCESS_CURRENT,
        syscall_current_process,
    ),
    (
        cos_sys::idx::IDX_PROCESS,
        cos_sys::idx::IDX_SUB_PROCESS_CREATE,
        syscall_create_process,
    ),
    (
        cos_sys::idx::IDX_PROCESS,
        cos_sys::idx::IDX_SUB_PROCESS_KILL,
        syscall_test,
    ),
    (
        cos_sys::idx::IDX_PROCESS,
        cos_sys::idx::IDX_SUB_PROCESS_WAIT,
        syscall_wait_process,
    ),
    (
        cos_sys::idx::IDX_FILE,
        cos_sys::idx::IDX_SUB_FILE_CREATE,
        syscall_file_create,
    ),
    (
        cos_sys::idx::IDX_FILE,
        cos_sys::idx::IDX_SUB_FILE_OPEN,
        syscall_file_open,
    ),
    (
        cos_sys::idx::IDX_FILE,
        cos_sys::idx::IDX_SUB_FILE_READ,
        syscall_file_read,
    ),
    (
        cos_sys::idx::IDX_FILE,
        cos_sys::idx::IDX_SUB_FILE_WRITE,
        syscall_file_write,
    ),
    (
        cos_sys::idx::IDX_FILE,
        cos_sys::idx::IDX_SUB_FILE_GET_POS,
        syscall_file_get_pos,
    ),
    (
        cos_sys::idx::IDX_FILE,
        cos_sys::idx::IDX_SUB_FILE_SET_POS,
        syscall_file_set_pos,
    ),
    (
        cos_sys::idx::IDX_FILE,
        cos_sys::idx::IDX_SUB_FILE_CLOSE,
        syscall_file_close,
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
    fn syscall_debug_get_char(char_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(char_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let receiver = io::keyboard::receiver();
        let char = multitask::async_rt::block_on(async {
            receiver.lock().await.recv().await.unwrap()
        });

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, char_ptr, &char).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_debug_put_char(char_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(char_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let char: u8;
        unsafe {
            let mut uninit_char = MaybeUninit::uninit();
            if multitask::process::read_user_process_memory_struct(&process, char_ptr, &mut uninit_char).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
            char = uninit_char.assume_init();
        };

        kprint!("{}", char as char);

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_exit(code: u64) {
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
    fn syscall_exit_thread(code: u64) {
        // TODO: 当前未实现线程退出码
        _ = code;
        {
            let current_thread = multitask::thread::current_thread().unwrap();
            multitask::thread::stop_thread(&current_thread);
        }

        multitask::thread::thread_yield(true);
        unreachable!()
    }
}

syscall_handler! {
    fn syscall_current_thread(thread_id_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(thread_id_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }
        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, thread_id_ptr, &thread_id).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }
        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_current_process(process_id_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(process_id_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }
        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();
        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, process_id_ptr, &process_id).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }
        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_create_process(exe_ptr: u64, exe_len: u64, process_handle_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(exe_ptr as usize) ||
            !memory::physics::is_user_space_virtual_memory((exe_ptr + exe_len) as usize) ||
            !memory::physics::is_user_space_virtual_memory(process_handle_ptr as usize) {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let mut exe = alloc::vec![0u8; exe_len as usize];
        unsafe {
            if multitask::process::read_user_process_memory(&process, exe_ptr, exe.as_mut_ptr(), exe_len as usize).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let Ok(exe_str) = str::from_utf8(&exe) else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument)).await;
                return;
            };

            if let Some(process) = multitask::process::create_user_process(exe_str).await {
                sender.send(Ok(process)).await;
            } else {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown)).await; // TODO: 占位，应当返回具体错误类型
            }
        });

        let created_process = match multitask::async_rt::block_on(receiver.recv()).unwrap() {
            Ok(process) => process,
            Err(err) => return err as u64,
        };

        let handle = HandleObject::Process {
            process: Arc::downgrade(&created_process),
            exit: multitask::process::get_exit_code_subscriber(&created_process),
        };

        let handle = multitask::process::insert_process_handle(&process, handle) as u64;

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, process_handle_ptr, &handle).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_wait_process(process_handle: u64, exit_code_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(exit_code_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, process_handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let HandleObject::Process { exit, .. } = &*handle else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let mut exit = exit.clone();
        multitask::async_rt::block_on(async {
            loop {
                if exit.wait().await.is_err() {
                    break;
                }
            }
        });

        multitask::process::remove_process_handle(&process, process_handle as usize);

        let code = *exit.borrow();
        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, exit_code_ptr, &code).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_file_create(path_ptr: u64, path_len: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(path_ptr as usize) ||
            !memory::physics::is_user_space_virtual_memory((path_ptr + path_len) as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let mut path = alloc::vec![0u8; path_len as usize];
        unsafe {
            if multitask::process::read_user_process_memory(&process, path_ptr, path.as_mut_ptr(), path_len as usize).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        let filesystem = io::disk::FILE_SYSTEMS.lock().get(&0).cloned().unwrap();
        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let Ok(path) = filesystem::path::PathBuf::from_bytes(&path) else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };
            if filesystem.create_file(path.as_path()).await.is_err() {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await; // TODO: 错误类型占位
                return ;
            }
            sender.send(Ok(())).await;
        });

        let result = multitask::async_rt::block_on(receiver.recv()).unwrap();
        match result {
            Ok(()) => SYSCALL_SUCCESS,
            Err(error) => error,
        }
    }
}

syscall_handler! {
    fn syscall_file_open(path_ptr: u64, path_len: u64, handle_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(path_ptr as usize) ||
            !memory::physics::is_user_space_virtual_memory((path_ptr + path_len) as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let mut path = alloc::vec![0u8; path_len as usize];
        unsafe {
            if multitask::process::read_user_process_memory(&process, path_ptr, path.as_mut_ptr(), path_len as usize).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        let filesystem = io::disk::FILE_SYSTEMS.lock().get(&0).cloned().unwrap();
        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let Ok(path) = filesystem::path::PathBuf::from_bytes(&path) else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };
            let Ok(handle) = filesystem.open_file(path.as_path()).await else {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await; // TODO: 错误类型占位
                return ;
            };
            sender.send(Ok(handle)).await;
        });

        let handle = multitask::async_rt::block_on(receiver.recv()).unwrap();
        let handle = match handle {
            Ok(handle) => handle,
            Err(error) => return error,
        };

        let file_handle = FileHandleObject::new(handle);
        let handle = multitask::process::insert_process_handle(&process, HandleObject::File(file_handle)) as u64;

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, handle_ptr, &handle).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_file_read(handle: u64, buffer_ptr: u64, buffer_len: u64, read_count_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(buffer_ptr as usize) ||
            !memory::physics::is_user_space_virtual_memory((buffer_ptr + buffer_len) as usize) ||
            !memory::physics::is_user_space_virtual_memory(read_count_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let mut buffer = alloc::vec![0u8; buffer_len as usize];
        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let HandleObject::File(handle) = &*handle else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };

            let mut file = handle.lock().await;
            let Ok(count) = file.read(&mut buffer).await else {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await;
                return;
            };
            sender.send(Ok((count, buffer))).await;
        });
        let (read_count, buffer) = match multitask::async_rt::block_on(receiver.recv()).unwrap() {
            Ok(read) => read,
            Err(error) => return error,
        };

        unsafe {
            if multitask::process::write_user_process_memory(&process, buffer_ptr, buffer.as_ptr(), read_count as usize).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
            if multitask::process::write_user_process_memory_struct(&process, read_count_ptr, &read_count).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_file_write(handle: u64, buffer_ptr: u64, buffer_len: u64, write_count_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(buffer_ptr as usize) ||
            !memory::physics::is_user_space_virtual_memory((buffer_ptr + buffer_len) as usize) ||
            !memory::physics::is_user_space_virtual_memory(write_count_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let mut buffer = alloc::vec![0u8; buffer_len as usize];
        unsafe {
            if multitask::process::read_user_process_memory(&process, buffer_ptr, buffer.as_mut_ptr(), buffer_len as usize).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let HandleObject::File(handle) = &*handle else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };

            let mut file = handle.lock().await;
            let Ok(count) = file.write(&buffer).await else {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await;
                return;
            };
            sender.send(Ok(count)).await;
        });
        let read_count = match multitask::async_rt::block_on(receiver.recv()).unwrap() {
            Ok(read_count) => read_count,
            Err(error) => return error,
        };

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, write_count_ptr, &read_count).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_file_close(handle: u64) {
        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        multitask::process::remove_process_handle(&process, handle as usize);
    }
}

syscall_handler! {
    fn syscall_file_get_pos(handle: u64, pos_ptr: u64) -> u64 {
        if !memory::physics::is_user_space_virtual_memory(pos_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let HandleObject::File(handle) = &*handle else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };

            let mut file = handle.lock().await;
            let Ok(count) = file.get_pointer().await else {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await;
                return;
            };
            sender.send(Ok(count)).await;
        });
        let pos = match multitask::async_rt::block_on(receiver.recv()).unwrap() {
            Ok(pos) => pos,
            Err(error) => return error,
        };

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, pos_ptr, &pos).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_file_set_pos(handle: u64, pos: u64) -> u64 {
        let thread_id = percpu::get_current_thread_id();
        let process_id = {
            let _guard = IrqGuard::cli();
            multitask::thread::get_thread(thread_id).unwrap().lock().process_id.unwrap().get()
        };
        let process = multitask::process::get_process(process_id).unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let HandleObject::File(handle) = &*handle else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };

            let mut file = handle.lock().await;
            if file.move_pointer(pos).await.is_err() {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await;
                return;
            };
            sender.send(Ok(())).await;
        });
        match multitask::async_rt::block_on(receiver.recv()).unwrap() {
            Ok(()) => SYSCALL_SUCCESS,
            Err(error) => error,
        }
    }
}
