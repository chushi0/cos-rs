use core::{
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{
    collections::{btree_map::BTreeMap, btree_set::BTreeSet},
    sync::Arc,
};

use crate::{
    memory,
    sync::{IrqGuard, SpinLock},
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
