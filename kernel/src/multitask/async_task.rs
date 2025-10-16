use core::{
    pin::Pin,
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
    task::{Context, Poll, Waker},
    time::Duration,
};

use alloc::{
    collections::binary_heap::BinaryHeap,
    sync::{Arc, Weak},
};

use crate::sync::{IrqGuard, SpinLock};

// 系统时间，以us为单位
static SYSTEM_INSTANT: AtomicU64 = AtomicU64::new(0);

// 唤醒列表
static SLEEP_WAKE_QUEUE: SpinLock<BinaryHeap<WakeQueue>> = SpinLock::new(BinaryHeap::new());

const FLAG_INIT: u8 = 0;
const FLAG_SLEEP: u8 = 1;
const FLAG_WAKE: u8 = 2;

struct WakeQueue {
    wake_time: u64,
    flag: Weak<AtomicU8>,
    waker: Waker,
}

impl PartialEq for WakeQueue {
    fn eq(&self, other: &Self) -> bool {
        self.wake_time == other.wake_time
    }
}

impl PartialOrd for WakeQueue {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for WakeQueue {}

impl Ord for WakeQueue {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // 反转比较顺序，因为我们要把WakeQueue放入BinaryHeap中，而BinaryHeap是max-heap，我们需要的是min-heap
        other.wake_time.cmp(&self.wake_time)
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Sleep {
    duration: u64,
    flag: Arc<AtomicU8>,
}

/// 等待指定时间后唤醒
pub fn sleep(time: Duration) -> Sleep {
    Sleep {
        duration: time.as_micros() as u64,
        flag: Arc::new(AtomicU8::new(FLAG_INIT)),
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let flag = self.flag.load(Ordering::Acquire);
        match flag {
            FLAG_INIT => {
                self.flag.store(FLAG_SLEEP, Ordering::Release);

                let wake = WakeQueue {
                    wake_time: SYSTEM_INSTANT.load(Ordering::Acquire) + self.duration,
                    flag: Arc::downgrade(&self.flag),
                    waker: cx.waker().clone(),
                };

                let _guard = unsafe { IrqGuard::cli() };
                let mut queue = SLEEP_WAKE_QUEUE.lock();
                queue.push(wake);

                Poll::Pending
            }
            FLAG_SLEEP => Poll::Pending,
            FLAG_WAKE => Poll::Ready(()),
            _ => unreachable!(),
        }
    }
}

/// 计时器硬中断使用，传入流经的时间，并唤醒等待中的任务
pub fn tick(elapsed: u64) {
    let now = SYSTEM_INSTANT.fetch_add(elapsed, Ordering::SeqCst) + elapsed;
    let mut queue = SLEEP_WAKE_QUEUE.lock();
    while let Some(wake) = queue.peek() {
        if wake.wake_time > now {
            break;
        }
        let Some(wake) = queue.pop() else {
            break;
        };
        if let Some(flag) = wake.flag.upgrade() {
            flag.store(FLAG_WAKE, Ordering::Release);
        }
        wake.waker.wake();
    }
}
