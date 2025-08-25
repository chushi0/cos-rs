#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

extern crate rlibc;

use core::arch::asm;

pub mod display;
pub mod sync;

#[unsafe(no_mangle)]
pub extern "C" fn kmain() -> ! {
    // 初始化VGA文本缓冲，并输出文本
    display::vga_text::init();
    kprintln!("Hello, kernel!");

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
