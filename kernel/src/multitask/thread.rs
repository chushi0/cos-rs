use core::{
    arch::{asm, naked_asm},
    mem::MaybeUninit,
    num::NonZeroU64,
    ptr,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{
    boxed::Box,
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::{Arc, Weak},
};

use crate::sync::{IrqGuard, SpinLock};

static THREADS: SpinLock<BTreeMap<u64, Arc<SpinLock<Thread>>>> = SpinLock::new(BTreeMap::new());
static READY_THREADS: SpinLock<VecDeque<Weak<SpinLock<Thread>>>> = SpinLock::new(VecDeque::new());
static TERMINATED_THREADS: SpinLock<VecDeque<Weak<SpinLock<Thread>>>> =
    SpinLock::new(VecDeque::new());
static THREAD_ID_GENERATOR: AtomicU64 = AtomicU64::new(0);

// TODO: 多核CPU后，修改到per-cpu内存页
static mut CURRENT_THREAD: Option<Arc<SpinLock<Thread>>> = None;

// TODO: IDLE THREAD应当每个线程一个
static mut IDLE_THREAD: Option<Arc<SpinLock<Thread>>> = None;
static mut IDLE_THREAD_ID: u64 = 0;

pub struct Thread {
    // 线程ID
    pub thread_id: u64,
    // 进程ID，None表示内核进程
    pub process_id: Option<NonZeroU64>,
    // 上下文，如果当前为Running状态，则此值未定义
    pub context: Context,
    // 线程状态
    pub status: ThreadStatus,
}

#[derive(Debug)]
pub struct Context {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rip: u64,
    pub rsp: u64,
}

// 线程状态
#[derive(Debug, Clone, Copy)]
pub enum ThreadStatus {
    Ready,       // 就绪，调度器可以将此线程调度到CPU上执行
    Running,     // 执行，此线程当前正在CPU上执行
    Suspend,     // 挂起，此线程正在等待某项资源而被暂停执行
    Terminating, // 停止中，此线程已被要求停止，但它仍在占用CPU，在其离开CPU后会被终止
    Terminated,  // 终止，此线程已被终止，但其仍持有资源。稍后内核将对其进行清理
}

impl Context {
    const fn uninit() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

/// 创建线程
///
/// 此函数将创建线程，并将线程添加到全局队列中。
///
/// 如果线程创建成功，返回该线程。如果创建失败，返回None（如内存不足）。
///
/// Safety:
/// start_address表示线程创建后执行的第一条指令的虚拟地址，当线程执行时，需保证此地址可访问，否则会立即触发中断。
/// stack为线程使用的栈位置，注意栈为从高到低增长，且必须对齐到0x8（不能对齐到0x10）。当线程首次使用栈时，需保证此地址可访问，否则会立即触发中断。
/// initial_suspend表示是否在线程创建后立刻挂起，挂起的线程不会运行。调用方需重新唤起线程，否则线程不会执行。
pub unsafe fn create_thread(
    process_id: Option<NonZeroU64>,
    start_address: u64,
    stack: u64,
    initial_suspend: bool,
) -> Option<Arc<SpinLock<Thread>>> {
    let thread_id = THREAD_ID_GENERATOR.fetch_add(1, Ordering::SeqCst) + 1;
    let mut context = Context::uninit();
    context.rip = start_address;
    context.rsp = stack;
    let thread = Thread {
        thread_id,
        process_id,
        context,
        status: if initial_suspend {
            ThreadStatus::Suspend
        } else {
            ThreadStatus::Ready
        },
    };
    let thread = Arc::new(SpinLock::new(thread));

    let _guard = unsafe { IrqGuard::cli() };

    THREADS.lock().insert(thread_id, thread.clone());
    if !initial_suspend {
        READY_THREADS.lock().push_back(Arc::downgrade(&thread));
    }

    Some(thread)
}

/// 创建IDLE线程
pub fn create_idle_thread() {
    extern "C" fn idle_thread_entry() -> ! {
        loop {
            try_yield_thread();
            unsafe {
                asm!("hlt");
            }
        }
    }

    let stack = Box::leak(Box::new(MaybeUninit::<[u8; 4096]>::uninit())) as *mut _ as usize as u64
        + 4096
        - 8;

    let thread_id = THREAD_ID_GENERATOR.fetch_add(1, Ordering::SeqCst) + 1;
    let mut context = Context::uninit();
    context.rsp = stack;
    context.rip = idle_thread_entry as u64;
    let thread = Thread {
        thread_id,
        process_id: None,
        context,
        status: ThreadStatus::Running,
    };
    let thread = Arc::new(SpinLock::new(thread));

    let _guard = unsafe { IrqGuard::cli() };
    THREADS.lock().insert(thread_id, thread.clone());
    unsafe {
        IDLE_THREAD = Some(thread);
        IDLE_THREAD_ID = thread_id;
    }
}

/// 将当前CPU执行线程创建为内核线程
///
/// 此函数将当前线程封装为一个内核线程对象，并加入到全局队列中
/// 线程创建后即自动挂载为当前线程，并立即为运行状态
pub fn create_kernel_thread() {
    let thread_id = THREAD_ID_GENERATOR.fetch_add(1, Ordering::SeqCst) + 1;
    let context = Context::uninit();
    let thread = Thread {
        thread_id,
        process_id: None,
        context,
        status: ThreadStatus::Running,
    };
    let thread = Arc::new(SpinLock::new(thread));

    let _guard = unsafe { IrqGuard::cli() };
    THREADS.lock().insert(thread_id, thread);
}

/// 获取当前正在执行的线程
pub fn current_thread() -> Option<Arc<SpinLock<Thread>>> {
    let _guard = unsafe { IrqGuard::cli() };
    // Safety: CURRENT_THREAD为per-cpu变量，访问是安全的
    unsafe { (*&raw const CURRENT_THREAD).clone() }
}

/// 将当前CPU执行上下文切换到指定上下文
///
/// 此函数不会返回，因为其将进入参数中指定的上下文执行代码
/// 上下文仅包含callee-save的通用寄存器，以及rsp、rip两个特殊寄存器
/// 此函数仅能用于内核态之间进行上下文切换
///
/// Safety:
/// 需保证传入的上下文可以正确执行代码。若无法执行，则会立即引发中断。
unsafe fn switch_to_context(context: *const Context) -> ! {
    unsafe {
        asm!(
            "mov rbp, r8",
            "mov rbx, r9",
            "mov rsp, r10",
            "jmp r11",
            in("r15") (*context).r15,
            in("r14") (*context).r14,
            in("r13") (*context).r13,
            in("r12") (*context).r12,
            in("r8") (*context).rbp,
            in("r9") (*context).rbx,
            in("r10") (*context).rsp,
            in("r11") (*context).rip,
            options(noreturn)
        )
    }
}

/// 切换线程
///
/// 此函数将当前上下文保存至from中，并将当前上下文切换至to。
/// 函数将“不会返回”，直到线程调度器将上下文切换回当前线程。
/// 在调用方的视角看来，函数不会做任何事。
/// 但仍需注意，切换线程后，调用方将失去CPU控制权，此时其他线程可能会破坏调用方正在操作的数据
///
/// Safety:
/// 调用方需保证目标上下文是可访问的，并且调用方所持有的对象不会影响其他线程的运行。
/// from和to是同一个线程的行为未定义，调用方有义务确保from和to不是同一线程
/// 调用时务必【关中断】，并在切换线程后的合适位置重新打开中断
///
/// 注意：
/// 输入的from指针和to指针均为Arc::into_raw的指针！
unsafe fn switch_thread(from: *const SpinLock<Thread>, to: *const SpinLock<Thread>, suspend: bool) {
    // 旧线程逻辑处理
    unsafe extern "C" fn deal_old_thread(
        ctx: *const Context,
        thread: *const SpinLock<Thread>,
        suspend: bool,
    ) {
        unsafe {
            let thread = Arc::from_raw(thread);
            let thread_id;
            let new_status = {
                let mut thread = thread.lock();
                thread_id = thread.thread_id;
                // 上下文
                thread.context = ptr::read(ctx);
                // 状态
                if matches!(thread.status, ThreadStatus::Terminating) {
                    thread.status = ThreadStatus::Terminated;
                } else if suspend {
                    thread.status = ThreadStatus::Suspend;
                } else {
                    thread.status = ThreadStatus::Ready;
                }
                thread.status
            };

            // 将旧线程放入指定队列，如果是IDLE线程，则不放入
            let is_idle_thread = thread_id == IDLE_THREAD_ID;
            if !is_idle_thread {
                match new_status {
                    ThreadStatus::Ready => READY_THREADS.lock().push_back(Arc::downgrade(&thread)),
                    ThreadStatus::Running => unreachable!(),
                    ThreadStatus::Suspend => (),
                    ThreadStatus::Terminating => unreachable!(),
                    ThreadStatus::Terminated => {
                        TERMINATED_THREADS.lock().push_back(Arc::downgrade(&thread))
                    }
                }
            }
        }
    }

    // 新线程逻辑处理
    unsafe extern "C" fn deal_new_thread(thread: *const SpinLock<Thread>) -> ! {
        unsafe {
            let thread = Arc::from_raw(thread);
            let mut lock = thread.lock();
            // 获取上下文信息
            let context = &raw const lock.context;
            // 将新线程设置为运行状态
            if matches!(lock.status, ThreadStatus::Ready) {
                lock.status = ThreadStatus::Running;
            }
            drop(lock);
            // 设置当前线程
            CURRENT_THREAD = Some(thread);
            // 切换上下文
            switch_to_context(context);
        }
    }

    #[unsafe(naked)]
    unsafe extern "C" fn switch_thread_internal() {
        naked_asm!(
            // 先拿到rip（返回地址）
            "mov rax, [rsp]",
            // 暂存数据
            "push rsp",
            "push rdi",
            "push rsi",
            // 压入上下文，调用其他函数进行复制，避免在汇编中手写偏移
            "mov rcx, [rsp+16]",
            "add rcx, 8",
            "push rcx", // rsp
            "push rax", // rip
            "push rbx",
            "push rbp",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "mov rdi, rsp",
            "mov rsi, [rsp+72]",
            // 栈对齐，将之前的栈保存到rbp
            "mov rbp, rsp",
            "and rsp, 0xfffffffffffffff0",
            "call {deal_old_thread}",
            // 进行线程切换，当前线程上下文已经写入
            "mov rdi, [rbp+64]",
            "jmp {deal_new_thread}",
            deal_old_thread = sym deal_old_thread,
            deal_new_thread = sym deal_new_thread,
        )
    }

    unsafe {
        asm!(
            "call {switch_thread_internal}",
            in("rdi") from,
            in("rsi") to,
            in("rdx") suspend as u64,
            switch_thread_internal = sym switch_thread_internal,
        )
    }

    // TODO: 是否应该在这里再次检查线程是否真的处于Running状态？
    // 比如，是否无意中唤醒了suspend状态或terminated状态的线程？

    // 切换线程后，我们检查是否有线程需要清理资源
    loop {
        let mut terminated = TERMINATED_THREADS.lock();
        if let Some(thread) = terminated.pop_front() {
            drop(terminated);
            if let Some(thread) = thread.upgrade() {
                destroy_thread(thread);
            }
        } else {
            break;
        }
    }
}

/// 销毁线程，同时销毁其关联资源
fn destroy_thread(thread: Arc<SpinLock<Thread>>) {
    let thread_id = thread.lock().thread_id;
    THREADS.lock().remove(&thread_id);
}

/// 让出当前线程，由其他线程获取CPU并执行
pub fn thread_yield(suspend: bool) {
    let current_thread = Arc::into_raw(current_thread().unwrap());
    let mut next_thread = {
        let _guard = unsafe { IrqGuard::cli() };
        loop {
            let mut queue = READY_THREADS.lock();
            let Some(thread) = queue.pop_front() else {
                break None;
            };
            drop(queue);
            if let Some(thread) = thread.upgrade() {
                let status = thread.lock().status;
                if matches!(status, ThreadStatus::Ready) {
                    break Some(thread);
                }
            }
        }
    };

    // 如果当前线程预期要被挂起，且已无线程可执行，则进入中断线程
    if suspend && next_thread.is_none() {
        next_thread = unsafe { (*&raw const IDLE_THREAD).clone() };
    }

    // 仅当需要切换时，进行线程切换
    if let Some(next_thread) = next_thread {
        let next_thread = Arc::into_raw(next_thread);
        unsafe {
            switch_thread(current_thread, next_thread, suspend);
        }
    }
}

/// 如果有ready状态的线程，则进行线程切换。此函数由IDLE线程调用
fn try_yield_thread() {
    let next_thread = {
        let _guard = unsafe { IrqGuard::cli() };
        loop {
            let mut queue = READY_THREADS.lock();
            let Some(thread) = queue.pop_front() else {
                break None;
            };
            drop(queue);
            if let Some(thread) = thread.upgrade() {
                let status = thread.lock().status;
                if matches!(status, ThreadStatus::Ready) {
                    break Some(thread);
                }
            }
        }
    };

    if let Some(thread) = next_thread {
        let current_thread = current_thread().unwrap();

        unsafe {
            switch_thread(Arc::into_raw(current_thread), Arc::into_raw(thread), false);
        }
    }
}
