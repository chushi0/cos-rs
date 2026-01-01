pub const IDX_DEBUG_INFO: u64 = 0x100001;
pub const IDX_DEBUG_GET_CHAR: u64 = 0x100002;
pub const IDX_DEBUG_PUT_CHAR: u64 = 0x100003;

/// 退出当前进程
///
/// 函数封装为 [crate::multitask::exit]
pub const IDX_EXIT_PROCESS: u64 = 1;
/// 退出当前线程
///
/// 函数封装为 [crate::multitask::exit_thread]
pub const IDX_EXIT_THREAD: u64 = 2;

/// 获取当前线程
///
/// 函数封装为 [crate::multitask::current_thread]
pub const IDX_THREAD_CURRENT: u64 = 3;
/// 挂起线程
///
/// 函数封装为 [crate::multitask::suspend_thread]
pub const IDX_THREAD_SUSPEND: u64 = 4;
/// 恢复线程
///
/// 函数封装为 [crate::multitask::resume_thread]
pub const IDX_THREAD_RESUME: u64 = 5;
/// 停止线程
///
/// 函数封装为 [crate::multitask::kill_thread]
pub const IDX_THREAD_KILL: u64 = 6;
/// 创建线程
///
/// 函数封装为 [crate::multitask::create_thread]
pub const IDX_THREAD_CREATE: u64 = 7;

/// 申请内存页，内存页默认为可读写不可执行
///
/// 函数封装为 [crate::memory::alloc_page]
pub const IDX_MEMORY_ALLOC: u64 = 8;
/// 释放内存页
///
/// 函数封装为 [crate::memory::free_page]
pub const IDX_MEMORY_FREE: u64 = 9;
/// 测试内存页
///
/// 函数封装为 [crate::memory::test_page]
pub const IDX_MEMORY_TEST: u64 = 10;
/// 修改内存页权限
///
/// 函数封装为 [crate::memory::modify_page]
pub const IDX_MEMORY_MODIFY: u64 = 11;

/// 获取当前进程
///
/// 函数封装为 [crate::multitask::current_process]
pub const IDX_PROCESS_CURRENT: u64 = 12;
/// 创建进程
///
/// 函数封装为 [crate::multitask::create_process]
pub const IDX_PROCESS_CREATE: u64 = 13;
/// 停止进程
///
/// 函数封装为 [crate::multitask::kill_process]
pub const IDX_PROCESS_KILL: u64 = 14;
/// 等待进程停止
///
/// 函数封装为 [crate::multitask::wait_process]
pub const IDX_PROCESS_WAIT: u64 = 15;

/// 创建文件
///
/// 函数封装为 [crate::file::create]
pub const IDX_FILE_CREATE: u64 = 16;
/// 打开文件
///
/// 函数封装为 [crate::file::open]
pub const IDX_FILE_OPEN: u64 = 17;
/// 读取文件内容
///
/// 函数封装为 [crate::file::read]
pub const IDX_FILE_READ: u64 = 18;
/// 写入文件内容
///
/// 函数封装为 [crate::file::write]
pub const IDX_FILE_WRITE: u64 = 19;
/// 获取文件游标位置
///
/// 函数封装为 [crate::file::get_pos]
pub const IDX_FILE_GET_POS: u64 = 20;
/// 移动文件游标位置
///
/// 函数封装为 [crate::file::set_pos]
pub const IDX_FILE_SET_POS: u64 = 21;
/// 关闭文件
///
/// 函数封装为 [crate::file::close]
pub const IDX_FILE_CLOSE: u64 = 22;
