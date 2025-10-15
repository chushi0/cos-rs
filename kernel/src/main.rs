#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

extern crate rlibc;

use core::{arch::asm, slice};

use crate::bootloader::MemoryRegion;

pub mod bootloader;
pub mod display;
pub mod int;
pub mod memory;
pub mod sync;

#[unsafe(no_mangle)]
pub extern "C" fn kmain(
    memory_region_ptr: *const MemoryRegion,
    memory_region_len: usize,
    _startup_disk: u32,
) -> ! {
    // 初始化VGA文本缓冲，并输出文本
    display::vga_text::init();
    kprintln!("Hello, kernel!");
    // 初始化中断
    unsafe {
        int::init();
    }
    // 初始化内存
    unsafe {
        memory::physics::init(slice::from_raw_parts(memory_region_ptr, memory_region_len));
    }

    kprintln!("CPU hlt");
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
