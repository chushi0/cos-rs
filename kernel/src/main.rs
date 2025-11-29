#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

extern crate alloc;
extern crate rlibc;

use core::{arch::asm, slice};

use crate::bootloader::MemoryRegion;

pub mod bootloader;
pub mod display;
pub mod int;
pub mod io;
pub mod memory;
pub mod multitask;
pub mod sync;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmain(
    memory_region_ptr: *const MemoryRegion,
    memory_region_len: usize,
    startup_disk: u32,
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
    // 初始化per-cpu结构
    unsafe {
        sync::percpu::init();
    }

    // 初始化内核线程
    multitask::thread::create_kernel_async_thread();
    // 初始化IDLE线程
    multitask::thread::create_idle_thread();

    // 初始化磁盘
    multitask::async_rt::spawn(async move {
        if io::disk::init_disk(startup_disk as u8).await.is_err() {
            kprintln!("failed to init disk");
        }

        // 磁盘初始化完成后，加载第一个用户程序（/system/init）
        if multitask::process::create_user_process("/system/init")
            .await
            .is_none()
        {
            kprintln!("load /system/init failed");
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
