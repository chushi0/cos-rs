use core::{
    arch::asm,
    num::NonZeroU16,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{
    int::{StackFrame, IDT},
    interrupt_handler, kprintln, multitask,
};

// 定时器 PIT Channel 0
const IRQ_TIMER: u8 = 0;
pub const INDEX_TIMER: usize = IDT::INDEX_USER_DEFINED + 0;
// 键盘 PS/2 Keyboard
const IRQ_KEYBOARD: u8 = 1;
pub const INDEX_KEYBOARD: usize = IDT::INDEX_USER_DEFINED + 1;
// 连接从片PIC
const IRQ_CASCADE: u8 = 2;
pub const INDEX_CASCADE: usize = IDT::INDEX_USER_DEFINED + 2;
// 串口2
const IRQ_COM2: u8 = 3;
pub const INDEX_COM2: usize = IDT::INDEX_USER_DEFINED + 3;
// 串口1
const IRQ_COM1: u8 = 4;
pub const INDEX_COM1: usize = IDT::INDEX_USER_DEFINED + 4;
// LPT2或PS/2鼠标
const IRQ_LPT2: u8 = 5;
pub const INDEX_LPT2: usize = IDT::INDEX_USER_DEFINED + 5;
// 硬盘控制器
const IRQ_DISK: u8 = 6;
pub const INDEX_DISK: usize = IDT::INDEX_USER_DEFINED + 6;
// LPT1
const IRQ_LPT1: u8 = 7;
pub const INDEX_LPT1: usize = IDT::INDEX_USER_DEFINED + 7;
// 实时时钟 RTC
const IRQ_RTC: u8 = 8;
pub const INDEX_RTC: usize = IDT::INDEX_USER_DEFINED + 8;
// ACPI或可编程用途
const IRQ_ACPI: u8 = 9;
pub const INDEX_ACPI: usize = IDT::INDEX_USER_DEFINED + 9;
// 可编程用途
const IRQ_PCI1: u8 = 10;
pub const INDEX_PCI1: usize = IDT::INDEX_USER_DEFINED + 10;
// 可编程用途
const IRQ_PCI2: u8 = 11;
pub const INDEX_PCI2: usize = IDT::INDEX_USER_DEFINED + 11;
// PS/2 鼠标
const IRQ_MOUSE: u8 = 12;
pub const INDEX_MOUSE: usize = IDT::INDEX_USER_DEFINED + 12;
// 协处理器 FPU/Coprocessor
const IRQ_FPU: u8 = 13;
pub const INDEX_FPU: usize = IDT::INDEX_USER_DEFINED + 13;
// 主IDE通道
const IRQ_IDE1: u8 = 14;
pub const INDEX_IDE1: usize = IDT::INDEX_USER_DEFINED + 14;
// 主IDE通道
const IRQ_IDE2: u8 = 15;
pub const INDEX_IDE2: usize = IDT::INDEX_USER_DEFINED + 15;

const PIC1_COMMAND: u32 = 0x20;
const PIC2_COMMAND: u32 = 0xA0;
const PIC1_DATA: u32 = 0x21;
const PIC2_DATA: u32 = 0xA1;

/// 初始化硬中断（PIC芯片）
///
/// Safety: 仅在第一次调用时，该函数是安全的。不能并发
pub unsafe fn init() {
    // 初始化控制字（ICW）
    unsafe {
        asm!(
            "out dx, al",
            in("dx") PIC1_COMMAND,
            in("al") 0x11 as i8,
            options(nostack, preserves_flags)
        );
        asm!(
            "out dx, al",
            in("dx") PIC2_COMMAND,
            in("al") 0x11 as i8,
            options(nostack, preserves_flags)
        );
    }

    // 向量偏移
    // Master PIC IRQ0~7 映射到 IDT[32..39]
    // Slave PIC IRQ8~15 映射到 IDT[40..47]
    unsafe {
        asm!(
            "out dx, al",
            in("dx") PIC1_DATA,
            in("al") 32 as i8,
            options(nostack, preserves_flags)
        );
        asm!(
            "out dx, al",
            in("dx") PIC2_DATA,
            in("al") 40 as i8,
            options(nostack, preserves_flags)
        );
    }

    // 主从连接关系
    // 主片：告诉主片哪一位连接从片，0x04 -> IRQ2接从片
    // 从片：告诉从片它是主片的哪条IRQ， 0x02 -> 从片连接到主片IRQ2
    // 主片的IRQ2连接从片是硬件规范，可直接使用
    unsafe {
        asm!(
            "out dx, al",
            in("dx") PIC1_DATA,
            in("al") 0x04 as i8,
            options(nostack, preserves_flags)
        );
        asm!(
            "out dx, al",
            in("dx") PIC2_DATA,
            in("al") 0x02 as i8,
            options(nostack, preserves_flags)
        );
    }

    // 额外控制字，设置8086/88模式
    // 0x01 8086模式
    // 0x03 8086+自动EOI
    unsafe {
        asm!(
            "out dx, al",
            in("dx") PIC1_DATA,
            in("al") 0x01 as i8,
            options(nostack, preserves_flags)
        );
        asm!(
            "out dx, al",
            in("dx") PIC2_DATA,
            in("al") 0x01 as i8,
            options(nostack, preserves_flags)
        );
    }

    // 打开中断
    // 先暂时只开时钟中断和键盘中断，等后续再开全部中断
    unsafe {
        asm!(
            "out dx, al",
            in("dx") PIC1_DATA,
            in("al") 0b11111100 as u8,
            options(nostack, preserves_flags)
        );
        asm!(
            "out dx, al",
            in("dx") PIC2_DATA,
            in("al") 0b11111111 as u8,
            options(nostack, preserves_flags)
        );
    }
}

// 发送EOI（End of Interrupt）
unsafe fn send_eoi(irq: u8) {
    // 如果对应从片，则额外向从片发送
    if irq >= 8 {
        unsafe {
            asm!(
                "out dx, al",
                in("dx") PIC2_COMMAND,
                in("al") 0x20 as i8,
                options(nostack, preserves_flags)
            );
        }
    }
    // 总是需要向主片发送
    unsafe {
        asm!(
            "out dx, al",
            in("dx") PIC1_COMMAND,
            in("al") 0x20 as i8,
            options(nostack, preserves_flags)
        );
    }
}

// 硬件计时器的频率
const TIMER_FREQUENCY: u32 = 1193182;

/// 设置硬件计时器的中断频率
///
/// 中断频率将设置为 [`TIMER_FREQUENCY`]/n，即每 n/[`TIMER_FREQUENCY`] 秒触发一次中断
fn set_timer_interval(n: NonZeroU16) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") 0x43,
            in("al") 0x36 as i8,
            options(nostack, preserves_flags)
        );
        asm!(
            "out dx, al",
            in("dx") 0x40,
            in("al") (n.get() & 0xff) as u8,
            options(nostack, preserves_flags)
        );
        asm!(
            "out dx, al",
            in("dx") 0x40,
            in("al") ((n.get() >> 8) & 0xff) as u8,
            options(nostack, preserves_flags)
        );
    }
}

interrupt_handler! {
    fn timer_irq(stack: &mut StackFrame) {
        const ELAPSED: u64 = 1_000_000 * 65535 / TIMER_FREQUENCY as u64;

        multitask::async_task::tick(ELAPSED);

        unsafe {
            send_eoi(IRQ_TIMER);
        }
    }
}

interrupt_handler! {
    fn keyboard_irq(stack: &mut StackFrame) {
        // 获取键盘扫描码
        let scan_code: u8;
        unsafe {
            asm!(
                "in al, 0x60",
                out("al") scan_code,
                options(nostack, preserves_flags)
            );
        }

        kprintln!("keyboard: 0x{scan_code:x}");

        unsafe {
            send_eoi(IRQ_KEYBOARD);
        }
    }
}
