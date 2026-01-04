use core::cmp::Ordering;

mod debug;
mod file;
mod memory;
mod multitask;

pub type SyscallEntry = (u64, extern "C" fn(u64, u64, u64, u64, u64, u64) -> u64);

const SYSCALL_SUCCESS: u64 = cos_sys::error::ErrorKind::Success as u64;

pub const SYSCALL_HANDLER: &[SyscallEntry] = &[
    (cos_sys::idx::IDX_EXIT_PROCESS, multitask::exit_process),
    (cos_sys::idx::IDX_EXIT_THREAD, multitask::exit_thread),
    (cos_sys::idx::IDX_THREAD_CURRENT, multitask::current_thread),
    (cos_sys::idx::IDX_THREAD_WAIT, multitask::wait_thread),
    (cos_sys::idx::IDX_THREAD_WAKE, multitask::wake_thread),
    (cos_sys::idx::IDX_THREAD_KILL, multitask::kill_thread),
    (cos_sys::idx::IDX_THREAD_CREATE, debug::syscall_test),
    (cos_sys::idx::IDX_THREAD_JOIN, multitask::join_thread),
    (cos_sys::idx::IDX_MEMORY_ALLOC, memory::alloc_page),
    (cos_sys::idx::IDX_MEMORY_FREE, memory::free_page),
    (cos_sys::idx::IDX_PROCESS_CURRENT, multitask::current_process),
    (cos_sys::idx::IDX_PROCESS_CREATE, multitask::create_process),
    (cos_sys::idx::IDX_PROCESS_KILL, multitask::kill_process),
    (cos_sys::idx::IDX_PROCESS_WAIT, multitask::wait_process),
    (cos_sys::idx::IDX_FILE_CREATE, file::create),
    (cos_sys::idx::IDX_FILE_OPEN, file::open),
    (cos_sys::idx::IDX_FILE_READ, file::read),
    (cos_sys::idx::IDX_FILE_WRITE, file::write),
    (cos_sys::idx::IDX_FILE_GET_POS, file::get_pos),
    (cos_sys::idx::IDX_FILE_SET_POS, file::set_pos),
    (cos_sys::idx::IDX_FILE_CLOSE, file::close),
    (cos_sys::idx::IDX_DEBUG_INFO, debug::syscall_test),
    (cos_sys::idx::IDX_DEBUG_GET_CHAR, debug::get_char),
    (cos_sys::idx::IDX_DEBUG_PUT_CHAR, debug::put_char),
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

#[macro_export]
macro_rules! syscall_handler {
    (fn $name:ident() -> u64 { $($t:tt)* }) => {
        pub extern "C" fn $name(_p1: u64, _p2: u64, _p3: u64, _p4: u64, _p5: u64, _p6: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64) -> u64 { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, _p2: u64, _p3: u64, _p4: u64, _p5: u64, _p6: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64) -> u64 { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, _p3: u64, _p4: u64, _p5: u64, _p6: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64) -> u64 { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, _p4: u64, _p5: u64, _p6: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64) -> u64 { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, _p5: u64, _p6: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64, $p5:ident: u64) -> u64 { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, $p5: u64, _p6: u64) -> u64 { $($t)* }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64, $p5:ident: u64, $p6:ident: u64) -> u64 { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, $p5: u64, $p6: u64) -> u64 { $($t)* }
    };

    (fn $name:ident() { $($t:tt)* }) => {
        pub extern "C" fn $name(_p1: u64, _p2: u64, _p3: u64, _p4: u64, _p5: u64, _p6: u64) -> u64 { (|| { $($t)* })(); $crate::syscall::SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64) { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, _p2: u64, _p3: u64, _p4: u64, _p5: u64, _p6: u64) -> u64 { (|| { $($t)* })(); $crate::syscall::SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64) { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, _p3: u64, _p4: u64, _p5: u64, _p6: u64) -> u64 { (|| { $($t)* })(); $crate::syscall::SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64) { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, _p4: u64, _p5: u64, _p6: u64) -> u64 { (|| { $($t)* })(); $crate::syscall::SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64) { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, _p5: u64, _p6: u64) -> u64 { (|| { $($t)* })(); $crate::syscall::SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64, $p5:ident: u64) { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, $p5: u64, _p6: u64) -> u64 { (|| { $($t)* })(); $crate::syscall::SYSCALL_SUCCESS }
    };
    (fn $name:ident($p1:ident: u64, $p2:ident: u64, $p3:ident: u64, $p4:ident: u64, $p5:ident: u64, $p6:ident: u64) { $($t:tt)* }) => {
        pub extern "C" fn $name($p1: u64, $p2: u64, $p3: u64, $p4: u64, $p5: u64, $p6: u64) -> u64 { (|| { $($t)* })(); $crate::syscall::SYSCALL_SUCCESS }
    };
}
