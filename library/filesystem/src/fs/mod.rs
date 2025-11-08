use alloc::{boxed::Box, string::String, vec::Vec};

use crate::{BoxFuture, device::BlockDeviceError, path::Path};

pub mod fat32;

/// 文件系统的抽象
///
/// 此trait定义了文件系统的基本接口，包括文件和目录的创建、删除、重命名等。
///
/// 当构造文件系统时，通常需要传入一个块设备作为底层存储介质。此接口由实现层定义，不属于接口定义范围。
///
/// 此trait为dyn safe的，可以进行动态分发。
///
/// # Cancel Safety
/// 除极少数特例外，所有异步方法都是 **取消不安全（not cancel safe）** 的。
///
/// 文件系统的许多操作涉及多次 I/O：
/// - 中途中止可能导致文件系统元数据或日志不一致；  
/// - 即使是“只读”操作，也可能触发日志更新、访问时间刷新等隐式写入；  
/// - 取消 Future 可能使文件系统进入部分更新状态。  
pub trait FileSystem: Send + Sync + 'static {
    /// 磁盘总空间，单位为字节
    fn total_space(&self) -> BoxFuture<'_, Result<u64, FileSystemError>>;

    /// 磁盘剩余可用空间，单位为字节
    fn free_space(&self) -> BoxFuture<'_, Result<u64, FileSystemError>>;

    /// 创建文件
    ///
    /// 文件系统不会创建父级目录，仅会创建最后一级文件。如果目录不存在，返回 [`FileSystemError::FileNotFound`]。
    /// 文件系统不允许在同一目录下有完全同名的文件或文件夹。如果此目录中文件已经存在，返回 [`FileSystemError::FileExists`]。
    fn create_file<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>>;

    /// 创建文件夹
    ///
    /// 文件系统不会创建父级目录，仅会创建最后一级文件。如果目录不存在，返回 [`FileSystemError::FileNotFound`]。
    /// 文件系统不允许在同一目录下有完全同名的文件或文件夹。如果此目录中文件已经存在，返回 [`FileSystemError::FileExists`]。
    fn create_directory<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>>;

    /// 打开文件
    ///
    /// 此函数仅打开文件，并不会创建文件。如果文件不存在，返回 [`FileSystemError::FileNotFound`]。
    /// 如果指定的路径为目录而非文件，返回 [`FileSystemError::FileTypeMismatch`]。
    fn open_file<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<Box<dyn FileHandle>, FileSystemError>>;

    /// 删除文件
    ///
    /// 如果指定路径不存在，返回 [`FileSystemError::FileNotFound`]。
    /// 如果指定路径为目录，返回 [`FileSystemError::FileTypeMismatch`]。
    fn delete_file<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>>;

    /// 删除文件夹
    ///
    /// 文件系统只允许删除非空文件夹。如果文件夹非空，需要先逐个删除文件夹中的内容。
    ///
    /// 如果指定路径不存在，返回 [`FileSystemError::FileNotFound`]。
    /// 如果指定路径为文件，返回 [`FileSystemError::FileTypeMismatch`]。
    /// 如果指定路径为目录，但是文件夹非空，返回 [`FileSystemError::FileExists`]。
    fn delete_directory<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>>;

    /// 重命名文件或文件夹，或移动文件或文件夹
    ///
    /// 如果指定的旧路径不存在，返回 [`FileSystemError::FileNotFound`]
    /// 如果指定的新路径已经存在，返回 [`FileSystemError::FileExists`]
    /// 如果指定的新路径对应目录不存在，返回 [`FileSystemError::FileNotFound`]
    fn rename<'fut>(
        &'fut self,
        old_path: Path<'fut>,
        new_path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>>;

    /// 获取文件或文件夹元数据
    ///
    /// 如果指定路径不存在，返回 [`FileSystemError::FileNotFound`]
    fn get_metadata<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<FileMetadata, FileSystemError>>;

    /// 列出文件夹中的内容
    ///
    /// 如果指定路径不存在，返回 [`FileSystemError::FileNotFound`]
    /// 如果指定路径为文件夹，返回 [`FileSystemError::FileTypeMismatch`]
    fn list_directory<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<Vec<FileMetadata>, FileSystemError>>;

    /// 卸载文件系统
    ///
    /// 调用方需尽可能保证每个文件系统都会调用 [`FileSystem::unmounted`] 函数，
    /// 以将内存中的数据刷新至磁盘，避免损坏文件系统。
    ///
    /// 文件系统需要将所有文件关闭，并将文件系统需将所有内存缓存的数据写入磁盘。
    /// 文件系统不应依赖Drop关闭资源，而必须依赖此函数。
    ///
    /// 对实现方而言：不应完全依赖此函数。在极端情况下，此函数不保证调用（如意外断电、硬件重置、内核恐慌等）。
    /// 实现方需保证在不调用此函数的情况下依然能正确保障一致性。
    ///
    /// 在文件系统卸载后，对文件系统的所有操作均返回 [`FileSystemError::Unmounted`] 错误
    fn unmount(&self) -> BoxFuture<'_, Result<(), FileSystemError>>;
}

