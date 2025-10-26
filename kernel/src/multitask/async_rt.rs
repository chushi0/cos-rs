use core::{
    cell::UnsafeCell,
    marker::PhantomPinned,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    task::{Context, RawWaker, RawWakerVTable, Waker},
};

use alloc::{
    boxed::Box,
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::{Arc, Weak},
};

use crate::{
    multitask::thread,
    sync::{
        int::{IrqGuard, sti},
        spin::SpinLock,
    },
};

// 运行时结构
static RUNTIME: SpinLock<Runtimer> = SpinLock::new(Runtimer::new());
// task id 分配
static TASK_ID_GENERATOR: AtomicU64 = AtomicU64::new(0);

/// 内核使用的异步运行时
pub struct Runtimer {
    /// 已经就绪、待执行的任务
    ready: VecDeque<Pin<Arc<UnsafeCell<Task>>>>,
    /// 等待中的任务，需要Waker将其触发
    suspend: BTreeMap<u64, Pin<Arc<UnsafeCell<Task>>>>,
}

unsafe impl Send for Runtimer {}
unsafe impl Sync for Runtimer {}

type PinBoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// 对任务的抽象
struct Task {
    /// 任务ID
    task_id: u64,
    /// 实际任务，包含了执行上下文信息
    future: PinBoxFuture,
    /// waker，每个任务全程只使用一个Waker
    /// waker 内部的指针是 Arc::<Arc<WakerInner>>::into_raw
    waker: Waker,
    /// 状态，true - ready / false - suspend
    status: AtomicBool,
    /// Task中的waker引用了自身（Task），因此为!Unpin
    _phantom_pin: PhantomPinned,
}

/// Waker
struct WakerInner {
    // 持有task的弱引用，当task执行完成并释放后，后续的wake操作将不再有效
    task: UnsafeCell<Weak<UnsafeCell<Task>>>,
}

const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    WakerInner::clone,
    WakerInner::wake,
    WakerInner::wake_by_ref,
    WakerInner::drop,
);

impl Runtimer {
    const fn new() -> Self {
        Self {
            ready: VecDeque::new(),
            suspend: BTreeMap::new(),
        }
    }
}

impl WakerInner {
    unsafe fn clone(waker: *const ()) -> RawWaker {
        unsafe {
            // 重新转为Arc，并进行复制
            let waker = Arc::from_raw(waker as *const WakerInner);
            let cloned = waker.clone();
            // 之前的重新用into_raw泄漏
            _ = Arc::into_raw(waker);
            // 新的包装为RawWaker返回
            RawWaker::new(Arc::into_raw(cloned) as *const (), &WAKER_VTABLE)
        }
    }

    unsafe fn wake(waker: *const ()) {
        unsafe {
            Self::wake_by_ref(waker);
            Self::drop(waker);
        }
    }

    unsafe fn wake_by_ref(waker: *const ()) {
        unsafe {
            // 如果成功将任务加入队列，则为true
            let mut push_back = false;

            // 转为正确的指针
            let waker = waker as *const WakerInner;
            // 尝试获取弱引用
            let task = &*(*waker).task.get();
            if let Some(task) = task.upgrade() {
                let _guard = IrqGuard::cli();
                let mut rt = RUNTIME.lock();

                // 设置为就绪
                (*task.get()).status.store(true, Ordering::Release);

                // 如果在等待队列中，则将其放入就绪队列
                if let Some(task) = rt.suspend.remove(&(*task.get()).task_id) {
                    rt.ready.push_back(task);
                    push_back = true;
                }
            }

            // 如果已经放回了，将内核线程唤醒
            if push_back {
                thread::wake_kernel_thread();
            }
        }
    }

    unsafe fn drop(waker: *const ()) {
        unsafe {
            // 直接转为Arc对象，Arc释放时会自己回收
            Arc::from_raw(waker as *const WakerInner);
        }
    }
}

/// 执行异步运行时主函数
///
/// 该函数将不断地获取异步任务并执行。当所有任务均执行完毕后，会执行 hlt 并等待中断
pub fn run() -> ! {
    // 开中断，异步任务需要中断驱动
    unsafe {
        sti();
    }
    loop {
        // 获取一个任务
        let task = {
            let _guard = unsafe { IrqGuard::cli() };
            let mut rt = RUNTIME.lock();
            rt.ready.pop_front()
        };

        if let Some(task) = task {
            let waker = unsafe { (*task.get()).waker.clone() };
            let mut cx = Context::from_waker(&waker);

            // 标记当前为中断
            unsafe {
                (*task.get()).status.store(false, Ordering::Relaxed);
            }

            // 执行
            let result = unsafe { (*task.get()).future.as_mut().poll(&mut cx) };

            // 如果未完成，则重新放入队列中
            if result.is_pending() {
                let task_id = unsafe { (*task.get()).task_id };
                let _guard = unsafe { IrqGuard::cli() };
                let mut rt = RUNTIME.lock();
                // 如果状态为就绪，放入就绪队列，否则放入等待队列
                let status = unsafe { (*task.get()).status.load(Ordering::Acquire) };
                if status {
                    rt.ready.push_back(task);
                } else {
                    rt.suspend.insert(task_id, task);
                }
            }
        } else {
            // 没有任务了，挂起当前线程并执行其他线程
            thread::thread_yield(true);
        }
    }
}

/// 生成一个新的异步任务，加入到全局任务池中
pub fn spawn<Fut>(fut: Fut)
where
    Fut: Future<Output = ()> + Send + 'static,
{
    let future = Box::pin(fut) as PinBoxFuture;
    let waker_inner = Arc::new(WakerInner {
        task: UnsafeCell::new(Weak::new()),
    });
    let task = Task {
        task_id: TASK_ID_GENERATOR.fetch_add(1, Ordering::SeqCst),
        future,
        waker: unsafe {
            Waker::from_raw(RawWaker::new(
                Arc::as_ptr(&waker_inner) as *mut (),
                &WAKER_VTABLE,
            ))
        },
        status: AtomicBool::new(true),
        _phantom_pin: PhantomPinned,
    };
    let task = Arc::new(UnsafeCell::new(task));

    // 我们需要将一个Weak放入waker_inner中
    unsafe {
        *waker_inner.task.get() = Arc::downgrade(&task);
    }
    // 我们在构造task时使用了Arc::as_ptr，本质上已经拿走了Arc的所有权，因此这里需要将Arc泄漏
    // （WakerInner在drop时会释放Arc）
    _ = Arc::into_raw(waker_inner);

    let _guard = unsafe { IrqGuard::cli() };
    let mut rt = RUNTIME.lock();
    // 加入到异步队列
    unsafe {
        rt.ready.push_back(Pin::new_unchecked(task));
    }
}
