#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

extern crate alloc;
extern crate rlibc;

use core::{arch::asm, slice, time::Duration};

use alloc::boxed::Box;

use crate::{bootloader::MemoryRegion, sync::sti};

pub mod bootloader;
pub mod display;
pub mod int;
pub mod memory;
pub mod multitask;
pub mod sync;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmain(
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

    // 初始化内核线程
    multitask::thread::create_kernel_thread();
    // 初始化IDLE线程
    multitask::thread::create_idle_thread();
    // 模拟阻塞线程，若无抢占调度，则一旦进入此线程，则无法再执行其他线程
    unsafe {
        extern "C" fn busy_loop() -> ! {
            unsafe {
                sti();
            }
            loop {}
        }
        multitask::thread::create_thread(
            None,
            busy_loop as usize as u64,
            Box::leak(Box::new([0u8; 4096])) as *const u8 as usize as u64 + 4096 - 8,
            false,
        );
    }

    // 创建一个任务进行测试
    multitask::async_rt::spawn(async {
        loop {
            multitask::async_task::sleep(Duration::from_secs(1)).await;
            kprintln!("time elapsed A");
        }
    });
    multitask::async_rt::spawn(async {
        loop {
            multitask::async_task::sleep(Duration::from_secs(3)).await;
            kprintln!("time elapsed B");
        }
    });

    // 运行内核异步主任务
    multitask::async_rt::run()
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
