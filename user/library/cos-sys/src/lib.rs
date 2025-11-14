//! COS的系统调用库
//!
//! 此crate封装了COS支持的全部系统调用，用户程序会依赖此库，陷入内核并使用系统功能。
//! 内核同样会依赖此库，复用此库中定义的系统调用编号。
//!
//! 对于应用程序而言，尽量避免直接使用 [syscall()] 函数，而是使用它们的封装版本。
//! 直接使用 [syscall()] 容易出错且会丧失可读性。

#![cfg(target_arch = "x86_64")]
#![no_std]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

use core::arch::asm;

pub mod error;
pub mod idx;
pub mod memory;
pub mod multitask;

/// 进行系统调用
///
/// 此函数使用syscall指令发起系统调用请求，CPU会将执行点移动至内核，并在内核执行结束后返回。
///
/// # ABI
///
/// syscall指令会占用rcx和r11两个寄存器，为内核提供返回点信息。
///
/// 根据内核规范，系统调用编号通过rax寄存器传递，最多支持6个参数，分别通过rdi、rsi、rdx、
/// r10、r8、r9传递，系统调用的返回值使用rax寄存器传递。
///
/// 除以上寄存器外，syscall不应当修改包括rsp在内的寄存器。
///
/// # 陷入内核
///
/// syscall指令会将参数传递给内核执行。尽管其表现为一条机器指令，但不应当将其理解为原子的。
/// 内核会使用多个指令周期处理请求，并可能在其中加入线程调度等内核功能。
///
/// 参数只能通过寄存器传递，因此无法直接传递复杂结构体。通常会选择通过传递指针来传递结构体。
///
/// # 系统调用编号
///
/// COS定义了许多系统调用编号，可以在 [idx] 中进行查看。
///
/// 不同的系统调用功能对参数有不同的定义，请仔细阅读各系统调用的文档说明。
///
/// # 返回值
///
/// 返回值通常表示错误，或者，对于简单的系统调用，也可直接返回值。
///
/// 请参照各系统调用的文档了解其信息。
///
/// # Panic
///
/// 系统调用不会触发通常意义上的 rust panic。但是，它可能会因某种情况导致内核直接杀死线程或进程。
///
/// # Safety
///
/// 如果违反了以下任意不变式，那么可能会触发未定义行为：
///
/// - 使用了未定义的系统编号
///
/// - 传递了无效的参数
///
/// - 对于传递结构体指针的系统调用，传递了无效或未对齐的指针，或者在内核访问期间引发了可变引用冲突
///
/// - 各系统调用文档中的其他未定义行为
///
/// # 对应用程序开发者
///
/// 系统调用API是不稳定的，建议应用程序开发者不要通过硬编码syscall方式使用系统功能，而是使用系统的动态链接库。
pub unsafe fn syscall(id: u64, p1: u64, p2: u64, p3: u64, p4: u64, p5: u64, p6: u64) -> u64 {
    let ret;

    // Safety: 见函数说明
    unsafe {
        asm!(
            "syscall",
            in("rax") id,
            in("rdi") p1,
            in("rsi") p2,
            in("rdx") p3,
            in("r10") p4,
            in("r8") p5,
            in("r9") p6,
            lateout("rax") ret,
            out("rcx") _,
            out("r11") _,
            options(nostack, preserves_flags)
        );
    }

    ret
}

/// 进行最多6个参数的系统调用
///
/// 此宏为 [syscall()] 函数的变长参数版本
#[macro_export]
macro_rules! syscall {
    ($id:expr) => {
        $crate::syscall!($id, 0)
    };
    ($id:expr, $p1:expr) => {
        $crate::syscall!($id, $p1, 0)
    };
    ($id:expr, $p1:expr, $p2:expr) => {
        $crate::syscall!($id, $p1, $p2, 0)
    };
    ($id:expr, $p1:expr, $p2:expr, $p3:expr) => {
        $crate::syscall!($id, $p1, $p2, $p3, 0)
    };
    ($id:expr, $p1:expr, $p2:expr, $p3:expr, $p4:expr) => {
        $crate::syscall!($id, $p1, $p2, $p3, $p4, 0)
    };
    ($id:expr, $p1:expr, $p2:expr, $p3:expr, $p4:expr, $p5:expr) => {
        $crate::syscall!($id, $p1, $p2, $p3, $p4, $p5, 0)
    };
    ($id:expr, $p1:expr, $p2:expr, $p3:expr, $p4:expr, $p5:expr, $p6:expr) => {
        $crate::syscall($id, $p1, $p2, $p3, $p4, $p5, $p6)
    };
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
