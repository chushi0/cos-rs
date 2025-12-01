use core::{
    pin::Pin,
    sync::atomic::{AtomicU32, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{collections::vec_deque::VecDeque, sync::Arc};

use crate::{
    SyncLock,
    mutex::{Mutex, MutexGuard, MutexLockFuture},
};

/// 异步条件变量
///
/// [Condvar] 必须与 [Mutex<T>] 配套使用。当持有[MutexGuard]时，调用[Condvar::wait]函数可以**释放锁**并**让出**。
/// 直到调用[Condvar::wake]或[Condvar::wake_all]之后，任务才会**继续执行**并**重新获取锁**。让出期间，其他任务可以获取锁，并操作临界区资源。
///
/// 异步队列是公平的，FIFO。
///
/// # 取消安全
///
/// 如果一个已经被唤醒的 [CondvarWait]，无论其是否成功获取锁，都会尝试从队列中再次唤醒一个。
pub struct Condvar {
    queue: SyncLock<VecDeque<Arc<CondvarWaker>>>,
}

pub struct CondvarWait<'cond, 'lock, T>(CondvarWaitInner<'cond, 'lock, T>);

impl Condvar {
    pub fn new() -> Self {
        Self {
            queue: SyncLock::new(VecDeque::new()),
        }
    }

    /// 释放锁并让出。在[Self::wake]或[Self::wake_all]被调用后，重新获取锁
    pub fn wait<'cond, 'lock, T>(
        &'cond self,
        guard: MutexGuard<'lock, T>,
    ) -> CondvarWait<'cond, 'lock, T> {
        CondvarWait(CondvarWaitInner::Init {
            condvar: self,
            guard,
        })
    }

    /// 唤醒一个等待中的任务
    pub fn wake(&self) {
        loop {
            let Some(waker) = self.queue.lock().pop_front() else {
                return;
            };

            if waker
                .status
                .compare_exchange(
                    CondvarWaker::STATUS_WAITING,
                    CondvarWaker::STATUS_ACQUIRED,
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_err()
            {
                continue;
            }

            waker.waker.wake_by_ref();
            return;
        }
    }

    /// 唤醒全部等待中的任务
    pub fn wake_all(&self) {
        loop {
            let Some(waker) = self.queue.lock().pop_front() else {
                return;
            };

            if waker
                .status
                .compare_exchange(
                    CondvarWaker::STATUS_WAITING,
                    CondvarWaker::STATUS_ACQUIRED,
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_err()
            {
                continue;
            }

            waker.waker.wake_by_ref();
        }
    }

    fn queue(&self, waker: Arc<CondvarWaker>) {
        self.queue.lock().push_back(waker);
    }
}

enum CondvarWaitInner<'cond, 'lock, T> {
    Init {
        condvar: &'cond Condvar,
        guard: MutexGuard<'lock, T>,
    },
    Wait {
        condvar: &'cond Condvar,
        waker: Arc<CondvarWaker>,
        mutex: &'lock Mutex<T>,
    },
    Relock {
        condvar: &'cond Condvar,
        locking: MutexLockFuture<'lock, T>,
    },
    Done,
}

struct CondvarWaker {
    waker: Waker,
    status: AtomicU32,
}

impl CondvarWaker {
    const STATUS_WAITING: u32 = 0;
    const STATUS_ACQUIRED: u32 = 1;
    const STATUS_GIVEUP: u32 = 2;
}

impl<'cond, 'lock, T> Future for CondvarWait<'cond, 'lock, T> {
    type Output = MutexGuard<'lock, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match &mut self.0 {
                CondvarWaitInner::Init { condvar, guard } => {
                    let waker = Arc::new(CondvarWaker {
                        waker: cx.waker().clone(),
                        status: AtomicU32::new(CondvarWaker::STATUS_WAITING),
                    });
                    condvar.queue(waker.clone());
                    self.0 = CondvarWaitInner::Wait {
                        condvar,
                        waker: waker,
                        mutex: guard.mutex,
                    }
                }
                CondvarWaitInner::Wait {
                    condvar,
                    waker,
                    mutex,
                } => {
                    if waker.status.load(Ordering::Acquire) != CondvarWaker::STATUS_ACQUIRED {
                        return Poll::Pending;
                    }
                    self.0 = CondvarWaitInner::Relock {
                        condvar,
                        locking: mutex.lock(),
                    }
                }
                CondvarWaitInner::Relock {
                    condvar: _,
                    locking,
                } => match Pin::new(locking).poll(cx) {
                    Poll::Ready(result) => {
                        self.0 = CondvarWaitInner::Done;
                        return Poll::Ready(result);
                    }
                    Poll::Pending => return Poll::Pending,
                },
                CondvarWaitInner::Done => panic!("future polled after complete"),
            }
        }
    }
}

impl<T> Drop for CondvarWait<'_, '_, T> {
    fn drop(&mut self) {
        match &mut self.0 {
            CondvarWaitInner::Init { .. } => {}
            CondvarWaitInner::Wait { condvar, waker, .. } => {
                let status = waker
                    .status
                    .swap(CondvarWaker::STATUS_GIVEUP, Ordering::AcqRel);
                if status == CondvarWaker::STATUS_ACQUIRED {
                    condvar.wake();
                }
            }
            CondvarWaitInner::Relock { condvar, .. } => {
                condvar.wake();
            }
            CondvarWaitInner::Done => {}
        }
    }
}
