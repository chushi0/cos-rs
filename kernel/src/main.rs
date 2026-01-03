#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

extern crate alloc;
extern crate rlibc;

pub mod bootloader;
pub mod display;
pub mod io;
pub mod memory;
pub mod multitask;
pub mod panicking;
pub mod sync;
pub mod trap;
pub mod user;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmain(
    memory_region_ptr: *const bootloader::MemoryRegion,
    memory_region_len: usize,
    startup_disk: u32,
) -> ! {
    // 初始化VGA文本缓冲，并输出文本
    display::vga_text::init();
    // 初始化中断、异常处理和系统调用
    unsafe {
        trap::init();
    }
    // 初始化内存
    unsafe {
        memory::init(core::slice::from_raw_parts(
            memory_region_ptr,
            memory_region_len,
        ));
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
    panicking::panic_entry(info)
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
