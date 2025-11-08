use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};

use crate::semaphore::{Semaphore, SemaphoreAcquireFuture, SemaphoreGuard};

/// 一个异步读写锁，允许多个并发的读访问或一个独占的写访问。
///
/// 与 [`std::sync::RwLock`] 不同，本类型是异步友好的，
/// `read()` / `write()` 返回的都是可 `.await` 的 [`Future`]。
/// 读锁并发，写锁独占，写锁需要等待所有读锁释放。
///
/// 内部通过计数型信号量实现：
/// - 每个读锁获取 `1` 个 permit
/// - 写锁需要获取 `usize::MAX` 个 permit（即独占）
///
/// # Cancel Safety
///
/// 等待锁的 `Future` 是 *安全可取消* 的。
/// 如果 `Future` 在等待过程中被 `drop`，将自动从等待队列中移除，
/// 并不会错误地持有锁或减少 permit。
///
/// - `drop(ReadLockFuture)` 或 `drop(WriteLockFuture)` → 不持有锁，需要重新排队
/// - `drop(ReadLockGuard)` 或 `drop(WriteLockGuard)` → 释放锁
///
/// # 写锁降级
///
/// 写锁持有者可以调用 [`WriteLockGuard::downgrade`] 获取读锁，
/// 无需释放并重新等待：
/// ```ignore
/// let write = lock.write().await;
/// let read = write.downgrade(); // 仍然持有访问权，但不再阻挡读者
/// ```
///
/// # 公平性
///
/// 本读写锁是 **无偏公平 (fair)** 的。
/// 所有锁请求（读与写）都会进入同一个 FIFO 队列按顺序获取许可，
/// 因此不会出现读者或写者长期饥饿的情况。
///
/// - 如果写者先到，则后续的读者必须等待写者完成
/// - 如果读者已在排队，写者也不能插队
///
/// 换句话说，本实现既不是读优先，也不是写优先，而是 **严格队列公平**。
///
/// # 示例
///
/// ```ignore
/// let lock = RwLock::new(0);
///
/// // 多读并发
/// let r1 = lock.read().await;
/// let r2 = lock.read().await;
/// assert_eq!(*r1, *r2);
///
/// // 写独占
/// let mut w = lock.write().await;
/// *w += 1;
/// ```
pub struct RwLock<T> {
    data: UnsafeCell<T>,
    semaphore: Semaphore,
}

pub struct ReadLockGuard<'a, T> {
    lock: &'a RwLock<T>,
    #[allow(dead_code)]
    semaphore_guard: SemaphoreGuard<'a>,
}

pub struct WriteLockGuard<'a, T> {
    lock: &'a RwLock<T>,
    semaphore_guard: SemaphoreGuard<'a>,
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ReadLockFuture<'a, T> {
    lock: &'a RwLock<T>,
    semaphore_future: SemaphoreAcquireFuture<'a>,
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct WriteLockFuture<'a, T> {
    lock: &'a RwLock<T>,
    semaphore_future: SemaphoreAcquireFuture<'a>,
}

unsafe impl<T: Send> Send for RwLock<T> {}
unsafe impl<T: Send> Sync for RwLock<T> {}

impl<T> RwLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            semaphore: Semaphore::new(usize::MAX),
        }
    }

    pub fn try_read(&self) -> Option<ReadLockGuard<'_, T>> {
        self.semaphore
            .try_acquire(1)
            .map(|semaphore_guard| ReadLockGuard {
                lock: self,
                semaphore_guard,
            })
    }

    pub fn try_write(&self) -> Option<WriteLockGuard<'_, T>> {
        self.semaphore
            .try_acquire(usize::MAX)
            .map(|semaphore_guard| WriteLockGuard {
                lock: self,
                semaphore_guard,
            })
    }

    pub fn read(&self) -> ReadLockFuture<'_, T> {
        ReadLockFuture {
            lock: self,
            semaphore_future: self.semaphore.acquire(1),
        }
    }

    pub fn write(&self) -> WriteLockFuture<'_, T> {
        WriteLockFuture {
            lock: self,
            semaphore_future: self.semaphore.acquire(usize::MAX),
        }
    }
}

impl<'a, T> WriteLockGuard<'a, T> {
    pub fn downgrade(self) -> ReadLockGuard<'a, T> {
        let mut semaphore_guard = self.semaphore_guard;
        semaphore_guard.release_part(usize::MAX - 1);

        ReadLockGuard {
            lock: self.lock,
            semaphore_guard: semaphore_guard,
        }
    }
}

impl<'a, T> Future for ReadLockFuture<'a, T> {
    type Output = ReadLockGuard<'a, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.semaphore_future)
            .poll(cx)
            .map(|semaphore_guard| ReadLockGuard {
                lock: self.lock,
                semaphore_guard,
            })
    }
}

impl<'a, T> Future for WriteLockFuture<'a, T> {
    type Output = WriteLockGuard<'a, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.semaphore_future)
            .poll(cx)
            .map(|semaphore_guard| WriteLockGuard {
                lock: self.lock,
                semaphore_guard,
            })
    }
}

impl<T> Deref for ReadLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> Deref for WriteLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for WriteLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}
