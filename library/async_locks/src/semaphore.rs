use core::{
    hint::spin_loop,
    pin::Pin,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{collections::vec_deque::VecDeque, sync::Arc};

use crate::SyncLock;

/// 异步公平计数信号量。
///
/// 信号量维护一个表示可用“许可”数量的计数，用于控制对共享资源的并发访问。
/// 获取许可会使计数减少，释放许可会使计数增加。
///
/// 本实现是 **异步** 且 **公平** 的：
/// - **异步**：当许可不足时，`acquire()` 不会阻塞线程，而是返回一个在轮询时可能产生
///   `Poll::Pending` 的 Future；当许可可用时会被唤醒。
/// - **公平**：等待任务按照进入等待队列的顺序依次被唤醒（FIFO）。
///
/// # 获取许可
///
/// 信号量提供两种获取许可的方式：
///
/// - [`Semaphore::try_acquire`] —— 立即尝试获取许可，若当前无许可则返回 `None`；
/// - [`Semaphore::acquire`] —— 返回一个 [`SemaphoreAcquireFuture`]，在 `poll` 时尝试获取；
///   若获取失败，会加入等待队列，在许可可用时由内部唤醒器唤醒。
///
/// 成功获取后会返回 [`SemaphoreGuard`]。当该 Guard 被丢弃（`drop`）时，
/// 会自动归还许可（符合 RAII 语义）。如果希望永久消耗许可，可使用 `core::mem::forget`
/// 忽略该 Guard，使许可不被归还。
///
/// 也可以直接调用 [`Semaphore::release`] 主动归还许可。
///
/// # 内存模型
///
/// - `acquire()` 使用 **Acquire** 语义，保证获取许可后能看到之前持有者对共享数据的写入；
/// - `release()` 使用 **Release** 语义，保证在归还许可之前的写入对之后获取许可的任务可见；
///
/// 因此，在典型的「先获取许可 → 访问共享数据 → 释放许可」模式中，
/// 不需要额外的内存栅栏或原子指令。
///
/// 若需要更强的顺序保证（如跨设备寄存器或需要屏蔽乱序执行的场合），
/// 请在临界区内部自行添加显式内存屏障。
///
/// ### 调度行为
///
/// `acquire` 和 `release` 是否会导致调度或上下文切换，取决于底层实现：
/// - 在具有操作系统或调度器的环境中，它们可能会阻塞或唤醒线程/任务；
/// - 在裸机环境中，它们可能通过自旋或关闭中断来等待。
///
/// 因此，**不要假设 `acquire` 或 `release` 是轻量操作**。
///
/// # 注意事项
///
/// - 公平性保证唤醒顺序为 FIFO，但不保证最终执行顺序（取决于调度器）。  
/// - 如果许可被永久忘记（例如通过 `mem::forget`），信号量的有效总容量会相应减少。  
pub struct Semaphore {
    permits: AtomicUsize,
    queue: SyncLock<(usize, VecDeque<Arc<SemaphoreWaker>>)>,
}

/// 表示已成功获取的信号量许可。
///
/// 当 `SemaphoreGuard` 被丢弃（`drop`）时，会自动将对应许可归还信号量（RAII）。  
/// 若希望永久消耗该许可，可使用 `core::mem::forget` 忽略该值，使其不被释放。
///
/// `SemaphoreGuard` 不可手动复制或克隆。
pub struct SemaphoreGuard<'a> {
    semaphore: &'a Semaphore,
    permits: usize,
}

/// 一次异步获取信号量许可的操作。
///
/// 在 `poll` 时：
/// - 若信号量中存在可用许可，则立即返回 `Poll::Ready(SemaphoreGuard)`；
/// - 否则将当前任务加入等待队列，并返回 `Poll::Pending`；当许可可用时由内部唤醒。
///
/// 该 Future 本身不持有许可，只有在 `Poll::Ready` 时返回的 [`SemaphoreGuard`]
/// 才表示许可已经成功获取。
///
/// ### 取消安全
///
/// [`SemaphoreAcquireFuture`] 是取消安全的：
///
/// - 当 Future 正在等待许可时（状态为 `WAITING`），被 `drop` 时会从等待队列中移除，
///   且不会消耗任何许可；
///
/// - 若 Future 在许可已分配但尚未完成 `poll`（状态从 `WAITING` 已转为 `ACQUIRED`）
///   即被 `drop`，则会自动将这次已分配的许可归还信号量，避免许可泄漏；
///
/// 因此，无论取消发生在等待阶段还是被唤醒之后，信号量的可用许可数量都能保持一致。
///
/// 被取消的 Future 会失去其排队位置；若之后重新调用 [`Semaphore::acquire`]，
/// 则会重新排队。
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct SemaphoreAcquireFuture<'a> {
    semaphore: &'a Semaphore,
    permits: usize,
    waker: Option<Arc<SemaphoreWaker>>,
}

// 信号量应当是Send且Sync的
const _: () = {
    assert_send_sync::<Semaphore>();
    const fn assert_send_sync<T: Send + Sync>() {}
};

struct SemaphoreWaker {
    waker: Waker,
    status: AtomicU32,
    permits: usize,
}

impl SemaphoreWaker {
    const STATUS_WAITING: u32 = 0;
    const STATUS_ACQUIRED: u32 = 1;
    const STATUS_GIVEUP: u32 = 2;
}

impl Semaphore {
    /// 创建一个信号量，并初始化为 `permits` 个可用许可。
    ///
    /// `permits` 表示初始可用的资源数量。获取许可会减少该值，释放则会增加。
    pub const fn new(permits: usize) -> Self {
        Self {
            permits: AtomicUsize::new(permits),
            queue: SyncLock::new((0, VecDeque::new())),
        }
    }

