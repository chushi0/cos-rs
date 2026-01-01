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

/// 内存页读写权限
///
/// 该结构体采用位掩码表示内存页的权限。
/// 错误的位掩码组合是未定义行为，因此不开放通过u64构造的方式。请直接使用
/// [PageAccessible::RO]、[PageAccessible::RW]、[PageAccessible::RX]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct PageAccessible(u64);

impl PageAccessible {
    /// 可读
    pub const READ: u64 = 0x1;
    /// 可写
    pub const WRITE: u64 = 0x2;
    /// 可执行
    pub const EXECUTE: u64 = 0x4;

    /// 只读
    pub const RO: Self = Self(Self::READ);
    /// 读写
    pub const RW: Self = Self(Self::READ | Self::WRITE);
    /// 读可执行
    pub const RX: Self = Self(Self::READ | Self::EXECUTE);

    /// 获取内部值表示
    pub const fn inner(self) -> u64 {
        self.0
    }

    /// 判断是否可读
    pub const fn readable(self) -> bool {
        (self.0 & Self::READ) != 0
    }

    /// 判断是否可写
    pub const fn writable(self) -> bool {
        (self.0 & Self::WRITE) != 0
    }

    /// 判断是否可执行
    pub const fn executable(self) -> bool {
        (self.0 & Self::EXECUTE) != 0
    }
}

/// [PageAccessible] 的编译期断言
const _: () = {
    assert!(PageAccessible::RO.readable());
    assert!(!PageAccessible::RO.writable());
    assert!(!PageAccessible::RO.executable());

    assert!(PageAccessible::RW.readable());
    assert!(PageAccessible::RW.writable());
    assert!(!PageAccessible::RW.executable());

    assert!(PageAccessible::RX.readable());
    assert!(!PageAccessible::RX.writable());
    assert!(PageAccessible::RX.executable());
};

/// 测试内存页
///
/// 检查指定地址所在的内存页是否可读、可写或可执行。
/// 即便传入进程无法访问的内存（未映射内存、内核内存、保留内存等），此函数也不会产生未定义行为，
/// 而是以不可读、不可写、不可执行的方式返回。
///
/// 测试内存页仅限于当前进程，程序无法访问其他进程和内核内存。
pub fn test_page(ptr: NonNull<u8>) -> Result<PageAccessible> {
    let ptr = ptr.as_ptr() as u64;
    let mut status = MaybeUninit::<u64>::uninit();
    let status_ptr = status.as_mut_ptr() as u64;
    let error = unsafe { syscall!(idx::IDX_MEMORY_TEST, ptr, status_ptr) };
    SyscallError::to_result(error).map(|_| unsafe { PageAccessible(status.assume_init()) })
}

/// 修改内存页读写权限
///
/// 修改内存页仅限于当前进程，程序无法访问其他进程和内核内存。
pub unsafe fn modify_page(ptr: NonNull<u8>, status: PageAccessible) -> Result {
    let ptr = ptr.as_ptr() as u64;
    let error = unsafe { syscall!(idx::IDX_MEMORY_TEST, ptr, status.0) };
    SyscallError::to_result(error)
}
