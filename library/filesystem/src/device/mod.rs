use alloc::boxed::Box;

use crate::BoxFuture;

pub mod mbr;
pub mod memory;

/// 块设备的抽象
///
/// 此trait定义了块设备的基本属性和行为。[`BlockDevice`] 仅表示逻辑上的块设备，它的实现可以是磁盘，
/// 可以是磁盘的某个分区，可以是U盘，可以是内存，可以是宿主机上的某个文件，可以是远程计算机的某个块存储服务。
/// 无论 [`BlockDevice`] 的底层是什么存储介质，对外表现均为对一个完整块的读取和写入。对于不同介质的特殊访问逻辑，
/// 应对调用方透明。
///
/// 此trait为dyn safe的，可以进行动态分发。
///
/// # Cancel Safety
/// 此 trait 的所有异步 I/O 操作都是 **取消不安全（not cancel safe）** 的。
///
/// 如果在执行过程中取消 Future：
/// - 设备可能已部分完成读写；
/// - 部分数据可能已写入或读取；
/// - 底层 I/O 控制器可能仍在使用其缓冲区。
///
/// 若任务在此时释放相关资源，可能导致数据损坏或未定义行为。
pub trait BlockDevice: Send + Sync + 'static {
    /// 获取块设备每个扇区的大小，单位为字节。
    ///
    /// 通常而言，块设备的扇区大小为512字节或4096字节。
    /// 对于消费级硬件，大多都支持512字节的访问。
    ///
    /// 对于支持可选逻辑扇区大小的块设备，可以实现为获取默认值，也可以提供更多方法供用户选择。
    /// 块设备抽象不提供选取函数，应由具体实现判断是否需要额外暴露方法。
    ///
    /// 扇区大小不能被修改。调用方可以假设每次调用此函数均返回相同的值，以此减少动态分发的成本。
    fn block_size(&self) -> u64;

    /// 获取块设备的扇区数量
    ///
    /// 扇区数量不能被修改。调用方可以假设每次调用此函数均返回相同的值，以此减少动态分发的成本。
    fn block_count(&self) -> u64;

    /// 写入单个块
    ///
    /// `block_index`为块索引，范围为`0..block_count()`。
    /// `buf`为数据缓冲区，其长度必须等于`block_size()`
    fn write_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>>;

    /// 读取单个块
    ///
    /// `block_index`为块索引，范围为`0..block_count()`
    /// `buf`为数据缓冲区，其长度必须等于`block_size()`
    fn read_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>>;

    /// 写入多个块
    ///
    /// 在磁盘层面，一次访问多个块是常见且高效的操作。
    /// `block_index`为起始块索引，范围为`0..block_count()`。
    /// `count`为块数量，范围为`1..=(block_count() - block_index)`。
    /// `buf`为数据缓冲区，其长度必须等于`block_size() * count`
    fn write_blocks<'fut>(
        &'fut self,
        block_index: u64,
        count: u64,
        buf: &'fut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            let size = self.block_size() as usize;
            for i in 0..count {
                let offset = (i as usize) * size;
                self.write_block(block_index + i, &buf[offset..(offset + size)])
                    .await?;
            }
            Ok(())
        })
    }

    /// 读取多个块
    ///
    /// 在磁盘层面，一次访问多个块是常见且高效的操作。
    /// `block_index`为起始块索引，范围为`0..block_count()`。
    /// `count`为块数量，范围为`1..=(block_count() - block_index)`。
    /// `buf`为数据缓冲区，其长度必须等于`block_size() * count`
    fn read_blocks<'fut>(
        &'fut self,
        block_index: u64,
        count: u64,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            let size = self.block_size() as usize;
            for i in 0..count {
                let offset = (i as usize) * size;
                self.read_block(block_index + i, &mut buf[offset..(offset + size)])
                    .await?;
            }
            Ok(())
        })
    }

    /// 批量写零
    ///
    /// 通常批量写零要比直接写数据更快，因为可能无需传输额外数据。
    ///
    /// 写零不等同于清空数据，因为清空数据除了可以写零以外还可以写一，也可以用随机值覆盖。
    /// 调用此函数应当向块设备中写入确定的零，就像使用 [BlockDevice::write_blocks] 一样。
    ///
    /// 如果调用方仅需要清除信息而不在意写入的是否为零，使用 [BlockDevice::clear_blocks]
    fn write_zeros(
        &self,
        block_index: u64,
        count: u64,
    ) -> BoxFuture<'_, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            let buffer = alloc::vec![0u8 ; (self.block_size() * count )as usize];
            self.write_blocks(block_index, count, &buffer).await
        })
    }

    /// 清空指定块
    ///
    /// 清空指定块通常比写入数据更快，因为硬件可以使用特殊手段进行破坏式清除。
    ///
    /// 当调用此函数后，目标块中的数据是未定义的。您需要重新写入数据后才能读取到正确数据。
    /// 如果您希望清空后读到零，请使用 [BlockDevice::write_zeros]。
    ///
    /// trait 提供了写零的默认实现，但实现方可以替换为其他任意破坏性操作。不推荐空实现，
    /// 因为调用方可能不希望数据依然在设备中保留。
    ///
    /// 包括 [BlockDevice::clear_blocks] 在内，任何覆盖数据的行为均不保证不可恢复，但通常可以提高恢复难度。
    /// 对于保密等级要求高的场景，请考虑在技术以外进行物理销毁。
    fn clear_blocks(
        &self,
        block_index: u64,
        count: u64,
    ) -> BoxFuture<'_, Result<(), BlockDeviceError>> {
        self.write_zeros(block_index, count)
    }
}

/// 块设备访问错误
#[derive(Debug)]
pub enum BlockDeviceError {
    /// 索引范围或缓冲区范围越界
    OutOfBounds,
    /// 底层IO错误
    IoError,
    #[cfg(feature = "dyn-io-error")]
    DynError(Box<dyn core::error::Error + Send + 'static>),
}

#[cfg(feature = "dyn-io-error")]
impl<E> From<E> for BlockDeviceError
where
    E: core::error::Error + Send + 'static,
{
    fn from(value: E) -> Self {
        Self::DynError(Box::new(value) as Box<_>)
    }
}

// 断言BlockDevice是dyn safe的
const _: fn(&dyn BlockDevice) -> &dyn BlockDevice = |x| x;
