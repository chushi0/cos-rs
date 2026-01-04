use crate::{
    memory,
    multitask::{self, process::ProcessPageType},
    syscall_handler,
    syscall::SYSCALL_SUCCESS,
};

syscall_handler! {
    fn alloc_page(count: u64, addr_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(addr_ptr as usize) {
            return cos_sys::error::ErrorKind::BadPointer as u64;
        }

        if count == 0 {
            return SYSCALL_SUCCESS;
        }

        let process = multitask::process::current_process().unwrap();

        let Some(addr) = multitask::process::create_process_page(&process, (count * 0x1000) as usize, ProcessPageType::Data) else {
            return cos_sys::error::ErrorKind::OutOfMemory as u64;
        };

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, addr_ptr, &addr).is_err() {
                return cos_sys::error::ErrorKind::BadPointer as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn free_page(addr: u64, count: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(addr as usize)
            || !memory::page::is_user_space_virtual_memory((addr + count * 0x1000) as usize) {
            return cos_sys::error::ErrorKind::BadPointer as u64;
        }

        if count == 0 {
            return SYSCALL_SUCCESS;
        }

        let process = multitask::process::current_process().unwrap();
        unsafe {
            multitask::process::free_process_page(&process, addr as usize, (count * 0x1000) as usize);
        }

        SYSCALL_SUCCESS
    }
}
