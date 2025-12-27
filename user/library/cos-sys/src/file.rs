use core::mem::MaybeUninit;

use crate::{
    error::{Result, SyscallError},
    idx::{
        IDX_FILE, IDX_SUB_FILE_CLOSE, IDX_SUB_FILE_CREATE, IDX_SUB_FILE_GET_POS, IDX_SUB_FILE_OPEN,
        IDX_SUB_FILE_READ, IDX_SUB_FILE_SET_POS, IDX_SUB_FILE_WRITE,
    },
    syscall,
};

pub fn create(path: &[u8]) -> Result<()> {
    let path_ptr = path.as_ptr() as u64;
    let path_len = path.len() as u64;
    let error = unsafe { syscall!(IDX_FILE, IDX_SUB_FILE_CREATE, path_ptr, path_len) };
    SyscallError::to_result(error)
}

pub fn open(path: &[u8]) -> Result<u64> {
    let path_ptr = path.as_ptr() as u64;
    let path_len = path.len() as u64;
    let mut handle = MaybeUninit::uninit();
    let handle_ptr = handle.as_mut_ptr() as u64;
    let error = unsafe { syscall!(IDX_FILE, IDX_SUB_FILE_OPEN, path_ptr, path_len, handle_ptr) };
    SyscallError::to_result(error).map(|_| unsafe { handle.assume_init() })
}

pub fn read(handle: u64, buffer: &mut [u8]) -> Result<u64> {
    let buffer_ptr = buffer.as_mut_ptr() as u64;
    let buffer_len = buffer.len() as u64;
    let mut read_count = MaybeUninit::uninit();
    let read_count_ptr = read_count.as_mut_ptr() as u64;
    let error = unsafe {
        syscall!(
            IDX_FILE,
            IDX_SUB_FILE_READ,
            handle,
            buffer_ptr,
            buffer_len,
            read_count_ptr
        )
    };
    SyscallError::to_result(error).map(|_| unsafe { read_count.assume_init() })
}

pub fn write(handle: u64, buffer: &[u8]) -> Result<u64> {
    let buffer_ptr = buffer.as_ptr() as u64;
    let buffer_len = buffer.len() as u64;
    let mut write_count = MaybeUninit::uninit();
    let write_count_ptr = write_count.as_mut_ptr() as u64;
    let error = unsafe {
        syscall!(
            IDX_FILE,
            IDX_SUB_FILE_WRITE,
            handle,
            buffer_ptr,
            buffer_len,
            write_count_ptr
        )
    };
    SyscallError::to_result(error).map(|_| unsafe { write_count.assume_init() })
}

pub fn get_pos(handle: u64) -> Result<u64> {
    let mut pos = MaybeUninit::uninit();
    let pos_ptr = pos.as_mut_ptr() as u64;
    let error = unsafe { syscall!(IDX_FILE, IDX_SUB_FILE_GET_POS, handle, pos_ptr) };
    SyscallError::to_result(error).map(|_| unsafe { pos.assume_init() })
}

pub fn set_pos(handle: u64, pos: u64) -> Result<()> {
    let error = unsafe { syscall!(IDX_FILE, IDX_SUB_FILE_SET_POS, handle, pos) };
    SyscallError::to_result(error)
}

pub fn close(handle: u64) -> Result<()> {
    let error = unsafe { syscall!(IDX_FILE, IDX_SUB_FILE_CLOSE, handle) };
    SyscallError::to_result(error)
}
