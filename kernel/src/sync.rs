use core::{
    arch::asm,
    cell::UnsafeCell,
    hint::spin_loop,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

/// 关中断
///
/// 关中断后当前核心不会受到硬中断影响，代码执行路径不会被意外打断，其他核心不受影响。
///
/// Safety: 调用者需保证当前可以关中断
pub unsafe fn cli() {
    unsafe {
        asm!("cli", options(nostack, preserves_flags));
    }
}

/// 开中断
///
/// Safety: 调用者需保证当前可以安全开中断。
/// 在错误的上下文调用可能导致重入，造成数据损坏或死锁。
/// 当IDT未正确设置时，发生中断会触发double fault
pub unsafe fn sti() {
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
    ///
    /// Safety: 调用者需保证当前可以安全关中断
    pub unsafe fn cli() -> Self {
        let prev = interrupts_enabled();

        if prev {
            unsafe {
                cli();
            }
        }

        IrqGuard { prev }
    }

    /// 开中断，并在IrqGuard释放后恢复之前的中断状态
    ///
    /// Safety: 调用者需保证当前可以安全开中断
    pub unsafe fn sti() -> Self {
        let prev = interrupts_enabled();

        if !prev {
            unsafe {
                sti();
            }
        }

        IrqGuard { prev }
    }
}

impl Drop for IrqGuard {
    fn drop(&mut self) {
        unsafe {
            if self.prev {
                sti();
            } else {
                cli();
            }
        }
    }
}

/// 自旋锁
///
/// 注意持有锁时触发中断可能导致死锁！
pub struct SpinLock<T> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for SpinLock<T> {}
unsafe impl<T: Send> Sync for SpinLock<T> {}

pub struct SpinLockGuard<'lock, T> {
    lock: &'lock SpinLock<T>,
}

impl<T> SpinLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        let locked = self
            .lock
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok();
        if locked {
            Some(SpinLockGuard::new(self))
        } else {
            None
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        loop {
            if let Some(guard) = self.try_lock() {
                return guard;
            }

            spin_loop();
        }
    }
}

impl<'lock, T> SpinLockGuard<'lock, T> {
    fn new(lock: &'lock SpinLock<T>) -> Self {
        Self { lock }
    }
}

impl<'lock, T> Deref for SpinLockGuard<'lock, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let target = self.lock.data.get();
        // Safety: SpinLock的原子变量已经保证只有一个访问者
        unsafe { &*target }
    }
}

impl<'lock, T> DerefMut for SpinLockGuard<'lock, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let target = self.lock.data.get();
        // Safety: SpinLock的原子变量已经保证只有一个访问者
        unsafe { &mut *target }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, Ordering::Release);
    }
}
