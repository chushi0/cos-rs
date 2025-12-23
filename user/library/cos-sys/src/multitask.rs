use core::{mem::MaybeUninit, ptr::NonNull};

use crate::{
    error::{Result, SyscallError},
    idx::*,
    syscall,
};

pub const EXIT_SUCCESS: u64 = 0;
pub const EXIT_KILL: u64 = 1;

/// 退出进程
///
/// 退出当前进程。此函数对调用进程无约束，永不失败且永不返回。
/// 进程退出后会回收其全部资源，包括已创建的线程、已分配的内存、已打开的文件、已创建的内核对象等。
///
/// 进程可向内核告知进程退出码，它可能被其他应用程序使用。
/// 一般来说，使用 [EXIT_SUCCESS] 表示正常退出，而其他所有退出码表示异常退出。
pub fn exit(code: u64) -> ! {
    unsafe {
        syscall!(IDX_EXIT, IDX_SUB_EXIT_PROCESS, code);
    }
    unreachable!()
}

/// 退出线程
///
/// 退出当前线程，进程内的其他线程继续执行。此函数对调用进程无约束，永不失败且永不反悔。
/// 线程退出后会释放线程级资源，包括栈内存（如果由内核管理）、线程相关内核对象等。
///
/// 线程可向内核告知线程退出码，它可能被进程内的其他线程使用。
/// 一般来说，使用 [EXIT_SUCCESS] 表示正常退出，而其他所有退出码表示异常退出。
///
/// 如果线程退出后，进程内没有其他线程，则会退出进程。最后一个线程的退出码为进程的退出码。
pub fn exit_thread(code: u64) -> ! {
    unsafe {
        syscall!(IDX_EXIT, IDX_SUB_EXIT_THREAD, code);
    }
    unreachable!()
}

/// 获取当前线程
///
/// 获取当前执行的线程，返回 u64 表示线程id，并可用于其他系统调用
pub fn current_thread() -> Result<u64> {
    let mut thread_id = MaybeUninit::<u64>::uninit();
    let thread_id_ptr = thread_id.as_mut_ptr() as u64;
    let error = unsafe { syscall!(IDX_THREAD, IDX_SUB_THREAD_CURRENT, thread_id_ptr) };
    SyscallError::to_result(error).map(|_| unsafe { thread_id.assume_init() })
}

/// 挂起线程
///
/// 将线程从就绪或运行状态切换为挂起状态，不再被调度器执行。
/// 传入的线程id可以为当前线程，也可以为当前进程内的其他线程。
///
/// 线程被手动挂起后，除非调用[resume_thread]，否则永不执行。
pub fn suspend_thread(thread_id: u64) -> Result {
    let error = unsafe { syscall!(IDX_THREAD, IDX_SUB_THREAD_SUSPEND, thread_id) };
    SyscallError::to_result(error)
}

/// 恢复线程
///
/// 将线程从挂起状态切换为就绪状态，由调度器继续执行。
/// 传入的线程id为当前进程内的其他线程。
///
/// 如果目标线程不是手动挂起的线程，则返回 [crate::error::ErrorKind::PermissionDenied]
pub fn resume_thread(thread_id: u64) -> Result {
    let error = unsafe { syscall!(IDX_THREAD, IDX_SUB_THREAD_RESUME, thread_id) };
    SyscallError::to_result(error)
}

/// 停止线程
///
/// 无论目标线程为何种状态，停止线程并回收其资源。
/// 被停止的线程拥有固定错误码 [EXIT_KILL]
///
/// 对于用户程序而言，避免使用这种方式停止线程，因为它不能保证线程在何时被停止，
/// 线程可能来不及回收其资源。
pub fn kill_thread(thread_id: u64) -> Result {
    let error = unsafe { syscall!(IDX_THREAD, IDX_SUB_THREAD_KILL, thread_id) };
    SyscallError::to_result(error)
}

