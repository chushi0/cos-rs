use core::arch::asm;

/// 关中断
///
/// 关中断后当前核心不会受到硬中断影响，代码执行路径不会被意外打断，其他核心不受影响。
///
/// 调用者需保证当前可以关中断
pub fn cli() {
    unsafe {
        asm!("cli", options(nostack, preserves_flags));
    }
}

/// 开中断
///
/// 调用者需保证当前可以安全开中断。
/// 在错误的上下文调用可能导致重入，造成数据损坏或死锁。
/// 当IDT未正确设置时，发生中断会触发double fault
pub fn sti() {
    unsafe {
        asm!("sti", options(nostack, preserves_flags));
    }
}

/// 判断中断是否开启
pub fn interrupts_enabled() -> bool {
    let rflags: u64;

    // 中断状态存储在RFLAGS里的IF位，但不能直接读取，我们通过压栈访问
    // Safety: 我们在push后紧接pop恢复栈，因此不会破坏栈平衡
    unsafe {
        asm!(
            "pushfq",
            "pop {}",
            out(reg) rflags,
            options(nomem, preserves_flags)
        );
    }

    (rflags & (1 << 9)) != 0
}

pub struct IrqGuard {
    prev: bool,
}

impl IrqGuard {
    /// 关中断，并在IrqGuard释放后恢复之前的中断状态
    pub fn cli() -> Self {
        let prev = interrupts_enabled();
        cli();
        IrqGuard { prev }
    }

    /// 开中断，并在IrqGuard释放后恢复之前的中断状态
    ///
    /// Safety: 调用者需保证当前可以安全开中断
    pub fn sti() -> Self {
        let prev = interrupts_enabled();
        sti();
        IrqGuard { prev }
    }
}

impl Drop for IrqGuard {
    fn drop(&mut self) {
        if self.prev {
            sti();
        } else {
            cli();
        }
    }
}
