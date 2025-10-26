use core::{
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{
    collections::{btree_map::BTreeMap, btree_set::BTreeSet},
    sync::Arc,
};

use crate::{
    memory::{
        self,
        physics::{AccessMemoryError, AllocFrameHint},
    },
    sync::{int::IrqGuard, spin::SpinLock},
};

static PROCESSES: SpinLock<BTreeMap<u64, Arc<SpinLock<Process>>>> = SpinLock::new(BTreeMap::new());
static PROCESS_ID_GENERATOR: AtomicU64 = AtomicU64::new(0);

// 用户进程
pub struct Process {
    // 进程ID
    pub process_id: u64,
    // 线程ID
    pub thread_ids: BTreeSet<u64>,
    // 页表地址
    pub page_table: NonZeroU64,
}

/// 创建进程
pub fn create_process() -> Option<Arc<SpinLock<Process>>> {
    // 需要申请一页内存用作四级页表
    let page_table = memory::physics::alloc_user_page_table()?;

    let process_id = PROCESS_ID_GENERATOR.fetch_add(1, Ordering::SeqCst) + 1;
    let process = Process {
        process_id,
        thread_ids: BTreeSet::new(),
        page_table,
    };
    let process = Arc::new(SpinLock::new(process));

    let _guard = unsafe { IrqGuard::cli() };
    PROCESSES.lock().insert(process_id, process.clone());

    Some(process)
}

/// 根据id获取进程
pub fn get_process(id: u64) -> Option<Arc<SpinLock<Process>>> {
    let _guard = unsafe { IrqGuard::cli() };
    PROCESSES.lock().get(&id).cloned()
}

pub enum ProcessPageType {
    Code,
    Stack,
    Data,
}

/// 为进程分配页
///
/// size 必须对齐4K
pub fn create_process_page(
    process_id: u64,
    size: usize,
    page_type: ProcessPageType,
) -> Option<NonZeroU64> {
    let process = get_process(process_id)?;
    let _guard = unsafe { IrqGuard::cli() };
    let page_table = process.lock().page_table;
    let virtual_ptr = memory::physics::alloc_mapped_frame(
        size,
        match page_type {
            ProcessPageType::Code => AllocFrameHint::UserCode(page_table),
            ProcessPageType::Stack => AllocFrameHint::UserStack(page_table),
            ProcessPageType::Data => AllocFrameHint::UserHeap(page_table),
        },
    )?;

    virtual_ptr.addr().try_into().ok()
}

pub enum ProcessMemoryError {
    /// 进程不存在
    ProcessNotFound,
    /// 页表中不存在此虚拟地址的映射
    PageFault,
}

/// 向进程空间写入内存
pub unsafe fn write_user_process_memory<T>(
    process_id: u64,
    addr: u64,
    src: &T,
) -> Result<(), ProcessMemoryError> {
    let Some(process) = get_process(process_id) else {
        return Err(ProcessMemoryError::ProcessNotFound);
    };
    let page_table = {
        let _guard = unsafe { IrqGuard::cli() };
        process.lock().page_table
    };
    unsafe {
        memory::physics::write_page_table_memory(page_table.get(), addr, src).map_err(|e| match e {
            AccessMemoryError::PageFault => ProcessMemoryError::PageFault,
        })
    }
}

/// 从进程空间读取内存
pub unsafe fn read_user_process_memory<T>(
    process_id: u64,
    addr: u64,
    dst: &mut T,
) -> Result<(), ProcessMemoryError> {
    let Some(process) = get_process(process_id) else {
        return Err(ProcessMemoryError::ProcessNotFound);
    };
    let page_table = {
        let _guard = unsafe { IrqGuard::cli() };
        process.lock().page_table
    };
    unsafe {
        memory::physics::read_page_table_memory(page_table.get(), addr, dst).map_err(|e| match e {
            AccessMemoryError::PageFault => ProcessMemoryError::PageFault,
        })
    }
}
