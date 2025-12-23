/// 用于多任务，进程或线程退出
/// 
/// 子编号：
/// [IDX_SUB_EXIT_PROCESS]
/// [IDX_SUB_EXIT_THREAD]
pub const IDX_EXIT: u64 = 0x10001;
/// 退出当前进程
/// 
/// 系统调用编号为 [IDX_EXIT].
/// 函数封装为 [crate::multitask::exit]
pub const IDX_SUB_EXIT_PROCESS: u64 = 0x100001;
/// 退出当前线程
/// 
/// 系统调用编号为 [IDX_EXIT].
/// 函数封装为 [crate::multitask::exit_thread]
pub const IDX_SUB_EXIT_THREAD: u64 = 0x100002;

/// 多任务、线程相关系统调用
/// 
/// 子编号：
/// [IDX_SUB_THREAD_CURRENT]
/// [IDX_SUB_THREAD_SUSPEND]
/// [IDX_SUB_THREAD_RESUME]
/// [IDX_SUB_THREAD_KILL]
/// [IDX_SUB_THREAD_CREATE]
pub const IDX_THREAD: u64 = 0x10002;
/// 获取当前线程
/// 
/// 系统调用编号为 [IDX_THREAD].
/// 函数封装为 [crate::multitask::current_thread]
pub const IDX_SUB_THREAD_CURRENT: u64 = 0x100001;
/// 挂起线程
/// 
/// 系统调用编号为 [IDX_THREAD].
/// 函数封装为 [crate::multitask::suspend_thread]
pub const IDX_SUB_THREAD_SUSPEND: u64 = 0x100002;
/// 恢复线程
/// 
/// 系统调用编号为 [IDX_THREAD].
/// 函数封装为 [crate::multitask::resume_thread]
pub const IDX_SUB_THREAD_RESUME: u64 = 0x100003;
/// 停止线程
/// 
/// 系统调用编号为 [IDX_THREAD].
/// 函数封装为 [crate::multitask::kill_thread]
pub const IDX_SUB_THREAD_KILL: u64 = 0x100004;
/// 创建线程
/// 
/// 系统调用编号为 [IDX_THREAD].
/// 函数封装为 [crate::multitask::create_thread]
pub const IDX_SUB_THREAD_CREATE: u64 = 0x100005;

/// 进程内存相关系统调用
/// 
/// 子编号：
/// [IDX_SUB_MEMORY_ALLOC]
/// [IDX_SUB_MEMORY_FREE]
/// [IDX_SUB_MEMORY_TEST]
/// [IDX_SUB_MEMORY_RO]
/// [IDX_SUB_MEMORY_RW]
/// [IDX_SUB_MEMORY_RX]
pub const IDX_MEMORY: u64 = 0x10003;
/// 申请内存页，内存页默认为可读写不可执行
/// 
/// 系统调用编号为 [IDX_MEMORY].
/// 函数封装为 [crate::memory::alloc_page]
pub const IDX_SUB_MEMORY_ALLOC: u64 = 0x100001;
/// 释放内存页
/// 
/// 系统调用编号为 [IDX_MEMORY].
/// 函数封装为 [crate::memory::free_page]
pub const IDX_SUB_MEMORY_FREE: u64 = 0x100002;
/// 测试内存页
/// 
/// 系统调用编号为 [IDX_MEMORY].
/// 函数封装为 [crate::memory::test_page]
pub const IDX_SUB_MEMORY_TEST: u64 = 0x100003;
/// 修改内存页权限
/// 
/// 系统调用编号为 [IDX_MEMORY].
/// 函数封装为 [crate::memory::modify_page]
pub const IDX_SUB_MEMORY_MODIFY: u64 = 0x100004;

/// 与进程管理相关的系统调用
/// 
/// 子编号：
/// [IDX_SUB_PROCESS_CURRENT]
/// [IDX_SUB_PROCESS_CREATE]
/// [IDX_SUB_PROCESS_KILL]
pub const IDX_PROCESS: u64 = 0x10004;
/// 获取当前进程
/// 
/// 系统调用编号为 [IDX_PROCESS]
/// 函数封装为 [crate::multitask::current_process]
pub const IDX_SUB_PROCESS_CURRENT: u64 = 0x100001;
/// 创建进程
/// 
/// 系统调用编号为 [IDX_PROCESS]
/// 函数封装为 [crate::multitask::create_process]
pub const IDX_SUB_PROCESS_CREATE: u64 = 0x100002;
/// 停止进程
/// 
/// 系统调用编号为 [IDX_PROCESS]
/// 函数封装为 [crate::multitask::kill_process]
pub const IDX_SUB_PROCESS_KILL: u64 = 0x100003;
/// 等待进程停止
/// 
/// 系统调用编号为 [IDX_PROCESS]
/// 函数封装为 [crate::multitask::wait_process]
pub const IDX_SUB_PROCESS_WAIT: u64 = 0x100004;