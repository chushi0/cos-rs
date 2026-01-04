use core::{
    arch::naked_asm,
    mem::MaybeUninit,
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{
    collections::{btree_map::BTreeMap, btree_set::BTreeSet, vec_deque::VecDeque},
    sync::Arc,
    vec::Vec,
};
use async_locks::{channel::oneshot, watch};
use elf::ElfFile;
use filesystem::path::PathBuf;

use crate::{
    io,
    memory::{
        self,
        page::{AccessMemoryError, AllocateFrameOptions},
    },
    multitask::{
        self,
        elf_loader::ElfLoader,
        thread::{RSP0_SIZE, Thread},
    },
    sync::{int::IrqGuard, spin::SpinLock},
    trap,
    user::handle::HandleObject,
};

static PROCESSES: SpinLock<BTreeMap<u64, Arc<SpinLock<Process>>>> = SpinLock::new(BTreeMap::new());
static PROCESS_ID_GENERATOR: AtomicU64 = AtomicU64::new(0);

// 用户进程
pub struct Process {
    // 进程ID
    pub(super) process_id: u64,
    // 线程ID
    pub(super) thread_ids: BTreeSet<u64>,
    // 页表地址
    pub(super) page_table: NonZeroU64,
    // 退出码
    exit_code: watch::Publisher<u64>,
    // 退出码是否已经设置
    exit_code_setted: bool,
    // 为其他进程wait预留
    exit_code_sub: watch::Subscriber<u64>,
    // 句柄
    handles: Vec<Option<Arc<HandleObject>>>,
    // 等待中的线程 (通过wait/wake syscall)
    futex: BTreeMap<u64, VecDeque<oneshot::Sender<()>>>,
}

impl Drop for Process {
    fn drop(&mut self) {
        // 释放页表
        unsafe {
            memory::page::release_user_page_table(self.page_table);
        }
    }
}

/// 创建进程
fn create_process() -> Option<Arc<SpinLock<Process>>> {
    // 需要申请一页内存用作四级页表
    let page_table = memory::page::alloc_user_page_table()?;

    let process_id = PROCESS_ID_GENERATOR.fetch_add(1, Ordering::SeqCst) + 1;
    let (publisher, subscriber) = watch::pair(0);
    let process = Process {
        process_id,
        thread_ids: BTreeSet::new(),
        page_table,
        exit_code: publisher,
        exit_code_setted: false,
        exit_code_sub: subscriber,
        handles: Vec::new(),
        futex: BTreeMap::new(),
    };
    let process = Arc::new(SpinLock::new(process));

    let _guard = IrqGuard::cli();
    PROCESSES.lock().insert(process_id, process.clone());

    Some(process)
}

pub fn current_process() -> Option<Arc<SpinLock<Process>>> {
    let thread = multitask::thread::current_thread()?;
    let process_id = {
        let _guard = IrqGuard::cli();
        thread.lock().process_id
    }?;
    get_process(process_id.get())
}

/// 根据id获取进程
pub fn get_process(id: u64) -> Option<Arc<SpinLock<Process>>> {
    let _guard = IrqGuard::cli();
    PROCESSES.lock().get(&id).cloned()
}

pub enum ProcessPageType {
    Code,
    Stack,
    Data,
    StaticCode(NonZeroU64),
    StaticData(NonZeroU64),
    StaticConst(NonZeroU64),
}

/// 为进程分配页
///
/// size 必须对齐4K
pub fn create_process_page(
    process: &SpinLock<Process>,
    size: usize,
    page_type: ProcessPageType,
) -> Option<NonZeroU64> {
    let _guard = IrqGuard::cli();
    let page_table = process.lock().page_table;

    let options = match page_type {
        ProcessPageType::Code => AllocateFrameOptions::USER_CODE,
        ProcessPageType::Stack => AllocateFrameOptions::USER_DATA,
        ProcessPageType::Data => AllocateFrameOptions::USER_DATA,
        ProcessPageType::StaticCode(vaddr) => {
            AllocateFrameOptions::USER_CODE.with_static_vaddr(vaddr)
        }
        ProcessPageType::StaticData(vaddr) => {
            AllocateFrameOptions::USER_DATA.with_static_vaddr(vaddr)
        }
        ProcessPageType::StaticConst(vaddr) => {
            AllocateFrameOptions::USER_DATA.with_static_vaddr(vaddr)
        }
    };

    let virtual_ptr = unsafe { memory::page::alloc_mapped_frame(page_table.get(), size, options) };

    virtual_ptr.ok()?.addr().try_into().ok()
}

/// 释放进程内存页
pub unsafe fn free_process_page(process: &SpinLock<Process>, addr: usize, size: usize) {
    let _guard = IrqGuard::cli();
    let page_table = process.lock().page_table;

    unsafe {
        memory::page::free_mapped_frame(page_table.get(), addr, size);
    }
}

#[derive(Debug)]
pub enum ProcessMemoryError {
    /// 进程不存在
    ProcessNotFound,
    /// 页表中不存在此虚拟地址的映射
    PageFault,
}

/// 向进程空间写入内存
pub unsafe fn write_user_process_memory(
    process: &SpinLock<Process>,
    addr: u64,
    src: *const u8,
    len: usize,
) -> Result<(), ProcessMemoryError> {
    let page_table = {
        let _guard = IrqGuard::cli();
        process.lock().page_table
    };
    unsafe {
        memory::page::write_page_table_memory(page_table.get(), addr, src, len)
            .map_err(|e| match e {
                AccessMemoryError::PageFault => ProcessMemoryError::PageFault,
            })
    }
}

pub unsafe fn write_user_process_memory_struct<T>(
    process: &SpinLock<Process>,
    addr: u64,
    src: &T,
) -> Result<(), ProcessMemoryError> {
    unsafe {
        write_user_process_memory(process, addr, src as *const T as *const u8, size_of::<T>())
    }
}

pub unsafe fn write_user_process_memory_bytes(
    process: &SpinLock<Process>,
    addr: u64,
    byte: u8,
    len: usize,
) -> Result<(), ProcessMemoryError> {
    let page_table = {
        let _guard = IrqGuard::cli();
        process.lock().page_table
    };
    unsafe {
        memory::page::write_page_table_memory_bytes(page_table.get(), addr, byte, len).map_err(
            |e| match e {
                AccessMemoryError::PageFault => ProcessMemoryError::PageFault,
            },
        )
    }
}

/// 从进程空间读取内存
pub unsafe fn read_user_process_memory(
    process: &SpinLock<Process>,
    addr: u64,
    dst: *mut u8,
    len: usize,
) -> Result<(), ProcessMemoryError> {
    let page_table = {
        let _guard = IrqGuard::cli();
        process.lock().page_table
    };
    unsafe {
        memory::page::read_page_table_memory(page_table.get(), addr, dst, len).map_err(
            |e| match e {
                AccessMemoryError::PageFault => ProcessMemoryError::PageFault,
            },
        )
    }
}

pub unsafe fn read_user_process_memory_struct<T>(
    process: &SpinLock<Process>,
    addr: u64,
    dst: &mut T,
) -> Result<(), ProcessMemoryError> {
    unsafe { read_user_process_memory(process, addr, dst as *mut T as *mut u8, size_of::<T>()) }
}

/// 创建用户进程
///
/// 指定可执行文件路径，将加载指定可执行文件到用户空间，然后创建其主线程并运行代码
///
/// TODO: 需要优化失败路径的资源回收
pub async fn create_user_process(exe: &str) -> Option<Arc<SpinLock<Process>>> {
    // 打开可执行文件
    let path = PathBuf::from_str(exe).ok()?;
    let fs = {
        let _guard = IrqGuard::cli();
        io::disk::FILE_SYSTEMS.lock().get(&0).cloned()
    }?;
    let mut file = fs.open_file(path.as_path()).await.ok()?;

    // 创建进程
    let Some(process) = create_process() else {
        file.close().await.ok()?;
        return None;
    };

    // 加载程序段
    let Ok(mut elf) = ElfFile::from_io(file.as_mut()).await else {
        file.close().await.ok()?;
        return None;
    };
    let mut loader = ElfLoader::new(&process);
    if elf.load(&mut loader).await.is_err() {
        file.close().await.ok()?;
        return None;
    }
    // 入口点
    let entry_point = elf.header().entry_point;
    file.close().await.ok()?;

    // 主线程用户态栈
    let stack_page = create_process_page(&process, 0x1000, ProcessPageType::Stack)?;

    // 主线程内核陷入栈
    let rsp0 = unsafe {
        let _guard = IrqGuard::cli();
        memory::page::alloc_mapped_frame(
            memory::page::kernel_pml4(),
            RSP0_SIZE,
            AllocateFrameOptions::KERNEL_DATA,
        )
    }
    .ok()?;
    let rsp0 = rsp0.as_ptr() as usize;

    // 写入启动地址、栈地址
    unsafe {
        *((rsp0 + RSP0_SIZE - 8) as *mut u64) = entry_point;
        *((rsp0 + RSP0_SIZE - 8 - 8) as *mut u64) = stack_page.get() + 0x1000 - 8;
        *((rsp0 + RSP0_SIZE - 8 - 16) as *mut u64) = 0;
    }

    // 创建线程
    let _guard = IrqGuard::cli();
    unsafe {
        multitask::thread::create_thread(
            Some(&mut *process.lock()),
            user_thread_entry as u64,
            rsp0 as u64 + 0x1000 - 8 - 16,
            rsp0 as u64,
            false,
        );
    }

    Some(process)
}

pub fn create_user_thread(
    process: &SpinLock<Process>,
    rip: u64,
    rsp: u64,
    params: u64,
) -> Option<Arc<SpinLock<Thread>>> {
    // 主线程内核陷入栈
    let rsp0 = unsafe {
        let _guard = IrqGuard::cli();
        memory::page::alloc_mapped_frame(
            memory::page::kernel_pml4(),
            RSP0_SIZE,
            AllocateFrameOptions::KERNEL_DATA,
        )
    }
    .ok()?;
    let rsp0 = rsp0.as_ptr() as usize;

    // 写入启动地址、栈地址
    unsafe {
        *((rsp0 + RSP0_SIZE - 8) as *mut u64) = rip;
        *((rsp0 + RSP0_SIZE - 8 - 8) as *mut u64) = rsp;
        *((rsp0 + RSP0_SIZE - 8 - 16) as *mut u64) = params;
    }

    // 创建线程
    let _guard = IrqGuard::cli();
    unsafe {
        multitask::thread::create_thread(
            Some(&mut *process.lock()),
            user_thread_entry as u64,
            rsp0 as u64 + 0x1000 - 8 - 16,
            rsp0 as u64,
            false,
        );
    }

    None
}

// 用户线程入口点
#[unsafe(naked)]
extern "C" fn user_thread_entry() {
    extern "C" fn enter_user_mode(rip: u64, rsp: u64) -> ! {
        unsafe { trap::idt::enter_user_mode(rip, rsp, 0) }
    }
    naked_asm!(
        "mov rdi, [rsp+16]",
        "mov rsi, [rsp+8]",
        "mov rdx, [rsp]",
        "jmp {enter_user_mode}",
        enter_user_mode = sym enter_user_mode,
    )
}

pub fn set_exit_code(process: &SpinLock<Process>, exit_code: u64) {
    set_exit_code_with_lock(&mut *process.lock(), exit_code);
}

pub fn set_exit_code_with_lock(process: &mut Process, exit_code: u64) {
    if process.exit_code_setted {
        return;
    }
    process.exit_code.send(exit_code);
    process.exit_code_setted = true;
}

pub fn stop_all_thread(process: &SpinLock<Process>, exit_code: u64) {
    let process = process.lock();
    for thread_id in &process.thread_ids {
        if let Some(thread) = multitask::thread::get_thread(*thread_id) {
            multitask::thread::stop_thread(&thread, exit_code);
        }
    }
}

pub(super) fn stop_process(process_id: u64) {
    let _guard = IrqGuard::cli();
    PROCESSES.lock().remove(&process_id);
}

pub fn get_exit_code_subscriber(process: &SpinLock<Process>) -> watch::Subscriber<u64> {
    process.lock().exit_code_sub.clone()
}

pub fn insert_process_handle(process: &SpinLock<Process>, handle: HandleObject) -> usize {
    let mut process = process.lock();

    for (i, slot) in process.handles.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(Arc::new(handle));
            return i;
        }
    }

    process.handles.push(Some(Arc::new(handle)));
    process.handles.len() - 1
}

