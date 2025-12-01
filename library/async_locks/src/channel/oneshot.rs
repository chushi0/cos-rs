//! oneshot是只能使用一次的管道，内部使用锁、条件变量等方式实现
//!
//! 通过 [channel] 函数创建一组 [Sender] 和 [Receiver]，两个对象相互关联。
//! 当调用 [Sender::send] 后，内容便可以从 [Receiver::recv] 接收。
//! [Sender::send] 和 [Receiver::recv] 均会获取对象的所有权，以防重复调用。
//!
//! 如果在发送前将 [Sender] 丢弃，那么 [Receiver::recv] 会返回 [SenderLost]。

use core::{
    cell::UnsafeCell,
    pin::Pin,
    sync::atomic::{AtomicU32, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::sync::Arc;

use crate::{
    condvar::{Condvar, CondvarWait},
    mutex::{Mutex, MutexLockFuture},
};

pub struct Sender<T> {
    inner: Arc<OneshotInner<T>>,
    done: bool,
}

pub struct Receiver<T> {
    inner: Arc<OneshotInner<T>>,
}

pub struct SenderLost;

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(OneshotInner {
        data: Mutex::new(None),
        status: AtomicU32::new(STATUS_NO_RECV),
        rx_waker: UnsafeCell::new(None),
        condvar: Condvar::new(),
    });

    let sender = Sender {
        inner: inner.clone(),
        done: false,
    };
    let receiver = Receiver { inner };

    (sender, receiver)
}

impl<T> Sender<T> {
    /// 将数据发送至对应的 [Receiver]
    ///
    /// # 取消安全
    /// 取消后，[Receiver::recv] 会收到 [SenderLost]。
    /// 由于Rust的所有权限制，无法再次发送数据
    pub async fn send(mut self, data: T) {
        let mut guard = self.inner.data.lock().await;
        *guard = Some(data);
        drop(guard);
        self.inner.condvar.wake();
        self.done = true;
    }
}

impl<T> Receiver<T> {
    /// 等待并获取由 [Sender] 发送的数据
    ///
    /// # 取消安全
    /// 取消任务不会有任何影响，[Sender]依然可以发送数据，但会在发送后立即被drop
    pub async fn recv(self) -> Result<T, SenderLost> {
        ReceiverFuture::Init { inner: &self.inner }.await
    }
}

struct OneshotInner<T> {
    status: AtomicU32,
    rx_waker: UnsafeCell<Option<Waker>>,
    data: Mutex<Option<T>>,
    condvar: Condvar,
}

enum ReceiverFuture<'a, T> {
    Init {
        inner: &'a OneshotInner<T>,
    },
    Locking {
        inner: &'a OneshotInner<T>,
        locking: MutexLockFuture<'a, Option<T>>,
    },
    Waiting {
        inner: &'a OneshotInner<T>,
        waiting: CondvarWait<'a, 'a, Option<T>>,
    },
}

const STATUS_NO_RECV: u32 = 1;
const STATUS_RECVING: u32 = 2;
const STATUS_DONE: u32 = 3;
const STATUS_LOST: u32 = 4;

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if !self.done {
            let prev = self.inner.status.swap(STATUS_LOST, Ordering::AcqRel);
            if prev == STATUS_RECVING {
                unsafe {
                    (*self.inner.rx_waker.get()).as_ref().unwrap().wake_by_ref();
                }
            }
        }
    }
}

impl<T> Future for ReceiverFuture<'_, T> {
    type Output = Result<T, SenderLost>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.as_mut().get_mut() {
                ReceiverFuture::Init { inner } => {
                    unsafe {
                        *inner.rx_waker.get() = Some(cx.waker().clone());
                    }

                    if let Err(prev) = inner.status.compare_exchange(
                        STATUS_NO_RECV,
                        STATUS_RECVING,
                        Ordering::Release,
                        Ordering::Acquire,
                    ) {
                        if prev == STATUS_DONE {
                            panic!("future polled after complete")
                        }
                        if prev == STATUS_LOST {
                            return Poll::Ready(Err(SenderLost));
                        }
                    }

                    *self.as_mut().get_mut() = ReceiverFuture::Locking {
                        inner,
                        locking: inner.data.lock(),
                    }
                }
                ReceiverFuture::Locking { inner, locking } => {
                    let prev = inner.status.load(Ordering::Acquire);
                    if prev == STATUS_DONE {
                        panic!("future polled after complete")
                    }
                    if prev == STATUS_LOST {
                        return Poll::Ready(Err(SenderLost));
                    }

                    let mut result = match Pin::new(locking).poll(cx) {
                        Poll::Ready(result) => result,
                        Poll::Pending => return Poll::Pending,
                    };

                    if let Some(data) = result.take() {
                        inner.status.store(STATUS_DONE, Ordering::Release);
                        return Poll::Ready(Ok(data));
                    }

                    *self.as_mut().get_mut() = ReceiverFuture::Waiting {
                        inner,
                        waiting: inner.condvar.wait(result),
                    }
                }
                ReceiverFuture::Waiting { inner, waiting } => {
                    let prev = inner.status.load(Ordering::Acquire);
                    if prev == STATUS_DONE {
                        panic!("future polled after complete")
                    }
                    if prev == STATUS_LOST {
                        return Poll::Ready(Err(SenderLost));
                    }

                    let mut result = match Pin::new(waiting).poll(cx) {
                        Poll::Ready(result) => result,
                        Poll::Pending => return Poll::Pending,
                    };

                    if let Some(data) = result.take() {
                        inner.status.store(STATUS_DONE, Ordering::Release);
                        return Poll::Ready(Ok(data));
                    }

                    *self.as_mut().get_mut() = ReceiverFuture::Waiting {
                        inner,
                        waiting: inner.condvar.wait(result),
                    }
                }
            }
        }
    }
}
