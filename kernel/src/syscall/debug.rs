use core::mem::MaybeUninit;

use crate::{
    io, kprint, kprintln, memory, multitask,
    sync::{int::IrqGuard, percpu},
    syscall::SYSCALL_SUCCESS,
    syscall_handler,
};

syscall_handler! {
    fn syscall_test() {
        kprintln!("syscall test pass");
    }
}

syscall_handler! {
    fn get_char(char_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(char_ptr as usize) {
            return cos_sys::error::ErrorKind::BadPointer as u64;
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
        let char = match char {
            Ok(ch) => ch,
            Err(_) => return cos_sys::error::ErrorKind::Unknown as u64,
        };

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, char_ptr, &char).is_err() {
                return cos_sys::error::ErrorKind::BadPointer as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn put_char(char_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(char_ptr as usize) {
            return cos_sys::error::ErrorKind::BadPointer as u64;
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
                return cos_sys::error::ErrorKind::BadPointer as u64;
            }
            char = uninit_char.assume_init();
        };

        kprint!("{}", char as char);

        SYSCALL_SUCCESS
    }
}
