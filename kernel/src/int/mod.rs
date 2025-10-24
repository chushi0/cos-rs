#[allow(unused)]
mod hard;
mod idt;
mod soft;
pub mod tss;

pub unsafe fn init() {
    unsafe {
        // 硬中断初始化（初始化PIC芯片）
        hard::init();
        // 任务段初始化
        tss::init();
        // 中断描述符表初始化
        idt::init();
    }
}
