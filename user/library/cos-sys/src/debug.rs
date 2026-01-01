use core::mem::MaybeUninit;

use crate::{
    error::{Result, SyscallError},
    idx, syscall,
};

pub fn info() {
    unsafe {
        syscall!(idx::IDX_DEBUG_INFO);
    }
}

pub fn get_char() -> Result<u8> {
    let mut char = MaybeUninit::uninit();
    let char_ptr = char.as_mut_ptr() as u64;
    let error = unsafe { syscall!(idx::IDX_DEBUG_GET_CHAR, char_ptr) };
    SyscallError::to_result(error).map(|_| unsafe { char.assume_init() })
}

pub fn put_char(char: u8) -> Result<()> {
    let char_ptr = &raw const char as u64;
    let error = unsafe { syscall!(idx::IDX_DEBUG_PUT_CHAR, char_ptr) };
    SyscallError::to_result(error)
}
