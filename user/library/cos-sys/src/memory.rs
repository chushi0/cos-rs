use core::{mem::MaybeUninit, ptr::NonNull};

use crate::{
    error::{Result, SyscallError},
    idx, syscall,
};

/// 申请内存页
///
/// 如果可用空间充足，将分配连续可读写的内存页。内存页大小为4K。
/// 返回值为连续内存页的低地址。内核保证返回时内存页状态为可读写
///
/// 内核会避开0地址内存页。
pub fn alloc_page(count: u64) -> Result<NonNull<u8>> {
    let mut addr = MaybeUninit::<u64>::uninit();
    let addr_ptr = addr.as_mut_ptr() as u64;
    let error = unsafe { syscall!(idx::IDX_MEMORY_ALLOC, count, addr_ptr) };
    SyscallError::to_result(error)
        .map(|_| unsafe { NonNull::new_unchecked(addr.assume_init() as *mut u8) })
}

/// 释放内存页
///
/// 将指定内存页释放并归还系统。归还后的内存页仍有可能被 [alloc_page] 申请。
/// 系统将从ptr所在的内存页开始，回收count数量的内存页。这意味着程序可以先申请大空间，
/// 然后多次分批回收。
pub unsafe fn free_page(ptr: NonNull<u8>, count: u64) -> Result {
    let ptr = ptr.as_ptr() as u64;
    let error = unsafe { syscall!(idx::IDX_MEMORY_FREE, ptr, count) };
    SyscallError::to_result(error)
}
