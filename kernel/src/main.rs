#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

extern crate rlibc;

use core::arch::asm;

pub mod display;
pub mod int;
pub mod sync;

#[unsafe(no_mangle)]
pub extern "C" fn kmain() -> ! {
    // 初始化VGA文本缓冲，并输出文本
    display::vga_text::init();
    kprintln!("Hello, kernel!");
    // 初始化中断
    unsafe {
        int::init();
    }

    // 触发breakpoint
    unsafe {
        asm!("int 3");
    }

    // int3返回后自动执行下一条指令
    kprintln!("return from int3");

    // 触发 page fault
    // 这个不会返回，因为page fault中断返回后，会重新执行失败的指令
    // 我们的中断处理函数没有将这个内存恢复为可访问，因此这个指令不会成功
    // 我们仅在中断处理函数中loop_hlt以临时规避问题
    unsafe {
        let a = 0xdeadbeef as *mut u32;
        *a = 0;
    }

    loop_hlt();
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop_hlt();
}

fn loop_hlt() -> ! {
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
