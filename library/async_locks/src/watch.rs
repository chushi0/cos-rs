//! watch 是发布-订阅模型。内部使用锁、Waker实现。
//!
//! 通过 [pair] 函数创建一组 [Publisher] 和 [Subscriber]，两个对象互相关联。
//! 当调用 [Publisher::send] 后，内容变化将同步至所有 [Subscriber]，并唤醒 [Subscriber::changed]。
//! 如果短时间频繁调用 [Publisher::send]，[Subscribe::changed] 可能被多次唤醒，但可能无法观测到中间值。

use core::{
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{sync::Arc, vec::Vec};

use crate::spin::SpinLock;

pub struct Publisher<T> {
    inner: Arc<Inner<T>>,
}

pub struct Subscriber<T> {
    inner: Arc<Inner<T>>,
    waker: Arc<WakerSolt>,
    version: u64,
    value: T,
}

#[derive(Debug)]
pub struct PublisherLost;

/// Cancel Safety: 函数是取消安全的
pub struct SubscriberChanged<'a, T> {
    inner: &'a mut Subscriber<T>,
    init_waker: bool,
}

struct WakerSolt {
    waker: SpinLock<Waker>,
}

struct Inner<T> {
    publisher_lost: AtomicBool,
    value: SpinLock<T>,
    version: AtomicU64,
    waker: SpinLock<Vec<Arc<WakerSolt>>>,
}

pub fn pair<T: Clone>(value: T) -> (Publisher<T>, Subscriber<T>) {
    let waker_solt = Arc::new(WakerSolt {
        waker: SpinLock::new(Waker::noop().clone()),
    });

    let inner = Arc::new(Inner {
        publisher_lost: AtomicBool::new(false),
        value: SpinLock::new(value.clone()),
        version: AtomicU64::new(0),
        waker: SpinLock::new(alloc::vec![waker_solt.clone()]),
    });

    let subscriber = Subscriber {
        inner: inner.clone(),
        waker: waker_solt,
        version: 0,
        value,
    };
    let publisher = Publisher { inner: inner };

    (publisher, subscriber)
}

impl<T> Publisher<T> {
    pub fn send(&mut self, value: T) {
        *self.inner.value.lock() = value;
        self.inner.version.fetch_add(1, Ordering::Release);

        for waker in self.inner.waker.lock().iter() {
            waker.waker.lock().wake_by_ref();
        }
    }
}

impl<T: Clone> Subscriber<T> {
    fn sync(&mut self) -> bool {
        let version = self.inner.version.load(Ordering::Acquire);
        if version != self.version {
            self.version = version;
            self.value = self.inner.value.lock().clone();
            return true;
        }

        false
    }

    pub fn borrow(&mut self) -> &T {
        self.sync();
        &self.value
    }

    pub fn wait(&mut self) -> SubscriberChanged<'_, T> {
        SubscriberChanged {
            inner: self,
            init_waker: false,
        }
    }
}

impl<T: Clone> Clone for Subscriber<T> {
    fn clone(&self) -> Subscriber<T> {
        let waker = Arc::new(WakerSolt {
            waker: SpinLock::new(Waker::noop().clone()),
        });

        if !self.inner.publisher_lost.load(Ordering::Acquire) {
            self.inner.waker.lock().push(waker.clone());
        }

        Subscriber {
            inner: self.inner.clone(),
            waker,
            version: self.version,
            value: self.value.clone(),
        }
    }
}

impl<T: Clone> Future for SubscriberChanged<'_, T> {
    type Output = Result<T, PublisherLost>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.init_waker {
            *self.inner.waker.waker.lock() = cx.waker().clone();
            self.init_waker = true;
        }

        if self.inner.inner.publisher_lost.load(Ordering::Acquire) {
            *self.inner.waker.waker.lock() = cx.waker().clone();
            self.init_waker = false;
            return Poll::Ready(Err(PublisherLost));
        };

        if self.inner.sync() {
            *self.inner.waker.waker.lock() = cx.waker().clone();
            self.init_waker = false;
            return Poll::Ready(Ok(self.inner.value.clone()));
        }

        Poll::Pending
    }
}

impl<T> Drop for SubscriberChanged<'_, T> {
    fn drop(&mut self) {
        if self.init_waker {
            *self.inner.waker.waker.lock() = Waker::noop().clone();
        }
    }
}

impl<T> Drop for Publisher<T> {
    fn drop(&mut self) {
        self.inner.publisher_lost.store(true, Ordering::Release);
        for waker in self.inner.waker.lock().iter() {
            waker.waker.lock().wake_by_ref();
        }
    }
}

impl<T> Drop for Subscriber<T> {
    fn drop(&mut self) {
        self.inner
            .waker
            .lock()
            .retain(|solt| !Arc::ptr_eq(solt, &self.waker));
    }
}