/// 创建线程
///
/// 创建一个处于挂起或就绪状态的线程，其与当前线程共享进程空间。
///
/// # 参数
///
/// - entry_point: 线程入口点。新线程将从这里开始执行代码。
/// - stack: 线程使用的栈指针。在进入线程入口点时，保证rsp寄存器被设置为此值。如果为[None]，则由内核创建栈空间。
/// - params: 可选的参数，在进入入口点时，rdi被设置为此值，以允许向新线程传递参数。
/// - initial_suspend: 如果为true，新线程会以挂起方式创建，必须在准备好后通过[resume_thread]继续执行
///
/// # ABI
///
/// 此函数可以不假设ABI，但推荐使用System V ABI以接收参数。当新线程启动后，它会跳转到entry_point的第一条汇编指令开始运行。
/// 在此之前，内核**不会**对栈空间进行任何操作。除rsp和rdi寄存器外，所有寄存器的值均是**未定义**的。
///
/// 函数**不能使用**ret指令返回，在entry_point函数中直接执行ret指令是未定义行为。
/// ret指令会从栈中获取返回地址并跳转到此地址继续执行代码。但它**无法**停止线程。
/// 正确的停止线程方式是通过 [exit_thread] 系统调用。
///
/// 参数 entry_point 的签名默认使用 extern "C" 来固定System V ABI。对于特殊使用方式，可以通过unsafe代码进行强制转换。
///
/// # 栈
///
/// 可选择由用户代码管理栈，或由内核管理栈。
///
/// 如果由用户代码管理栈，那么rsp寄存器会被设置为用户传入的stack。用户有义务保证栈空间足够且已经正确对齐。
/// 用户需要保证在新线程执行期间，栈空间不会被破坏（如错误释放）。线程执行结束后，栈空间依然保留，并可在其他线程中继续访问。
///
/// 如果由内核管理栈，那么内核会创建一个4K内存页用于栈空间，并将rsp寄存器设置为栈的高地址，且对齐到16n+8。
/// 在线程运行期间，栈空间可由进程内的所有线程访问，但在线程停止后，栈空间会被内核回收销毁。
///
/// # 返回
///
/// 如果线程成功，返回新线程的id
///
/// # Safety
///
/// 请确保以下不变式，否则可能会有未定义行为
/// 1. entry_point 必须为合法的可执行代码页中的函数，且不会返回
/// 2. stack 必须为合法的栈指针，且满足入口点函数的ABI
/// 3. 如果params为指针，请确保在新线程中正确处理
/// 4. 如果栈空间由用户管理，在新线程运行期间，不能释放栈空间。
pub unsafe fn create_thread(
    entry_point: extern "C" fn(u64) -> !,
    stack: Option<NonNull<u8>>,
    params: u64,
    initial_suspend: bool,
) -> Result<u64> {
    let new_rip = entry_point as u64;
    let new_rsp = stack.map_or(0, |stack| stack.as_ptr() as u64);
    let initial_suspend = initial_suspend as u64;

    let mut new_thread_id = MaybeUninit::<u64>::uninit();
    let new_thread_id_ptr = new_thread_id.as_mut_ptr() as u64;
    let error = unsafe {
        syscall!(
            IDX_THREAD,
            IDX_SUB_THREAD_CREATE,
            new_rip,
            new_rsp,
            params,
            initial_suspend,
            new_thread_id_ptr
        )
    };
    SyscallError::to_result(error).map(|_| unsafe { new_thread_id.assume_init() })
}

/// 获取当前进程
///
/// 获取当前进程，返回 u64 表示进程id
pub fn current_process() -> Result<u64> {
    let mut process_id = MaybeUninit::<u64>::uninit();
    let process_id_ptr = process_id.as_mut_ptr() as u64;
    let error = unsafe { syscall!(IDX_PROCESS, IDX_SUB_PROCESS_CURRENT, process_id_ptr) };
    SyscallError::to_result(error).map(|_| unsafe { process_id.assume_init() })
}

/// 创建进程
///
/// 指定一个可执行文件，将其加载为进程，创建主线程进入其入口点。
/// 创建后的进程将作为当前进程的子进程。
///
/// 如果成功，将返回其进程ID
pub fn create_process(exe: &str) -> Result<u64> {
    let exe_ptr = exe.as_ptr() as u64;
    let exe_len = exe.len() as u64;
    let mut process_id = MaybeUninit::<u64>::uninit();
    let process_id_ptr = process_id.as_mut_ptr() as u64;
    let error = unsafe {
        syscall!(
            IDX_PROCESS,
            IDX_SUB_PROCESS_CREATE,
            exe_ptr,
            exe_len,
            process_id_ptr
        )
    };
    SyscallError::to_result(error).map(|_| unsafe { process_id.assume_init() })
}

/// 强制停止进程
///
/// 停止进程并清理其所有资源。指定的进程必须为当前进程的子进程。
pub fn kill_process(process_id: u64) -> Result<()> {
    let error = unsafe { syscall!(IDX_PROCESS, IDX_SUB_PROCESS_KILL, process_id) };
    SyscallError::to_result(error)
}

/// 等待指定子进程退出，并获取其退出码。
///
/// 指定的进程必须为当前进程的子进程。
/// 在进程退出之后，无法再次通过此函数获取其退出码。
/// 如果进程当前正在运行，此函数将挂起当前线程。
pub fn wait_process(process_id: u64) -> Result<u64> {
    let mut exit_code = MaybeUninit::<u64>::uninit();
    let exit_code_ptr = exit_code.as_mut_ptr() as u64;
    let error = unsafe {
        syscall!(
            IDX_PROCESS,
            IDX_SUB_PROCESS_WAIT,
            process_id,
            exit_code_ptr
        )
    };
    SyscallError::to_result(error).map(|_| unsafe { exit_code.assume_init() })
}
