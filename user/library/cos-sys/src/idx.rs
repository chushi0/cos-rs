pub const IDX_DEBUG_INFO: u64 = 0x1F00001;
pub const IDX_DEBUG_GET_CHAR: u64 = 0x1F00002;
pub const IDX_DEBUG_PUT_CHAR: u64 = 0x1F00003;

/// 退出当前进程
///
/// 函数封装为 [crate::multitask::exit]
pub const IDX_EXIT_PROCESS: u64 = 0x100001;
/// 退出当前线程
///
/// 函数封装为 [crate::multitask::exit_thread]
pub const IDX_EXIT_THREAD: u64 = 0x100002;

/// 获取当前线程
///
/// 函数封装为 [crate::multitask::current_thread]
pub const IDX_THREAD_CURRENT: u64 = 0x200001;
/// 挂起线程
///
/// 函数封装为 [crate::multitask::wait_thread]
pub const IDX_THREAD_WAIT: u64 = 0x200002;
/// 恢复线程
///
/// 函数封装为 [crate::multitask::wake_thread]
pub const IDX_THREAD_WAKE: u64 = 0x200003;
/// 停止线程
///
/// 函数封装为 [crate::multitask::kill_thread]
pub const IDX_THREAD_KILL: u64 = 0x200004;
/// 创建线程
///
/// 函数封装为 [crate::multitask::create_thread]
pub const IDX_THREAD_CREATE: u64 = 0x200005;
/// 等待线程执行完成
///
/// 函数封装为 [crate::multitask::join_thread]
pub const IDX_THREAD_JOIN: u64 = 0x200006;

/// 申请内存页，内存页默认为可读写不可执行
///
/// 函数封装为 [crate::memory::alloc_page]
pub const IDX_MEMORY_ALLOC: u64 = 0x300001;
/// 释放内存页
///
/// 函数封装为 [crate::memory::free_page]
pub const IDX_MEMORY_FREE: u64 = 0x300002;

/// 获取当前进程
///
/// 函数封装为 [crate::multitask::current_process]
pub const IDX_PROCESS_CURRENT: u64 = 0x400001;
/// 创建进程
///
/// 函数封装为 [crate::multitask::create_process]
pub const IDX_PROCESS_CREATE: u64 = 0x400002;
/// 停止进程
///
/// 函数封装为 [crate::multitask::kill_process]
pub const IDX_PROCESS_KILL: u64 = 0x400003;
/// 等待进程停止
///
/// 函数封装为 [crate::multitask::wait_process]
pub const IDX_PROCESS_WAIT: u64 = 0x400004;

/// 创建文件
///
/// 函数封装为 [crate::file::create]
pub const IDX_FILE_CREATE: u64 = 0x500001;
/// 打开文件
///
/// 函数封装为 [crate::file::open]
pub const IDX_FILE_OPEN: u64 = 0x500002;
/// 读取文件内容
///
/// 函数封装为 [crate::file::read]
pub const IDX_FILE_READ: u64 = 0x500003;
/// 写入文件内容
///
/// 函数封装为 [crate::file::write]
pub const IDX_FILE_WRITE: u64 = 0x500004;
/// 获取文件游标位置
///
/// 函数封装为 [crate::file::get_pos]
pub const IDX_FILE_GET_POS: u64 = 0x500005;
/// 移动文件游标位置
///
/// 函数封装为 [crate::file::set_pos]
pub const IDX_FILE_SET_POS: u64 = 0x500006;
/// 关闭文件
///
/// 函数封装为 [crate::file::close]
pub const IDX_FILE_CLOSE: u64 = 0x500007;