/// 文件
///
/// 此trait缓存了文件系统的一些数据，以便快速访问并操作文件
///
/// 当调用 [`FileSystem::open_file`] 时，文件系统负责将文件的关联内容读取，并缓存到 [`FileHandle`] 中。
/// 当需要继续对文件进行操作时，无需重新搜索目录树。
///
/// [`FileHandle`] 内部可以持有 [`FileSystem`] 中部分数据的引用。此情况下，如果文件系统被卸载，[`FileHandle`]
/// 的函数应当返回 [`FileSystemError::Unmounted`] 错误。
///
/// [`FileHandle`] 内部存储了写入/读取文件的指针。当进行写入和读取操作时，指针会同步移动。调用方也可以使用
/// [`FileHandle::move_pointer`] 和 [`FileHandle::get_pointer`] 来手动控制指针。指针以字节为单位。
///
/// 当不再需要文件时，调用 [`FileHandle::close`] 关闭文件。此后，所有调用都会返回 [`FileSystemError::FileClosed`] 错误。
///
/// 此trait为dyn safe的，可以进行动态分发。
pub trait FileHandle: Send + Sync + 'static {
    /// 关闭文件
    fn close(&mut self) -> BoxFuture<'_, Result<(), FileSystemError>>;

    /// 移动文件指针
    fn move_pointer(&mut self, position: u64) -> BoxFuture<'_, Result<(), FileSystemError>>;

    /// 获取文件指针位置
    fn get_pointer(&mut self) -> BoxFuture<'_, Result<u64, FileSystemError>>;

    /// 读取文件内容
    ///
    /// buf: 读取缓冲区
    /// 返回的u64为实际读取的字节数
    ///
    /// 对于实际读取范围超出文件大小的读取请求，应返回Ok(0)而非Err
    fn read<'fut>(
        &'fut mut self,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<u64, FileSystemError>>;

    /// 写入文件内容
    ///
    /// buf: 写入缓冲区
    ///
    /// 如果文件指针不在文件末尾，则写入意味着覆盖旧数据
    /// 如果文件指针在文件末尾，则写入意味着追加数据
    fn write<'fut>(&'fut mut self, buf: &'fut [u8])
    -> BoxFuture<'fut, Result<(), FileSystemError>>;
}

/// 文件系统错误
#[derive(Debug)]
pub enum FileSystemError {
    /// 底层IO错误
    IoError(BlockDeviceError),
    /// 文件不存在
    FileNotFound,
    /// 文件已存在
    FileExists,
    /// 磁盘已满
    DiskFull,
    /// 文件类型不匹配
    FileTypeMismatch,
    /// 文件名过长
    NameTooLang,
    /// 文件正被占用
    FileOccupied,
    /// 操作不受支持
    OperationNotSupport,
    /// 文件系统已卸载
    Unmounted,
    /// 文件已关闭
    FileClosed,
}

impl From<BlockDeviceError> for FileSystemError {
    fn from(value: BlockDeviceError) -> Self {
        Self::IoError(value)
    }
}

pub struct FileMetadata {
    // 文件名
    pub name: String,
    // 文件大小
    pub size: u64,
    // 是否为目录
    pub is_directory: bool,
    // 实际占用空间
    pub allocated_size: u64,
}

// 断言FileSystem是dyn safe的
const _: fn(&dyn FileSystem) -> &dyn FileSystem = |x| x;
