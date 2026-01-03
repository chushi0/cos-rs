use core::{
    arch::asm,
    fmt::Write,
    panic::PanicInfo,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{display, sync};

static PANIC_COUNT: AtomicU32 = AtomicU32::new(0);

pub fn panic_entry(info: &PanicInfo) -> ! {
    // 关闭中断
    // TODO: 多核情况，需要通知其他核结束工作
    sync::int::cli();

    // panic 次数
    let panic_count = PANIC_COUNT.fetch_add(1, Ordering::SeqCst);
    match panic_count {
        // 正常panic，自动dump信息并展示蓝屏
        0 => auto_dump_and_print_blue_screen(info),
        // 双重panic，在dump信息时再次触发故障，仅展示静态蓝屏信息
        1 => print_static_blue_screen(),
        // 三重panic，说明展示蓝屏也是不安全的，立即复位
        2.. => restart_emergency(),
    }
}

fn auto_dump_and_print_blue_screen(info: &PanicInfo) -> ! {
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

    loop_hlt();
}

fn print_static_blue_screen() -> ! {
    let mut writer = unsafe { display::vga_text::VgaTextWriter::with_style(0x1f) };

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
    _ = writeln!(writer, "*** MESSAGE: DOUBLE PANIC DETECTED");
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
    _ = writeln!(writer, "STOP: 0x0000007F (KERNEL_DOUBLE_PANIC)");
    _ = writeln!(writer, "");

    loop_hlt()
}

fn restart_emergency() -> ! {
    // 尝试通过键盘控制器触发CPU Reset
    const RESET_CMD: u8 = 0xFE;
    const PORT: u16 = 0x64;
    for _ in 0..5 {
        unsafe {
            asm!(
                "out dx, al",
                in("dx") PORT,
                in("al") RESET_CMD,
            );
        }
    }

    // 如果尝试失败，进入hlt循环
    loop_hlt()
}

fn loop_hlt() -> ! {
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}
