use core::{
    arch::{asm, naked_asm},
    mem::MaybeUninit,
    num::NonZeroU64,
    ptr::{self, null_mut},
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{
    boxed::Box,
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::{Arc, Weak},
};

use crate::{
    int,
    memory::{self, physics::AllocateFrameOptions},
    multitask,
    sync::{
        self,
        int::{IrqGuard, sti},
        spin::SpinLock,
    },
};

static THREADS: SpinLock<BTreeMap<u64, Arc<SpinLock<Thread>>>> = SpinLock::new(BTreeMap::new());
static READY_THREADS: SpinLock<VecDeque<Weak<SpinLock<Thread>>>> = SpinLock::new(VecDeque::new());
static TERMINATED_THREADS: SpinLock<VecDeque<Weak<SpinLock<Thread>>>> =
    SpinLock::new(VecDeque::new());
static THREAD_ID_GENERATOR: AtomicU64 = AtomicU64::new(0);

// RSP0栈大小（8K）
const RSP0_PAGE_COUNT: usize = 2;
const RSP0_SIZE: usize = 0x1000 * RSP0_PAGE_COUNT;

// 内核线程或用户线程
pub struct Thread {
    // 线程ID
    pub thread_id: u64,
    // 进程ID，None表示内核进程
    pub process_id: Option<NonZeroU64>,
    // 上下文，如果当前为Running状态，则此值未定义
    pub context: Context,
    // 线程状态
    pub status: ThreadStatus,
    // rsp0 进入内核切换栈地址（低地址）
    pub rsp0: Option<NonZeroU64>,
}

impl Drop for Thread {
    fn drop(&mut self) {
        let thread_id = self.thread_id;
        let rsp0 = self.rsp0.take();
        let process_id = self.process_id.take();
        multitask::async_rt::spawn(async move {
            if let Some(rsp0) = rsp0 {
                unsafe {
                    memory::physics::free_mapped_frame(
                        memory::physics::kernel_pml4(),
                        rsp0.get() as usize,
                        RSP0_SIZE,
                    );
                }
            }

            if let Some(process_id) = process_id {
                if let Some(process) = multitask::process::get_process(process_id.get()) {
                    let _guard = IrqGuard::cli();
                    let mut process = process.lock();
                    process.thread_ids.remove(&thread_id);
                    if process.thread_ids.is_empty() {
                        multitask::process::stop_process(process_id.get());
                    }
                }
            }
        });
    }
}

#[repr(C)]
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
    let rsp0 = if process_id.is_some() {
        let addr = unsafe {
            memory::physics::alloc_mapped_frame(
                memory::physics::kernel_pml4(),
                RSP0_SIZE,
                AllocateFrameOptions::KERNEL_DATA,
            )
            .ok()?
        };
        Some(addr.addr().try_into().expect("usize is u64"))
    } else {
        None
    };
    let thread = Thread {
        thread_id,
        process_id,
        context,
        status: if initial_suspend {
            ThreadStatus::Suspend
        } else {
            ThreadStatus::Ready
        },
        rsp0,
    };
    let thread = Arc::new(SpinLock::new(thread));

    let _guard = IrqGuard::cli();

    THREADS.lock().insert(thread_id, thread.clone());
    if !initial_suspend {
        READY_THREADS.lock().push_back(Arc::downgrade(&thread));
    }

    if let Some(process_id) = process_id {
        let process = multitask::process::get_process(process_id.get());
        if let Some(process) = process {
            process.lock().thread_ids.insert(thread_id);
        }
    }

    Some(thread)
}

