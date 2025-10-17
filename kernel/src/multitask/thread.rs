use core::{
    arch::{asm, naked_asm},
    mem::MaybeUninit,
    ptr,
    sync::atomic::AtomicU64,
};

use alloc::{
    boxed::Box,
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::{Arc, Weak},
};

use crate::{kprintln, sync::SpinLock};

#[allow(unused)]
static THREADS: SpinLock<BTreeMap<u64, Arc<SpinLock<Thread>>>> = SpinLock::new(BTreeMap::new());
#[allow(unused)]
static READY_THREADS: SpinLock<VecDeque<Weak<SpinLock<Thread>>>> = SpinLock::new(VecDeque::new());
#[allow(unused)]
static THREAD_ID_GENERATOR: AtomicU64 = AtomicU64::new(0);

pub struct Thread {
    pub thread_id: u64,
    pub process_id: u64,
    pub context: Context,
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

pub enum ThreadStatus {
    Ready,
    Running,
    Suspend,
}

impl Context {
    const fn uninit() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
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
/// 调用方需保证目标上下文是可访问的，并且调用方所持有的对象不会影响其他线程的运行
unsafe extern "C" fn switch_thread(from: *mut Context, to: *const Context) {
    unsafe extern "C" fn copy_context(src: *const Context, dst: *mut Context) {
        unsafe {
            *dst = ptr::read(src);
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
            "call {copy_context}",
            // 进行线程切换，当前线程上下文已经写入
            "mov rdi, [rbp+64]",
            "jmp {switch_to_context}",
            copy_context = sym copy_context,
            switch_to_context = sym switch_to_context,
        )
    }

    unsafe {
        asm!(
            "call {switch_thread_internal}",
            in("rdi") from,
            in("rsi") to,
            switch_thread_internal = sym switch_thread_internal,
        )
    }
}

pub fn test_thread_switch() {
    static mut CTX1: Context = Context::uninit();
    static mut CTX2: Context = Context::uninit();
    let rsp_1 = Box::leak(Box::new([0u8; 4096])) as *mut u8 as usize as u64 + 4096 - 8;
    let rsp_2 = Box::leak(Box::new([0u8; 4096])) as *mut u8 as usize as u64 + 4096 - 8;
    unsafe {
        CTX1.rsp = rsp_1;
        CTX2.rsp = rsp_2;
        CTX1.rip = thread_a as u64;
        CTX2.rip = thread_b as u64;
    }

    kprintln!("ready to test switch");

    unsafe {
        switch_to_context(&raw const CTX1);
    }

    extern "C" fn thread_a() -> ! {
        loop {
            kprintln!("in thread a");
            unsafe {
                switch_thread(&raw mut CTX1, &raw const CTX2);
            }
        }
    }

    extern "C" fn thread_b() -> ! {
        loop {
            kprintln!("in thread b");
            unsafe {
                switch_thread(&raw mut CTX2, &raw const CTX1);
            }
        }
    }
}
