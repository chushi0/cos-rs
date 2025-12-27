pub const IDX_DEBUG: u64 = 0x0;
pub const IDX_SUB_DEBUG_INFO: u64 = 0x0;
pub const IDX_SUB_DEBUG_GET_CHAR: u64 = 0x1;
pub const IDX_SUB_DEBUG_PUT_CHAR: u64 = 0x2;

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

/// 与文件相关的系统调用
///
/// 子编号：
/// [IDX_SUB_FILE_CREATE]
/// [IDX_SUB_FILE_OPEN]
/// [IDX_SUB_FILE_READ]
/// [IDX_SUB_FILE_WRITE]
/// [IDX_SUB_FILE_GET_POS]
/// [IDX_SUB_FILE_SET_POS]
/// [IDX_SUB_FILE_CLOSE]
pub const IDX_FILE: u64 = 0x10005;
/// 创建文件
///
/// 系统调用编号为 [IDX_FILE]
/// 函数封装为 [crate::file::create]
pub const IDX_SUB_FILE_CREATE: u64 = 0x100001;
/// 打开文件
///
/// 系统调用编号为 [IDX_FILE]
/// 函数封装为 [crate::file::open]
pub const IDX_SUB_FILE_OPEN: u64 = 0x100002;
/// 读取文件内容
///
/// 系统调用编号为 [IDX_FILE]
/// 函数封装为 [crate::file::read]
pub const IDX_SUB_FILE_READ: u64 = 0x100003;
/// 写入文件内容
///
/// 系统调用编号为 [IDX_FILE]
/// 函数封装为 [crate::file::write]
pub const IDX_SUB_FILE_WRITE: u64 = 0x100004;
/// 获取文件游标位置
///
/// 系统调用编号为 [IDX_FILE]
/// 函数封装为 [crate::file::get_pos]
pub const IDX_SUB_FILE_GET_POS: u64 = 0x100005;
/// 移动文件游标位置
///
/// 系统调用编号为 [IDX_FILE]
/// 函数封装为 [crate::file::set_pos]
pub const IDX_SUB_FILE_SET_POS: u64 = 0x100006;
/// 关闭文件
///
/// 系统调用编号为 [IDX_FILE]
/// 函数封装为 [crate::file::close]
pub const IDX_SUB_FILE_CLOSE: u64 = 0x100007;
