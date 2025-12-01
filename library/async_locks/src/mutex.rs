use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};

use crate::semaphore::{Semaphore, SemaphoreAcquireFuture, SemaphoreGuard};

/// 一个基于异步信号量实现的互斥量。
///
/// `Mutex<T>` 允许多个任务以异步方式对内部数据进行独占访问。
/// 每次加锁会尝试获取一个信号量许可，因此：
/// - 互斥是由信号量保证的（内部固定为 `1` 个许可）；
/// - 加锁是 **有序且公平的**：等待的任务按排队顺序依次获得锁；
/// - 支持异步等待，不会阻塞线程；
/// - 加锁过程是 **取消安全的**：如果一个等待锁的 Future 被 `drop`，不会导致死锁或许可泄漏。
///
/// 与标准库的 `std::sync::Mutex` 的主要区别：
/// - 本实现为 **异步 Mutex**，适用于无阻塞运行时 / 裸机场景；
/// - 未提供锁毒化 (poisoning) 行为。
pub struct Mutex<T> {
    data: UnsafeCell<T>,
    semaphore: Semaphore,
}

/// 一个互斥锁的持有标记，提供对内部数据的独占访问。
///
/// 当该值被 `drop` 时，会自动释放锁（归还信号量许可），
/// 因此锁的释放是 **RAII** 的。
///
/// 持有期间提供 `&T` / `&mut T` 的访问。
pub struct MutexGuard<'a, T> {
    // pub(crate) for Condvar
    pub(crate) mutex: &'a Mutex<T>,
    #[allow(dead_code)]
    semaphore_guard: SemaphoreGuard<'a>,
}

/// 代表一次异步加锁操作。
///
/// 当锁被持有时，该 Future 会将当前任务加入等待队列并挂起，
/// 当锁可用时按排队顺序被唤醒。
///
/// ### 取消安全
/// 若本 Future 在等待期间被 `drop`，则会安全移出队列；
/// 若已被唤醒但尚未完成 `poll`，则会自动归还锁的许可。
///
/// 因此，本 Future 的取消不会导致死锁或许可泄漏。
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct MutexLockFuture<'a, T> {
    mutex: &'a Mutex<T>,
    semaphore_future: SemaphoreAcquireFuture<'a>,
}

unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// 创建一个新的互斥量，内部值为 `data`。
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            semaphore: Semaphore::new(1),
        }
    }

    /// 尝试立即获取锁。
    ///
    /// 如果锁当前已被持有，则返回 `None`，且不会进入等待队列。
    ///
    /// 该操作 **不会挂起**，因此本方法是 **无等待的、非阻塞** 调用。
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.semaphore
            .try_acquire(1)
            .map(|semaphore_guard| MutexGuard {
                mutex: self,
                semaphore_guard,
            })
    }

    /// 异步获取锁。
    ///
    /// 若锁当前已被持有，则返回的 [`MutexLockFuture`] 会将任务加入等待队列并挂起，
    /// 当锁可用时按排队顺序唤醒。
    ///
    /// ### 取消安全（Cancel Safety）
    /// 如果在等待过程中 Future 被 `drop`：
    /// - 它会安全移出等待队列；
    /// - 如果锁尚未分配给该任务，则不会影响锁状态；
    /// - 如果锁已经被分配但尚未完成 `poll`，则会归还该次锁的持有；
    ///
    /// 因此，本方法是 **完全取消安全的**：取消不会导致死锁或许可泄漏。
    pub fn lock(&self) -> MutexLockFuture<'_, T> {
        MutexLockFuture {
            mutex: self,
            semaphore_future: self.semaphore.acquire(1),
        }
    }
}

impl<'a, T> Future for MutexLockFuture<'a, T> {
    type Output = MutexGuard<'a, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.semaphore_future)
            .poll(cx)
            .map(|semaphore_guard| MutexGuard {
                mutex: self.mutex,
                semaphore_guard,
            })
    }
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}
