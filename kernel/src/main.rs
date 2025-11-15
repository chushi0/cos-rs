#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

extern crate alloc;
extern crate rlibc;

use core::{
    arch::{asm, naked_asm},
    num::NonZeroU64,
    slice,
    time::Duration,
};

use alloc::boxed::Box;
use filesystem::device::BlockDevice;

use crate::{
    bootloader::MemoryRegion,
    io::disk::ata_lba::AtaLbaDriver,
    multitask::process::ProcessPageType,
    sync::int::{IrqGuard, sti},
};

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
            Box::leak(Box::new([0u8; 4096])) as *mut u8 as usize as u64 + 4096 - 8,
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
    multitask::async_rt::spawn(async {
        // 模拟一个用户线程
        let Some(process) = multitask::process::create_process() else {
            kprintln!("create process failed");
            return;
        };
        let process_id = {
            let _guard = unsafe { IrqGuard::cli() };
            process.lock().process_id
        };

        // 创建页表
        let Some(code_page) =
            multitask::process::create_process_page(process_id, 0x1000, ProcessPageType::Code)
        else {
            kprintln!("create process code page failed");
            return;
        };
        kprintln!("code page: {code_page:x}");

        let Some(stack_page) =
            multitask::process::create_process_page(process_id, 0x1000, ProcessPageType::Stack)
        else {
            kprintln!("create process stack page failed");
            return;
        };
        kprintln!("stack page: {stack_page:x}");

        unsafe {
            // 写入代码
            // 0x90 nop
            // 0xcc int 3
            // 0x0f 0x05 syscall
            // 0xf4 hlt 特权指令，会立即触发#GP
            // 0xeb 0xfe jmp $-2 会卡在这里
            let code: [u8; _] = [0x90, 0xcc, 0x0f, 0x05, /*0xf4,*/ 0xeb, 0xfe];
            if multitask::process::write_user_process_memory(process_id, code_page.get(), &code)
                .is_err()
            {
                kprintln!("write 0xcc code failed");
                return;
            }

            // 写入启动地址
            if multitask::process::write_user_process_memory(
                process_id,
                stack_page.get() + 0x1000 - 8,
                &code_page,
            )
            .is_err()
            {
                kprintln!("write &code_page stack failed");
                return;
            }

            // 创建线程
            multitask::thread::create_thread(
                NonZeroU64::new(process_id),
                user_thread_entry as u64,
                stack_page.get() + 0x1000 - 8,
                false,
            );
        }

        #[unsafe(naked)]
        extern "C" fn user_thread_entry() -> ! {
            extern "C" fn enter_user_mode(rip: u64, rsp: u64) -> ! {
                unsafe { int::idt::enter_user_mode(rip, rsp) }
            }
            naked_asm!(
                "mov rdi, [rsp]",
                "mov rsi, rsp",
                "jmp {enter_user_mode}",
                enter_user_mode = sym enter_user_mode,
            )
        }
    });

    multitask::async_rt::spawn(async move {
        kprintln!("startup disk: {startup_disk}");

        // 读盘
        let Ok(driver) = AtaLbaDriver::new(startup_disk as u8).await else {
            kprintln!("failed to new ata lba driver");
            return;
        };

        let mut buf = [0u8; 512];
        if let Err(e) = driver.read_block(0, &mut buf).await {
            kprintln!("failed to read block: {e:?}");
        }
        kprintln!("read buf: {buf:x?}");
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
