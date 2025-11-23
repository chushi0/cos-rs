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
use filesystem::path::PathBuf;

use crate::{
    bootloader::MemoryRegion,
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

    // 初始化磁盘
    multitask::async_rt::spawn(async move {
        if io::disk::init_disk(startup_disk as u8).await.is_err() {
            kprintln!("failed to init disk");
        }
        kprintln!("init disk done");
    });

    // 测试/模拟代码
    kernel_test_main();

    // 运行内核异步主任务
    multitask::async_rt::run()
}

fn kernel_test_main() {
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
            if multitask::process::write_user_process_memory(
                process_id,
                code_page.get(),
                code.as_ptr(),
                code.len(),
            )
            .is_err()
            {
                kprintln!("write 0xcc code failed");
                return;
            }

            // 写入启动地址
            if multitask::process::write_user_process_memory_struct(
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
    // 磁盘测试
    multitask::async_rt::spawn(async {
        // 等待1秒，假设磁盘会在这1秒内完成初始化
        multitask::async_task::sleep(Duration::from_secs(1)).await;
        // 获取文件系统对象
        let fs = {
            let _guard = unsafe { IrqGuard::cli() };
            io::disk::FILE_SYSTEMS.lock().get(&0).cloned()
        };
        let Some(fs) = fs else {
            kprintln!("filesystem is not mounted");
            return;
        };
        let Ok(path) = PathBuf::from_str("test.txt") else {
            kprintln!("failed to parse path");
            return;
        };
        // 首先尝试打开文件
        match fs.open_file(path.as_path()).await {
            Ok(mut file) => {
                kprintln!("open file success");
                let mut buf = [0u8; 20];
                if let Err(e) = file.read(&mut buf).await {
                    kprintln!("read file error: {e:?}");
                } else {
                    kprintln!("read file success: {buf:?}");
                }
                if let Err(e) = file.close().await {
                    kprintln!("failed to close file: {e:?}");
                }
                return;
            }
            Err(error) => {
                kprintln!("open file error: {error:?}");
                if let Err(e) = fs.create_file(path.as_path()).await {
                    kprintln!("create file error: {e:?}");
                    return;
                }
                kprintln!("create file success");
            }
        }
        // 尝试写文件
        let mut file = match fs.open_file(path.as_path()).await {
            Ok(file) => file,
            Err(error) => {
                kprintln!("open file error: {error:?}");
                return;
            }
        };
        kprintln!("opened file to write");
        const CONTENT: &[u8] = b"hello world";
        if let Err(e) = file.write(CONTENT).await {
            kprintln!("failed to write content: {e:?}");
        }
        if let Err(e) = file.close().await {
            kprintln!("failed to close file: {e:?}");
        }
        kprintln!("disk test done");
    });
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