/// 创建IDLE线程
pub fn create_idle_thread() {
    extern "C" fn idle_thread_entry() -> ! {
        loop {
            sti();
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
        rsp0: None,
    };
    let thread = Arc::new(SpinLock::new(thread));

    let _guard = IrqGuard::cli();
    THREADS.lock().insert(thread_id, thread.clone());
    sync::percpu::set_idle_thread_id(thread_id);
}

/// 将当前CPU执行线程创建为内核异步线程
///
/// 此函数将当前线程封装为一个内核线程对象，并加入到全局队列中
/// 线程创建后即自动挂载为当前线程，并立即为运行状态
pub fn create_kernel_async_thread() {
    let thread_id = THREAD_ID_GENERATOR.fetch_add(1, Ordering::SeqCst) + 1;
    let context = Context::uninit();
    let thread = Thread {
        thread_id,
        process_id: None,
        context,
        status: ThreadStatus::Running,
        rsp0: None,
    };
    let thread = Arc::new(SpinLock::new(thread));

    let _guard = IrqGuard::cli();
    THREADS.lock().insert(thread_id, thread.clone());
    sync::percpu::set_current_thread_id(thread_id);
    sync::percpu::set_kernel_async_thread_id(thread_id);
}

/// 唤醒内核线程，用于内核异步运行时
pub fn wake_kernel_thread() {
    let thread_id = sync::percpu::get_kernel_async_thread_id();

    let Some(thread) = ({
        let _guard = IrqGuard::cli();
        THREADS.lock().get(&thread_id).cloned()
    }) else {
        return;
    };

    wake_thread(&thread);
}

pub fn wake_thread(thread: &Arc<SpinLock<Thread>>) {
    let _guard = IrqGuard::cli();
    let mut thread_lock = thread.lock();
    let status = thread_lock.status;
    if matches!(status, ThreadStatus::Suspend) {
        thread_lock.status = ThreadStatus::Ready;
        drop(thread_lock);
        READY_THREADS.lock().push_back(Arc::downgrade(thread));
    }
}

/// 获取当前正在执行的线程
pub fn current_thread() -> Option<Arc<SpinLock<Thread>>> {
    let thread_id = sync::percpu::get_current_thread_id();
    let _guard = IrqGuard::cli();
    THREADS.lock().get(&thread_id).cloned()
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
unsafe fn switch_thread(
    from: *const SpinLock<Thread>,
    to: *const SpinLock<Thread>,
    suspend: bool,
    on_yield: Yield,
) {
    // 旧线程逻辑处理
    unsafe extern "C" fn deal_old_thread(
        ctx: *const Context,
        thread: *const SpinLock<Thread>,
        suspend: bool,
        on_yield: *const Yield,
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
            let is_idle_thread = thread_id == sync::percpu::get_idle_thread_id();
            if !is_idle_thread {
                match new_status {
                    ThreadStatus::Ready => READY_THREADS.lock().push_back(Arc::downgrade(&thread)),
                    ThreadStatus::Running => unreachable!(),
                    ThreadStatus::Suspend => (),
                    ThreadStatus::Terminating => unreachable!(),
                    ThreadStatus::Terminated => {
                        let mut terminated_thread = TERMINATED_THREADS.lock();
                        // TODO: 当内存不足时，考虑如何回收线程资源
                        if terminated_thread.try_reserve(1).is_ok() {
                            terminated_thread.push_back(Arc::downgrade(&thread));
                        }
                    }
                }
            }

            (*on_yield).call()
        }
    }

    // 新线程逻辑处理
    unsafe extern "C" fn deal_new_thread(thread: *const SpinLock<Thread>) -> ! {
        unsafe {
            let thread = Arc::from_raw(thread);
            let mut lock = thread.lock();
            let thread_id = lock.thread_id;
            // 获取上下文信息
            let context = &raw const lock.context;
            // 将新线程设置为运行状态
            if matches!(lock.status, ThreadStatus::Ready) {
                lock.status = ThreadStatus::Running;
            }
            // 切换栈
            let rsp0 = lock.rsp0;
            // 所属进程
            let process_id = lock.process_id;
            drop(lock);
            drop(thread);
            // 设置当前线程
            sync::percpu::set_current_thread_id(thread_id);
            // 设置切换栈
            let addr = if let Some(rsp0) = rsp0 {
                rsp0.get() + RSP0_SIZE as u64
            } else {
                0
            };
            int::tss::set_rsp0(addr);
            sync::percpu::set_syscall_stack(addr);
            // 设置页表
            let pml4 = if let Some(process_id) = process_id
                && let Some(process) = multitask::process::get_process(process_id.get())
            {
                process.lock().page_table.get()
            } else {
                memory::physics::kernel_pml4()
            };
            asm!(
                "mov cr3, {}",
                in(reg) pml4,
                options(nostack, preserves_flags)
            );
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
            "mov r11, [rsp+16]",
            "add r11, 8",
            "push r11", // rsp
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
            "sub rsp, 8",
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
            in("rcx") &raw const on_yield,
            switch_thread_internal = sym switch_thread_internal,
            lateout("rax") _,
            lateout("rcx") _,
            lateout("rdx") _,
            lateout("rsi") _,
            lateout("rdi") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("r11") _,
        )
    }

    // 是否应该在这里再次检查线程是否真的处于Running状态？
    // 比如，是否无意中唤醒了suspend状态或terminated状态的线程？
    // 结论：不需要。
    // 如果在单CPU上，由于我们已经关中断，此时其他线程无法挂起当前线程，所以当前线程必然处于READY状态
    // 如果在多CPU上，我们必然通过IPI中断提醒当前CPU此线程已经结束，我们会在关中断时立刻收到中断，此时再结束线程即可

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

/// 在让出线程后，再执行此函数
/// 如果线程未让出，则不会执行此函数
///
/// 此结构体便于实现类似条件变量的逻辑。条件变量需要挂起当前线程，但挂起同时释放线程。
/// 此时可能会出现这种代码：
///
/// ```rust, norun
/// struct CondVar<'a, T> {
///     mutex: &'a, Mutex<T>,
///     queue: SpinLock<Vec<u64>>,
/// }
///
/// impl<'a, T> CondVar<'a, T> {
///     fn wait(&self, guard: MutexGuard<'_, T>) -> MutexGuard<'a, T> {
///         drop(guard);
///         queue.lock().push(current_thread());
///         thread_yield(true); // <-- NOTICE HERE
///         self.mutex().lock()
///     }
/// }
/// ```
///
/// 注意代码中标记的位置。在并发时，其他线程可能在`thread_yield(true)`这行代码之前，抢先把队列中的线程id移出，
/// 然后唤醒当前线程，但此时当前线程仍然为READY状态。而后当前线程再执行`thread_yield(true)`，那么当前线程将被挂起，
/// 且永远不会再被唤醒。
///
/// 借助[Yield]结构体，你可以改写为以下代码：
///
/// ```rust, norun
/// struct CondVar<'a, T> {
///     mutex: &'a, Mutex<T>,
///     queue: SpinLock<Vec<u64>>,
/// }
///
/// impl<'a, T> CondVar<'a, T> {
///     fn wait(&self, guard: MutexGuard<'_, T>) -> MutexGuard<'a, T> {
///         struct YieldContext<'b, T> {
///             guard: MutexGuard<'b, T>,
///             thread: u64,
///         }
///
///         let mut context = Some(YieldContext { guard, thread: current_thread() });
///         unsafe fn on_yield(context: *mut ()) {
///             unsafe {
///                 let context = context as *mut Option<YieldContext>;
///                 if let Some(context) = (*context).take() {
///                     queue.lock().push(context.thread);
///                 }
///             }
///         }
///
///         queue.lock().push(current_thread());
///         thread_yield_with(Yield { context: &raw mut context as *mut (), vtable: on_yield });
///         self.mutex().lock()
///     }
/// }
/// ```
///
/// 这样修改后，代码便有了以下保证：
/// 1. vtable函数将被在当前线程彻底换出后（线程被标记为suspend后）才会执行，在此之前，其他线程不会持锁
/// 2. 当其他线程调用notify时，如果发现队列中有排队的线程，那么这个线程不会处于READY状态，可以将其唤醒
/// 3. 即便在vtable返回之前，其他线程获取了锁并唤醒，也不会有问题，因为当前线程会被正常唤醒为READY
pub struct Yield {
    pub context: *mut (),
    pub vtable: unsafe fn(context: *mut ()),
}

impl Yield {
    const fn empty() -> Self {
        unsafe fn empty(_context: *mut ()) {}
        Yield {
            context: null_mut(),
            vtable: empty,
        }
    }

    unsafe fn call(&self) {
        unsafe {
            (self.vtable)(self.context);
        }
    }
}

/// 让出当前线程，由其他线程获取CPU并执行
pub fn thread_yield(suspend: bool) {
    thread_yield_internal(suspend, Yield::empty());
}

/// 让出当前线程，由其他线程获取CPU并执行
/// 参考 [Yield]
pub fn thread_yield_with(on_yield: Yield) {
    thread_yield_internal(true, on_yield);
}

fn thread_yield_internal(suspend: bool, on_yield: Yield) {
    let current_thread = current_thread().unwrap();
    let mut next_thread = {
        let _guard = IrqGuard::cli();
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
        let idle_thread_id = sync::percpu::get_idle_thread_id();
        let _guard = IrqGuard::cli();
        next_thread = THREADS.lock().get(&idle_thread_id).cloned();
    }

    // 仅当需要切换时，进行线程切换
    if let Some(next_thread) = next_thread {
        let current_thread = Arc::into_raw(current_thread);
        let next_thread = Arc::into_raw(next_thread);
        // 我们在此处关闭中断，确保切换线程时不会被中断打断
        // 线程返回后，会重新开启中断
        let _guard = IrqGuard::cli();
        unsafe {
            switch_thread(current_thread, next_thread, suspend, on_yield);
        }
    }
}

/// 如果有ready状态的线程，则进行线程切换。此函数由IDLE线程调用
fn try_yield_thread() {
    let next_thread = {
        let _guard = IrqGuard::cli();
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

        let _guard = IrqGuard::cli();
        unsafe {
            switch_thread(
                Arc::into_raw(current_thread),
                Arc::into_raw(thread),
                false,
                Yield::empty(),
            );
        }
    }
}

pub fn stop_thread(thread: &SpinLock<Thread>) {
    let mut thread = thread.lock();
    thread.status = ThreadStatus::Terminating;
    // TODO: 多CPU - 如果线程仍处于Running状态，通过中断提醒对方结束
}

pub fn get_thread(thread_id: u64) -> Option<Arc<SpinLock<Thread>>> {
    THREADS.lock().get(&thread_id).cloned()
}
