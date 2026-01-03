use core::{cmp::Ordering, mem::MaybeUninit};

use crate::{
    trap::syscall::{SYSCALL_SUCCESS, SyscallEntry},
    io, kprint, kprintln, memory, multitask,
    sync::{int::IrqGuard, percpu},
    syscall_handler,
};

mod syscall_file;
mod syscall_memory;
mod syscall_multitask;

pub const SYSCALL_HANDLER: &[SyscallEntry] = &[
    (
        cos_sys::idx::IDX_EXIT_PROCESS,
        syscall_multitask::syscall_exit,
    ),
    (
        cos_sys::idx::IDX_EXIT_THREAD,
        syscall_multitask::syscall_exit_thread,
    ),
    (cos_sys::idx::IDX_THREAD_CURRENT, syscall_test),
    (cos_sys::idx::IDX_THREAD_SUSPEND, syscall_test),
    (cos_sys::idx::IDX_THREAD_RESUME, syscall_test),
    (cos_sys::idx::IDX_THREAD_KILL, syscall_test),
    (cos_sys::idx::IDX_THREAD_CREATE, syscall_test),
    (
        cos_sys::idx::IDX_MEMORY_ALLOC,
        syscall_memory::syscall_alloc_page,
    ),
    (cos_sys::idx::IDX_MEMORY_FREE, syscall_test),
    (cos_sys::idx::IDX_MEMORY_TEST, syscall_test),
    (cos_sys::idx::IDX_MEMORY_MODIFY, syscall_test),
    (cos_sys::idx::IDX_PROCESS_CURRENT, syscall_test),
    (
        cos_sys::idx::IDX_PROCESS_CREATE,
        syscall_multitask::syscall_create_process,
    ),
    (cos_sys::idx::IDX_PROCESS_KILL, syscall_test),
    (
        cos_sys::idx::IDX_PROCESS_WAIT,
        syscall_multitask::syscall_wait_process,
    ),
    (
        cos_sys::idx::IDX_FILE_CREATE,
        syscall_file::syscall_file_create,
    ),
    (cos_sys::idx::IDX_FILE_OPEN, syscall_file::syscall_file_open),
    (cos_sys::idx::IDX_FILE_READ, syscall_file::syscall_file_read),
    (
        cos_sys::idx::IDX_FILE_WRITE,
        syscall_file::syscall_file_write,
    ),
    (
        cos_sys::idx::IDX_FILE_GET_POS,
        syscall_file::syscall_file_get_pos,
    ),
    (
        cos_sys::idx::IDX_FILE_SET_POS,
        syscall_file::syscall_file_set_pos,
    ),
    (
        cos_sys::idx::IDX_FILE_CLOSE,
        syscall_file::syscall_file_close,
    ),
    (cos_sys::idx::IDX_DEBUG_INFO, syscall_test),
    (cos_sys::idx::IDX_DEBUG_GET_CHAR, syscall_debug_get_char),
    (cos_sys::idx::IDX_DEBUG_PUT_CHAR, syscall_debug_put_char),
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
        if entry1.0 < entry2.0 {
            Ordering::Less
        } else if entry1.0 > entry2.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
};

syscall_handler! {
    fn syscall_test() {
        kprintln!("syscall test pass");
    }
}

syscall_handler! {
    fn syscall_debug_get_char(char_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(char_ptr as usize) {
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
        if !memory::page::is_user_space_virtual_memory(char_ptr as usize) {
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
