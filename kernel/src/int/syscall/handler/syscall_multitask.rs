use alloc::sync::Arc;

use crate::{
    int::syscall::SYSCALL_SUCCESS, memory, multitask, syscall_handler, user::handle::HandleObject,
};

syscall_handler! {
    fn syscall_exit(code: u64) {
        // 在thread_yield执行前，必须释放全部临时对象
        // 因为thread_yield不会再返回，若不释放会导致内存泄漏
        {
            let process = multitask::process::current_process().unwrap();
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
    fn syscall_create_process(exe_ptr: u64, exe_len: u64, process_handle_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(exe_ptr as usize) ||
            !memory::page::is_user_space_virtual_memory((exe_ptr + exe_len) as usize) ||
            !memory::page::is_user_space_virtual_memory(process_handle_ptr as usize) {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let process = multitask::process::current_process().unwrap();

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
        if !memory::page::is_user_space_virtual_memory(exit_code_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let process = multitask::process::current_process().unwrap();

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
