use core::time::Duration;

use alloc::sync::Arc;
use async_locks::channel::oneshot;

use crate::{
    memory,
    multitask::{self, thread::thread_yield},
    syscall::SYSCALL_SUCCESS,
    syscall_handler,
    user::handle::HandleObject,
};

syscall_handler! {
    fn exit_process(code: u64) {
        // 在thread_yield执行前，必须释放全部临时对象
        // 因为thread_yield不会再返回，若不释放会导致内存泄漏
        {
            let process = multitask::process::current_process().unwrap();
            multitask::process::set_exit_code(&process, code);
            multitask::process::stop_all_thread(&process, code);
        }

        multitask::thread::thread_yield(true);

        // 当前线程已经结束，且已让出，调度器不应该再回到当前线程执行
        unreachable!()
    }
}

syscall_handler! {
    fn exit_thread(code: u64) {
        {
            let current_thread = multitask::thread::current_thread().unwrap();
            multitask::thread::stop_thread(&current_thread, code);
        }

        multitask::thread::thread_yield(true);
        unreachable!()
    }
}

syscall_handler! {
    fn current_thread(thread_handle_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(thread_handle_ptr as usize) {
            return  cos_sys::error::ErrorKind::BadPointer as u64;
        }

        let process = multitask::process::current_process().unwrap();
        let thread = multitask::thread::current_thread().unwrap();
        let thread_handle = HandleObject::Thread {
            thread: Arc::downgrade(&thread),
            exit: multitask::thread::get_exit_code_subscriber(&thread),
        };
        let handle = multitask::process::insert_process_handle(&process, thread_handle);

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, thread_handle_ptr, &handle).is_err() {
                return cos_sys::error::ErrorKind::BadPointer as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn wait_thread(addr: u64, expected: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(addr as usize) {
            return cos_sys::error::ErrorKind::BadPointer as u64;
        }

        let process = multitask::process::current_process().unwrap();
        let (sender, receiver) = oneshot::channel();
        if multitask::process::register_futex_if_match(&process, addr, expected, sender).is_err() {
            return cos_sys::error::ErrorKind::BadPointer as u64;
        }

        let wait = multitask::async_rt::block_on(async move {
            _ = receiver.recv().await;
        });
        if wait.is_err() {
            thread_yield(true);
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn wake_thread(addr: u64, count: u64) -> u64 {
        let process = multitask::process::current_process().unwrap();
        multitask::process::wake_futex(&process, addr, count);

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn kill_thread(thread_handle: u64) -> u64 {
        let process = multitask::process::current_process().unwrap();
        let Some(thread_handle) = multitask::process::get_process_handle(&process, thread_handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let HandleObject::Thread { thread, .. } = &*thread_handle else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        if let Some(thread) = thread.upgrade() {
            multitask::thread::stop_thread(&thread, cos_sys::multitask::EXIT_KILL);
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn create_thread(rip: u64, rsp: u64, params: u64, thread_handle_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(thread_handle_ptr as usize) {
            return cos_sys::error::ErrorKind::BadPointer as u64;
        }

        let process = multitask::process::current_process().unwrap();

        let Some(thread) = multitask::process::create_user_thread(&process, rip, rsp, params) else {
            return cos_sys::error::ErrorKind::OutOfMemory as u64;
        };
        let thread_handle = HandleObject::Thread {
            thread: Arc::downgrade(&thread),
            exit: multitask::thread::get_exit_code_subscriber(&thread),
        };
        let handle = multitask::process::insert_process_handle(&process, thread_handle);

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, thread_handle_ptr, &handle).is_err() {
                return cos_sys::error::ErrorKind::BadPointer as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn join_thread(thread_handle: u64, exit_code_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(exit_code_ptr as usize) {
            return cos_sys::error::ErrorKind::BadPointer as u64;
        }

        let process = multitask::process::current_process().unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, thread_handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let HandleObject::Thread { exit, .. } = &*handle else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let mut exit = exit.clone();
        let block_on = multitask::async_rt::block_on(async {
            loop {
                if exit.wait().await.is_err() {
                    break;
                }
            }
        });
        if block_on.is_err() {
            return cos_sys::error::ErrorKind::Unknown as u64;
        }

        multitask::process::remove_process_handle(&process, thread_handle as usize);

        let code = *exit.borrow();
        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, exit_code_ptr, &code).is_err() {
                return cos_sys::error::ErrorKind::BadPointer as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn sleep_thread(time_in_seconds: u64, time_in_ns: u64) {
        let duration = Duration::new(time_in_seconds, time_in_ns as u32);
        let sleep = multitask::async_task::sleep(duration);
        if multitask::async_rt::block_on(sleep).is_err() {
            thread_yield(true);
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn current_process(process_handle_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(process_handle_ptr as usize) {
            return  cos_sys::error::ErrorKind::BadPointer as u64;
        }

        let process = multitask::process::current_process().unwrap();
        let process_handle = HandleObject::Process {
            process: Arc::downgrade(&process),
            exit: multitask::process::get_exit_code_subscriber(&process),
        };
        let handle = multitask::process::insert_process_handle(&process, process_handle);

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, process_handle_ptr, &handle).is_err() {
                return cos_sys::error::ErrorKind::BadPointer as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn create_process(exe_ptr: u64, exe_len: u64, process_handle_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(exe_ptr as usize) ||
            !memory::page::is_user_space_virtual_memory((exe_ptr + exe_len) as usize) ||
            !memory::page::is_user_space_virtual_memory(process_handle_ptr as usize) {
                return cos_sys::error::ErrorKind::BadPointer as u64;
        }

        let process = multitask::process::current_process().unwrap();

        let mut exe = alloc::vec![0u8; exe_len as usize];
        unsafe {
            if multitask::process::read_user_process_memory(&process, exe_ptr, exe.as_mut_ptr(), exe_len as usize).is_err() {
                return cos_sys::error::ErrorKind::BadPointer as u64;
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

        let created_process = match multitask::async_rt::block_on(receiver.recv()) {
            Ok(res) => res,
            Err(_) => return cos_sys::error::ErrorKind::Unknown as u64,
        };
        let created_process = match created_process.unwrap() {
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
                return cos_sys::error::ErrorKind::BadPointer as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn kill_process(process_handle: u64) -> u64 {
        let process = multitask::process::current_process().unwrap();
        let Some(process_handle) = multitask::process::get_process_handle(&process, process_handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let HandleObject::Process { process, .. } = &*process_handle else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        if let Some(process) = process.upgrade() {
            multitask::process::set_exit_code(&process, cos_sys::multitask::EXIT_KILL);
            multitask::process::stop_all_thread(&process, cos_sys::multitask::EXIT_KILL);
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn wait_process(process_handle: u64, exit_code_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(exit_code_ptr as usize) {
            return cos_sys::error::ErrorKind::BadPointer as u64;
        }

        let process = multitask::process::current_process().unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, process_handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let HandleObject::Process { exit, .. } = &*handle else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let mut exit = exit.clone();
        let block_on = multitask::async_rt::block_on(async {
            loop {
                if exit.wait().await.is_err() {
                    break;
                }
            }
        });
        if block_on.is_err() {
            return cos_sys::error::ErrorKind::Unknown as u64;
        }

        multitask::process::remove_process_handle(&process, process_handle as usize);

        let code = *exit.borrow();
        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, exit_code_ptr, &code).is_err() {
                return cos_sys::error::ErrorKind::BadPointer as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}
