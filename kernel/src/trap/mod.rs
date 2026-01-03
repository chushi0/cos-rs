#[allow(unused)]
mod hard;
pub mod idt;
mod soft;
mod syscall;
pub mod tss;

pub unsafe fn init() {
    unsafe {
        // 硬中断初始化（初始化PIC芯片）
        hard::init();
        // 任务段初始化
        tss::init();
        // 中断描述符表初始化
        idt::init();
        // 系统调用初始化
        syscall::init();
    }
}
