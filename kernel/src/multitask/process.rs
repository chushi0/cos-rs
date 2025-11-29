use core::{
    arch::naked_asm,
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{
    collections::{btree_map::BTreeMap, btree_set::BTreeSet},
    sync::Arc,
};
use elf::ElfFile;
use filesystem::path::PathBuf;

use crate::{
    int, io,
    memory::{
        self,
        physics::{AccessMemoryError, AllocFrameHint},
    },
    multitask::{self, elf_loader::ElfLoader},
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

    let _guard = IrqGuard::cli();
    PROCESSES.lock().insert(process_id, process.clone());

    Some(process)
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
    process_id: u64,
    size: usize,
    page_type: ProcessPageType,
) -> Option<NonZeroU64> {
    let process = get_process(process_id)?;
    let _guard = IrqGuard::cli();
    let page_table = process.lock().page_table;
    let virtual_ptr = memory::physics::alloc_mapped_frame(
        size,
        match page_type {
            ProcessPageType::Code => AllocFrameHint::UserCode(page_table),
            ProcessPageType::Stack => AllocFrameHint::UserStack(page_table),
            ProcessPageType::Data => AllocFrameHint::UserHeap(page_table),
            ProcessPageType::StaticCode(vaddr) => AllocFrameHint::StaticUserCode(page_table, vaddr),
            ProcessPageType::StaticData(vaddr) => AllocFrameHint::StaticUserData(page_table, vaddr),
            ProcessPageType::StaticConst(vaddr) => {
                AllocFrameHint::StaticUserConst(page_table, vaddr)
            }
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
pub unsafe fn write_user_process_memory(
    process_id: u64,
    addr: u64,
    src: *const u8,
    len: usize,
) -> Result<(), ProcessMemoryError> {
    let Some(process) = get_process(process_id) else {
        return Err(ProcessMemoryError::ProcessNotFound);
    };
    let page_table = {
        let _guard = IrqGuard::cli();
        process.lock().page_table
    };
    unsafe {
        memory::physics::write_page_table_memory(page_table.get(), addr, src, len).map_err(|e| {
            match e {
                AccessMemoryError::PageFault => ProcessMemoryError::PageFault,
            }
        })
    }
}

pub unsafe fn write_user_process_memory_struct<T>(
    process_id: u64,
    addr: u64,
    src: &T,
) -> Result<(), ProcessMemoryError> {
    unsafe {
        write_user_process_memory(
            process_id,
            addr,
            src as *const T as *const u8,
            size_of::<T>(),
        )
    }
}

pub unsafe fn write_user_process_memory_bytes(
    process_id: u64,
    addr: u64,
    byte: u8,
    len: usize,
) -> Result<(), ProcessMemoryError> {
    let Some(process) = get_process(process_id) else {
        return Err(ProcessMemoryError::ProcessNotFound);
    };
    let page_table = {
        let _guard = IrqGuard::cli();
        process.lock().page_table
    };
    unsafe {
        memory::physics::write_page_table_memory_bytes(page_table.get(), addr, byte, len).map_err(
            |e| match e {
                AccessMemoryError::PageFault => ProcessMemoryError::PageFault,
            },
        )
    }
}

/// 从进程空间读取内存
pub unsafe fn read_user_process_memory(
    process_id: u64,
    addr: u64,
    dst: *mut u8,
    len: usize,
) -> Result<(), ProcessMemoryError> {
    let Some(process) = get_process(process_id) else {
        return Err(ProcessMemoryError::ProcessNotFound);
    };
    let page_table = {
        let _guard = IrqGuard::cli();
        process.lock().page_table
    };
    unsafe {
        memory::physics::read_page_table_memory(page_table.get(), addr, dst, len).map_err(|e| {
            match e {
                AccessMemoryError::PageFault => ProcessMemoryError::PageFault,
            }
        })
    }
}

pub unsafe fn read_user_process_memory_struct<T>(
    process_id: u64,
    addr: u64,
    dst: &mut T,
) -> Result<(), ProcessMemoryError> {
    unsafe { read_user_process_memory(process_id, addr, dst as *mut T as *mut u8, size_of::<T>()) }
}

/// 创建用户进程
///
/// 指定可执行文件路径，将加载指定可执行文件到用户空间，然后创建其主线程并运行代码
///
/// TODO: 需要优化失败路径的资源回收
pub async fn create_user_process(exe: &str) -> Option<u64> {
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
    let process_id = {
        let _guard = IrqGuard::cli();
        process.lock().process_id
    };

    // 加载程序段
    let Ok(mut elf) = ElfFile::from_io(file.as_mut()).await else {
        file.close().await.ok()?;
        return None;
    };
    let mut loader = ElfLoader { process_id };
    if elf.load(&mut loader).await.is_err() {
        file.close().await.ok()?;
        return None;
    }
    // 入口点
    let entry_point = elf.header().entry_point;
    file.close().await.ok()?;

    // 主线程栈
    let stack_page = create_process_page(process_id, 0x1000, ProcessPageType::Stack)?;

    // 写入启动地址
    // TODO: 应当写入内核页
    unsafe {
        if write_user_process_memory_struct(process_id, stack_page.get() + 0x1000 - 8, &entry_point)
            .is_err()
        {
            return None;
        }
    }

    // 创建线程
    unsafe {
        multitask::thread::create_thread(
            NonZeroU64::new(process_id),
            user_thread_entry as u64,
            stack_page.get() + 0x1000 - 8,
            false,
        );
    }

    Some(process_id)
}

// 用户线程入口点
#[unsafe(naked)]
extern "C" fn user_thread_entry() -> ! {
    extern "C" fn enter_user_mode(rip: u64, rsp: u64) -> ! {
        unsafe { int::idt::enter_user_mode(rip, rsp) }
    }
    naked_asm!(
        "mov rdi, [rsp]",
        "mov rsi, rsp",
        "jmp {enter_user_mode}",
        enter_user_mode = sym enter_user_mode,
    )
}
