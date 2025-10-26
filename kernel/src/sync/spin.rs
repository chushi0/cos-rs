use core::{
    cell::UnsafeCell,
    hint::spin_loop,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

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