pub fn get_process_handle(process: &SpinLock<Process>, index: usize) -> Option<Arc<HandleObject>> {
    process.lock().handles.get(index).cloned().flatten()
}

pub fn remove_process_handle(process: &SpinLock<Process>, index: usize) {
    if let Some(slot) = process.lock().handles.get_mut(index) {
        *slot = None;
    }
}

pub fn register_futex_if_match(
    process: &SpinLock<Process>,
    addr: u64,
    expected: u64,
    sender: oneshot::Sender<()>,
) -> Result<(), ProcessMemoryError> {
    let _guard = IrqGuard::cli();
    let mut process = process.lock();

    let addr_value = unsafe {
        let mut addr_value = MaybeUninit::<u64>::uninit();
        memory::page::read_page_table_memory(
            process.page_table.get(),
            addr,
            &raw mut addr_value as *mut u8,
            size_of::<u64>(),
        )
        .map_err(|e| match e {
            AccessMemoryError::PageFault => ProcessMemoryError::PageFault,
        })?;
        addr_value.assume_init()
    };

    if addr_value == expected {
        process.futex.entry(addr).or_default().push_back(sender);
    }

    Ok(())
}

pub fn wake_futex(process: &SpinLock<Process>, addr: u64, count: u64) {
    let _guard = IrqGuard::cli();
    let mut process = process.lock();

    let Some(queue) = process.futex.get_mut(&addr) else {
        return;
    };

    for _ in 0..count {
        if let Some(sender) = queue.pop_front() {
            drop(sender); // 接受者仍然会被唤醒并收到SenderLost
            continue;
        }
        break;
    }

    if queue.is_empty() {
        process.futex.remove(&addr);
    }
}
