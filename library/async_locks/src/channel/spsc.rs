//! spsc是单生产者、单消费者。内部使用ringbuffer和信号量实现消息传递。
//! 
//! 通过 [channel] 函数创建一组 [Sender] 和 [Receiver]，两个对象互相关联。
//! 同时，内部将分配固定大小的缓冲区，供发送方与接收方传递消息。
//! 之后，当调用 [Sender::send] 或 [Sender::try_send] 时，数据将被发送至缓冲区，并在
//! 调用 [Receiver::recv] 或 [Receiver::try_recv] 时，数据从缓冲区移出。
//! 
//! 当任意一方被drop后，另一方都会收到错误，而不会永远阻塞。
//! 当 [Sender] 被 drop 后，[Receiver] 在读完缓冲区的所有数据后，会返回错误指示发送方已丢失。
//! 当 [Receiver] 被 drop 后，后续 [Sender] 再次发送数据时将会收到错误，指示接收方已丢失。
//! 缓冲区中的数据将被保留，直到另一方也被drop时释放。
//! 
//! 虽然性能会较差，但可以将 [Sender] 包装为 [Arc<Mutex<Sender>>]，当作多生产者使用。
//! 同样，将 [Receiver] 包装为 [Arc<Mutex<Receiver>>]，当作多消费者使用。

