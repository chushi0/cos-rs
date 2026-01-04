use core::{
    cell::UnsafeCell,
    marker::PhantomPinned,
    mem::forget,
    pin::{Pin, pin},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use alloc::{
    boxed::Box,
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::{Arc, Weak},
};

use crate::{
    multitask::{
        self,
        thread::{self, Thread, Yield},
    },
    sync::{
        int::{IrqGuard, sti},
        spin::{SpinLock, SpinLockGuard},
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
            // 之前的重新泄漏
            forget(waker);
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
    sti();
    loop {
        let guard = IrqGuard::cli();
        let mut rt = RUNTIME.lock();
        // 获取一个任务
        let task = rt.ready.pop_front();

        if let Some(task) = task {
            drop((guard, rt));

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
                let _guard = IrqGuard::cli();
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
            // 这里不需要drop guard，或者说不能drop guard，原因：
            // 1. 无需drop：thread::thread_yield_with不要求是否开关中断，而且它会在内部自己关中断
            // 2. 不能drop：我们依然持有RUNTIME的lock，如果此时被中断打断，而中断处理程序或其他线程尝试获取锁，会导致死锁
            // 在线程重新切换回来后，会自动调用guard的drop，此时中断会被重新开启
            // 而在切到其他线程后，对应线程也会重新打开中断
            struct YieldContext<'a> {
                _rt: SpinLockGuard<'a, Runtimer>,
            }
            let mut context = Some(YieldContext { _rt: rt });
            unsafe fn yield_vtable(context: *mut ()) {
                let context = context as *mut Option<YieldContext<'_>>;
                unsafe {
                    (*context).take();
                }
            }

            thread::thread_yield_with(Yield {
                context: &raw mut context as *mut (),
                vtable: yield_vtable,
            });
        }
    }
}

/// 生成一个新的异步任务，加入到全局任务池中
///
/// 任务会被pin在堆上，并在专门的线程中执行。执行异步任务的线程栈大小为2M
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
    forget(waker_inner);
    {
        let _guard = IrqGuard::cli();
        let mut rt = RUNTIME.lock();
        // 加入到异步队列
        unsafe {
            rt.ready.push_back(Pin::new_unchecked(task));
        }
    }

    // 唤醒异步线程
    thread::wake_kernel_thread();
}

struct BlockOnWakerInner {
    thread: Weak<SpinLock<Thread>>,
    pending: SpinLock<bool>,
}

const BLOCK_ON_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    BlockOnWakerInner::clone,
    BlockOnWakerInner::wake,
    BlockOnWakerInner::wake_by_ref,
    BlockOnWakerInner::drop,
);

impl BlockOnWakerInner {
    unsafe fn clone(waker: *const ()) -> RawWaker {
        unsafe {
            let waker = Weak::from_raw(waker as *const BlockOnWakerInner);
            let clone_waker = Weak::into_raw(waker.clone()) as *const ();
            forget(waker);
            RawWaker::new(clone_waker, &BLOCK_ON_WAKER_VTABLE)
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
            let waker = Weak::from_raw(waker as *const BlockOnWakerInner);
            if let Some(waker) = waker.upgrade() {
                *waker.pending.lock() = false;
                if let Some(thread) = waker.thread.upgrade() {
                    multitask::thread::wake_thread(&thread);
                }
            }
            forget(waker);
        }
    }

    unsafe fn drop(waker: *const ()) {
        unsafe {
            Weak::from_raw(waker as *const BlockOnWakerInner);
        }
    }
}

pub struct ThreadTerminated;

/// 在当前线程上执行异步任务
///
/// 与[spawn]不同，此函数**不会**将异步任务放在堆上，并且会在当前栈中执行，栈空间可能不会太多。
/// 如果需要执行大任务，需要在调用前装箱（[Box::pin]）。
/// 任务的[Waker]与[spawn]的结构不一致，其与当前线程绑定。
/// 当异步任务返回[Poll::Pending]时，会**挂起**当前线程，直到在其结构体上调用[Waker::wake]或[Waker::wake_by_ref]。
///
/// 由于异步任务特性，执行时会强制打开中断。但在函数返回后，会将中断复原为原状态。
pub fn block_on<Fut>(future: Fut) -> Result<Fut::Output, ThreadTerminated>
where
    Fut: Future,
{
    // 开中断
    let _guard = IrqGuard::sti();
    // pin future
    let mut future = pin!(future);
    // waker构造
    let current_thread = multitask::thread::current_thread().unwrap();
    let waker_inner = Arc::new(BlockOnWakerInner {
        thread: Arc::downgrade(&current_thread),
        pending: SpinLock::new(false),
    });
    let waker = unsafe {
        let data = Arc::downgrade(&waker_inner).into_raw() as *const ();
        Waker::from_raw(RawWaker::new(data, &BLOCK_ON_WAKER_VTABLE))
    };

    // 将waker注册到线程中，当线程被kill时唤醒
    multitask::thread::register_waker(&current_thread, Some(waker.clone()));

    loop {
        // 标记pending=true，表示当前返回后需要等待
        *waker_inner.pending.lock() = true;

        // 检查线程是否需要退出
        if multitask::thread::thread_boundry_check() {
            multitask::thread::register_waker(&current_thread, None);
            return Err(ThreadTerminated);
        }

        // 执行异步任务
        let mut cx = Context::from_waker(&waker);
        let result = future.as_mut().poll(&mut cx);

        // 检查结果
        if let Poll::Ready(result) = result {
            multitask::thread::register_waker(&current_thread, None);
            return Ok(result);
        }

        // 如果已经被唤醒了，立刻再次调用poll
        let pending = waker_inner.pending.lock();
        if !*pending {
            continue;
        }

        // 让出线程并挂起
        struct YieldContext<'a> {
            _pending: SpinLockGuard<'a, bool>,
        }
        let mut context = Some(YieldContext { _pending: pending });
        unsafe fn yield_vtable(context: *mut ()) {
            let context = context as *mut Option<YieldContext<'_>>;
            unsafe {
                (*context).take();
            }
        }

        thread::thread_yield_with(Yield {
            context: &raw mut context as *mut (),
            vtable: yield_vtable,
        });
    }
}