    /// 尝试获取 `n` 个许可。
    ///
    /// 若当前可用许可数量不足，则立即返回 `None`，不会阻塞或挂起调用方。
    /// 若获取成功，则返回一个 [`SemaphoreGuard`]，其持有 `n` 个许可，
    /// 并在被丢弃时自动归还。
    ///
    /// ### 内存语义
    /// 成功获取许可具备 **Acquire** 语义。
    ///
    /// # 返回值
    /// - `Some(guard)`：成功获取到 `n` 个许可。
    /// - `None`：当前许可不足，未获取任何许可。
    pub fn try_acquire(&self, n: usize) -> Option<SemaphoreGuard<'_>> {
        loop {
            let permits = self.permits.load(Ordering::Acquire);
            if permits < n {
                break None;
            }
            if self
                .permits
                .compare_exchange(permits, permits - n, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                break Some(SemaphoreGuard {
                    semaphore: self,
                    permits: n,
                });
            }

            spin_loop();
        }
    }

    /// 获取 `n` 个许可，必要时会等待。
    ///
    /// 如果当前可用许可不足，此方法会返回一个 [`SemaphoreAcquireFuture`]，
    /// 在 `await` 期间挂起任务，直到许可可用。
    ///
    /// 成功完成 `await` 后，调用方已经获得 `n` 个许可，并会得到一个 [`SemaphoreGuard`]，
    /// 在其被丢弃时自动归还许可。
    ///
    /// ### 内存语义
    /// 成功获取许可具备 **Acquire** 语义。
    ///
    /// ### 调度与等待行为
    /// - 此方法不会阻塞线程本身，而是通过 `Future` 协作式挂起任务（若运行时支持）。
    /// - 在裸机或无法挂起任务的环境中，本方法可能退化为自旋等待。
    pub fn acquire(&self, n: usize) -> SemaphoreAcquireFuture<'_> {
        SemaphoreAcquireFuture {
            semaphore: self,
            permits: n,
            waker: None,
        }
    }

    /// 归还 `n` 个许可。
    ///
    /// 这会增加信号量中的可用许可数量，并可能唤醒一个或多个正在等待许可的任务/线程。
    ///
    /// ### 内存语义
    /// 具有 **Release** 语义。  
    /// 调用方在 `release` 之前对共享状态的修改，对随后被唤醒的任务是可见的。
    ///
    /// ### 调度行为
    /// 若存在等待者，本方法可能触发唤醒操作（具体取决于底层实现）。
    pub fn release(&self, n: usize) {
        let mut queue = self.queue.lock();
        queue.0 += n;
        while let Some(waker) = queue.1.pop_front() {
            let status = waker.status.load(Ordering::Acquire);
            if status == SemaphoreWaker::STATUS_GIVEUP {
                continue;
            }
            if queue.0 < waker.permits {
                queue.1.push_front(waker);
                return;
            }
            let acquired = waker.status.compare_exchange(
                SemaphoreWaker::STATUS_WAITING,
                SemaphoreWaker::STATUS_ACQUIRED,
                Ordering::Release,
                Ordering::Relaxed,
            );
            if acquired.is_ok() {
                queue.0 -= waker.permits;
                waker.waker.wake_by_ref();
            } else {
                queue.1.push_front(waker);
                return;
            }
        }
        if queue.1.is_empty() {
            self.permits.fetch_add(queue.0, Ordering::Release);
            queue.0 = 0;
        }
    }

    fn queue(&self, waker: Arc<SemaphoreWaker>) {
        let mut lock = self.queue.lock();
        lock.0 += self.permits.swap(0, Ordering::AcqRel);
        lock.1.push_back(waker);
    }
}

impl SemaphoreGuard<'_> {
    pub(crate) fn release_part(&mut self, n: usize) {
        assert!(self.permits >= n);
        self.semaphore.release(n);
        self.permits -= n;
    }
}

impl Drop for SemaphoreGuard<'_> {
    fn drop(&mut self) {
        self.semaphore.release(self.permits);
    }
}

impl<'a> Future for SemaphoreAcquireFuture<'a> {
    type Output = SemaphoreGuard<'a>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &self.waker {
            Some(waker) => {
                if waker.status.load(Ordering::Acquire) == SemaphoreWaker::STATUS_ACQUIRED {
                    Poll::Ready(SemaphoreGuard {
                        semaphore: self.semaphore,
                        permits: self.permits,
                    })
                } else {
                    Poll::Pending
                }
            }
            None => {
                if let Some(acquired) = self.semaphore.try_acquire(self.permits) {
                    return Poll::Ready(acquired);
                }

                let waker = Arc::new(SemaphoreWaker {
                    waker: cx.waker().clone(),
                    status: AtomicU32::new(SemaphoreWaker::STATUS_WAITING),
                    permits: self.permits,
                });

                self.semaphore.queue(waker.clone());

                self.waker = Some(waker);

                // 立即触发一次队列检查
                self.semaphore.release(0);

                Poll::Pending
            }
        }
    }
}

impl Drop for SemaphoreAcquireFuture<'_> {
    fn drop(&mut self) {
        if let Some(waker) = &self.waker {
            let status = waker
                .status
                .swap(SemaphoreWaker::STATUS_GIVEUP, Ordering::AcqRel);
            if status == SemaphoreWaker::STATUS_ACQUIRED {
                self.semaphore.release(self.permits);
            } else {
                self.semaphore.release(0);
            }
        }
    }
}