use core::{
    cell::UnsafeCell,
    mem::forget,
    pin::Pin,
    sync::atomic::{AtomicU32, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{boxed::Box, sync::Arc, vec::Vec};

use crate::semaphore::{Semaphore, SemaphoreAcquireFuture};

pub struct Sender<T> {
    inner: Arc<BoundedInner<T>>,
    index: usize,
    size: usize,
    lost_receiver: bool,
}

pub struct Receiver<T> {
    inner: Arc<BoundedInner<T>>,
    index: usize,
    size: usize,
    lost_sender: bool,
}

pub struct ReceiverLost<T>(pub T);

pub enum TrySendError<T> {
    BufferFull(T),
    ReceiverLost(T),
}

pub struct SenderLost;

pub enum TryReceiveError {
    BufferEmpty,
    SenderLost,
}

/// 创建一对管道，具有大小为 size 的内部缓冲区
/// 
/// # Panic
/// size 至少为1
pub fn channel<T>(size: usize) -> (Sender<T>, Receiver<T>) {
    assert!(size >= 1);
    let mut buffer = Vec::with_capacity(size);
    buffer.resize_with(size, || Option::None);
    let inner = Arc::new(BoundedInner {
        buffer: UnsafeCell::new(buffer.into_boxed_slice()),
        producer: Semaphore::new(size),
        consumer: Semaphore::new(0),
        txstate: AtomicU32::new(STATE_IDLE),
        txwaker: UnsafeCell::new(None),
        rxstate: AtomicU32::new(STATE_IDLE),
        rxwaker: UnsafeCell::new(None),
    });

    let sender = Sender {
        inner: inner.clone(),
        index: 0,
        size,
        lost_receiver: false,
    };
    let receiver = Receiver {
        inner,
        index: 0,
        size,
        lost_sender: false,
    };

    (sender, receiver)
}

impl<T> Sender<T> {
    /// 不阻塞，尝试将数据发送至对端。
    pub fn try_send(&mut self, data: T) -> Result<(), TrySendError<T>> {
        if !self.lost_receiver {
            if self.inner.txstate.load(Ordering::Acquire) == STATE_DROP {
                self.lost_receiver = true;
            }
        }

        if self.lost_receiver {
            return Err(TrySendError::ReceiverLost(data));
        }

        let guard = self.inner.producer.try_acquire(1);
        if guard.is_none() {
            return Err(TrySendError::BufferFull(data));
        }

        forget(guard);
        self.write_solt(data);
        Ok(())
    }

    /// 将数据发送至对端
    /// 
    /// # 取消安全
    /// 取消后，数据将不会被发送至对端，不会破坏内部状态，且可以再次调用
    pub async fn send(&mut self, data: T) -> Result<(), ReceiverLost<T>> {
        if self.lost_receiver {
            return Err(ReceiverLost(data));
        }

        match (SendReserveFuture::Init { sender: self }).await {
            Ok(_) => {
                self.write_solt(data);
                Ok(())
            }
            Err(_) => {
                self.lost_receiver = true;
                Err(ReceiverLost(data))
            }
        }
    }

    fn write_solt(&mut self, data: T) {
        // Safety: 我们已通过信号量确认buffer的下一个solt只有我们在访问
        unsafe {
            *self.inner.slot(self.index % self.size) = Some(data);
        }
        self.index += 1;
        self.inner.consumer.release(1);
    }
}

impl<T> Receiver<T> {
    /// 不阻塞，尝试从缓冲区接收一个数据
    pub fn try_recv(&mut self) -> Result<T, TryReceiveError> {
        let guard = self.inner.consumer.try_acquire(1);
        if guard.is_some() {
            forget(guard);
            return Ok(self.recv_solt());
        }

        if !self.lost_sender {
            if self.inner.rxstate.load(Ordering::Acquire) == STATE_DROP {
                self.lost_sender = true;
            }
        }

        if self.lost_sender {
            return Err(TryReceiveError::SenderLost);
        }

        Err(TryReceiveError::BufferEmpty)
    }

    /// 从缓冲区接收一个数据
    /// 
    /// # 取消安全
    /// 安全，取消不会破坏内部状态，且可以再次调用
    pub async fn recv(&mut self) -> Result<T, SenderLost> {
        loop {
            if self.lost_sender {
                let guard = self.inner.consumer.try_acquire(1);
                if guard.is_some() {
                    forget(guard);
                    return Ok(self.recv_solt());
                }
                return Err(SenderLost);
            }

            match (RecvReserveFuture::Init { receiver: self }).await {
                Ok(_) => {
                    return Ok(self.recv_solt());
                }
                Err(_) => {
                    self.lost_sender = true;
                    // 再次try_acquire，因为发送方可能在drop前发送了内容
                    continue;
                }
            }
        }
    }

    fn recv_solt(&mut self) -> T {
        // Safety: 我们已通过信号量确认buffer的下一个solt只有我们在访问
        let data = unsafe { self.inner.slot(self.index % self.size).take().unwrap() };
        self.index += 1;
        self.inner.producer.release(1);
        data
    }
}

struct BoundedInner<T> {
    buffer: UnsafeCell<Box<[Option<T>]>>, // ring-buffer
    producer: Semaphore,
    consumer: Semaphore,
    txstate: AtomicU32,
    txwaker: UnsafeCell<Option<Waker>>,
    rxstate: AtomicU32,
    rxwaker: UnsafeCell<Option<Waker>>,
}

const STATE_IDLE: u32 = 0;
const STATE_WAITING: u32 = 1;
const STATE_DROP: u32 = 2;

/// Safety: UnsafeCell的存在使得BoundedInner<T>不再具有Send和Sync，
/// 我们需要令BoundedInner具有Send和Sync，以便Sender和Receiver可以在不同的线程中访问
unsafe impl<T: Send> Send for BoundedInner<T> {}
unsafe impl<T: Send> Sync for BoundedInner<T> {}

impl<T> BoundedInner<T> {
    /// Safety: 由调用方保证产生&mut借用不会产生问题
    unsafe fn slot(&self, index: usize) -> &mut Option<T> {
        // Safety: 由调用方保证数据竞争的安全性
        unsafe { &mut (*self.buffer.get())[index] }
    }
}

enum SendReserveFuture<'a, T> {
    Init {
        sender: &'a Sender<T>,
    },
    Acquiring {
        sender: &'a Sender<T>,
        fut: SemaphoreAcquireFuture<'a>,
    },
    Done,
}

enum RecvReserveFuture<'a, T> {
    Init {
        receiver: &'a Receiver<T>,
    },
    Acquiring {
        receiver: &'a Receiver<T>,
        fut: SemaphoreAcquireFuture<'a>,
    },
    Done,
}

