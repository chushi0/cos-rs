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
pub mod user;

// 测试使用，临时关闭蓝屏
const BLUE_SCREEN: bool = true;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmain(
    memory_region_ptr: *const MemoryRegion,
    memory_region_len: usize,
    startup_disk: u32,
) -> ! {
    // 初始化VGA文本缓冲，并输出文本
    display::vga_text::init();
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

    // 初始化键盘
    unsafe {
        io::keyboard::init();
    }

    multitask::async_rt::spawn(async move {
        // 初始化磁盘
        if io::disk::init_disk(startup_disk as u8).await.is_err() {
            panic!("failed to init disk");
        }

        // 磁盘初始化完成后，加载第一个用户程序（/system/init）
        let Some(process) = multitask::process::create_user_process("/system/init").await else {
            panic!("start /system/init failed");
        };
        let mut process_subscriber = multitask::process::get_exit_code_subscriber(&process);
        drop(process);

        // /system/init 不应该结束
        loop {
            if process_subscriber.wait().await.is_err() {
                panic!(
                    "process /system/init die, exit code: {}",
                    process_subscriber.borrow()
                );
            }
        }
    });

    // 运行内核异步主任务
    multitask::async_rt::run()
}

#[panic_handler]
fn on_panic(info: &core::panic::PanicInfo) -> ! {
    use core::fmt::Write;

    // 关闭中断
    // TODO: 多核情况，需要通知其他核结束工作
    sync::int::cli();

    if BLUE_SCREEN {
        // 重新建立一个VGA TEXT BUFFER
        // 全局kprintln已不可信，需要使用新对象
        // 我们已经关中断，并且不会再次打开，不会有访问冲突
        let mut writer = unsafe { display::vga_text::VgaTextWriter::with_style(0x1f) };

        // 打印蓝屏消息
        // writeln不依赖堆，可以使用
        // 不要在这里进行任何堆分配！！
        _ = writeln!(
            writer,
            "A PROBLEM HAS BEEN DETECTED AND COS HAS BEEN SHUT DOWN TO PREVENT DAMAGE TO YOUR COMPUTER."
        );
        _ = writeln!(writer, "");
        _ = writeln!(
            writer,
            "The system encountered a fatal condition from which it cannot recover."
        );
        _ = writeln!(writer, "");

        _ = writeln!(writer, "Technical information:");
        _ = writeln!(writer, "");
        if let Some(location) = info.location() {
            _ = writeln!(writer, "*** FILE: {}", location.file());
            _ = writeln!(
                writer,
                "*** LINE: {} COLUMN: {}",
                location.line(),
                location.column()
            );
        }
        _ = writeln!(writer, "*** MESSAGE: {}", info.message());
        _ = writeln!(writer, "");
        _ = writeln!(
            writer,
            "If this is the first time you have seen this Stop error screen, restart your system. If this screen appears again, follow these steps:"
        );
        _ = writeln!(writer, "");
        _ = writeln!(
            writer,
            "* If problems continue, disable or remove any newly installed components."
        );
        _ = writeln!(
            writer,
            "* Contact your system administrator or kernel developer for assistance."
        );
        _ = writeln!(writer, "");
        _ = writeln!(writer, "The system has been halted.");
        _ = writeln!(writer, "");
        _ = writeln!(writer, "STOP: 0x0000007E (KERNEL_PANIC)");
        _ = writeln!(writer, "");
    }

    // TODO: 这里应该准备重启了

    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