impl<T> Future for SendReserveFuture<'_, T> {
    type Output = Result<(), ()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.as_mut().get_mut() {
                Self::Init { sender } => {
                    unsafe {
                        *sender.inner.txwaker.get() = Some(cx.waker().clone());
                    }
                    let swap = sender.inner.txstate.compare_exchange(
                        STATE_IDLE,
                        STATE_WAITING,
                        Ordering::Release,
                        Ordering::Relaxed,
                    );
                    if swap.is_err() {
                        unsafe {
                            *sender.inner.txwaker.get() = None;
                        }
                        *self = Self::Done;
                        return Poll::Ready(Err(()));
                    }
                    *self = Self::Acquiring {
                        sender,
                        fut: sender.inner.producer.acquire(1),
                    };
                }
                Self::Acquiring { sender, fut } => {
                    if sender.inner.txstate.load(Ordering::Acquire) == STATE_DROP {
                        *self = Self::Done;
                        return Poll::Ready(Err(()));
                    }

                    match Pin::new(fut).poll(cx) {
                        Poll::Ready(guard) => {
                            forget(guard);
                            let swap = sender.inner.txstate.compare_exchange(
                                STATE_WAITING,
                                STATE_IDLE,
                                Ordering::Release,
                                Ordering::Relaxed,
                            );
                            if swap.is_ok() {
                                unsafe {
                                    *sender.inner.txwaker.get() = None;
                                }
                            }
                            *self = Self::Done;
                            return Poll::Ready(Ok(()));
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
                Self::Done => {
                    panic!("future polled after complete")
                }
            }
        }
    }
}

impl<T> Future for RecvReserveFuture<'_, T> {
    type Output = Result<(), ()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.as_mut().get_mut() {
                Self::Init { receiver } => {
                    unsafe {
                        *receiver.inner.rxwaker.get() = Some(cx.waker().clone());
                    }
                    let swap = receiver.inner.rxstate.compare_exchange(
                        STATE_IDLE,
                        STATE_WAITING,
                        Ordering::Release,
                        Ordering::Relaxed,
                    );
                    if swap.is_err() {
                        unsafe {
                            *receiver.inner.rxwaker.get() = None;
                        }
                        *self = Self::Done;
                        return Poll::Ready(Err(()));
                    }
                    *self = Self::Acquiring {
                        receiver,
                        fut: receiver.inner.consumer.acquire(1),
                    };
                }
                Self::Acquiring { receiver, fut } => {
                    if receiver.inner.rxstate.load(Ordering::Acquire) == STATE_DROP {
                        *self = Self::Done;
                        return Poll::Ready(Err(()));
                    }

                    match Pin::new(fut).poll(cx) {
                        Poll::Ready(guard) => {
                            forget(guard);
                            let swap = receiver.inner.rxstate.compare_exchange(
                                STATE_WAITING,
                                STATE_IDLE,
                                Ordering::Release,
                                Ordering::Relaxed,
                            );
                            if swap.is_ok() {
                                unsafe {
                                    *receiver.inner.rxwaker.get() = None;
                                }
                            }
                            *self = Self::Done;
                            return Poll::Ready(Ok(()));
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
                Self::Done => {
                    panic!("future polled after complete")
                }
            }
        }
    }
}

impl<T> Drop for SendReserveFuture<'_, T> {
    fn drop(&mut self) {
        if let Self::Acquiring { sender, .. } = self {
            // 移除自己的waker
            let swap = sender.inner.txstate.compare_exchange(
                STATE_WAITING,
                STATE_IDLE,
                Ordering::Release,
                Ordering::Relaxed,
            );
            if swap.is_ok() {
                // Safety: 我们使用原子操作避免竞争
                unsafe {
                    *(sender.inner.txwaker.get()) = None;
                }
            }
        }
    }
}

impl<T> Drop for RecvReserveFuture<'_, T> {
    fn drop(&mut self) {
        if let Self::Acquiring { receiver, .. } = self {
            // 移除自己的waker
            let swap = receiver.inner.rxstate.compare_exchange(
                STATE_WAITING,
                STATE_IDLE,
                Ordering::Release,
                Ordering::Relaxed,
            );
            if swap.is_ok() {
                // Safety: 我们使用原子操作避免竞争
                unsafe {
                    *(receiver.inner.rxwaker.get()) = None;
                }
            }
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // 唤醒Receiver
        let prev = self.inner.rxstate.swap(STATE_DROP, Ordering::AcqRel);
        if prev == STATE_WAITING {
            // Safety: 我们使用原子操作避免竞争
            let waker = unsafe { (*self.inner.rxwaker.get()).take() };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        // 唤醒Producer
        let prev = self.inner.txstate.swap(STATE_DROP, Ordering::AcqRel);
        if prev == STATE_WAITING {
            // Safety: 我们使用原子操作避免竞争
            let waker = unsafe { (*self.inner.txwaker.get()).take() };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
}
