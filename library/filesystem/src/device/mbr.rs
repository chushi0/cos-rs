use core::array::from_fn;

use alloc::{boxed::Box, sync::Arc};

use crate::{
    BoxFuture,
    device::{BlockDevice, BlockDeviceError},
};

pub const PARTITION_TYPE_BOOTLOADER: u8 = 0xEB;
pub const PARTITION_TYPE_FAT32: u8 = 0x0C;

// 分区表偏移
const MBR_PARTITION_TABLE_OFFSET: usize = 446;
// 每个条目大小
const MBR_PARTITION_ENTRY_SIZE: usize = 16;
// 每磁盘支持的分区表数量
const MBR_PARTITION_ENTRY_COUNT: usize = 4;

/// MBR分区映射
///
/// 此对象接受一个块设备，并将其解释为具有MBR分区的块设备，隐藏分区细节，自动计算块地址。
///
/// 对于每一个块设备，其第一个块（0号块）的 [MBR_PARTITION_TABLE_OFFSET] 位置可存储MBR分区信息，
/// 每个条目大小为 [MBR_PARTITION_ENTRY_SIZE]，最多有 [MBR_PARTITION_ENTRY_COUNT] 个。
/// 条目中记录了不同分区的块范围和分区类型。
///
/// 调用[MbrPartitionDevice::mount]，从块设备中读取分区信息，并创建所有分区的块设备。
/// 调用[MbrPartitionDevice::format]，重置分区为指定值，此操作仅修改分区信息，不修改分区内容。
/// 调用[MbrPartitionDevice::get_partition_type]，获得此分区的分区类型。
pub struct MbrPartitionDevice {
    inner: Arc<dyn BlockDevice>,
    partition_type: u8,
    start: u32,
    end: u32,
}

#[derive(Debug)]
pub enum MountError {
    IoError(BlockDeviceError),
    BlockSizeNotExpected,
}

#[derive(Debug)]
pub enum FormatError {
    IoError(BlockDeviceError),
    BlockSizeNotExpected,
    BadArgument,
}

impl From<BlockDeviceError> for MountError {
    fn from(value: BlockDeviceError) -> Self {
        Self::IoError(value)
    }
}

impl From<BlockDeviceError> for FormatError {
    fn from(value: BlockDeviceError) -> Self {
        Self::IoError(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MbrPartitionEntry {
    pub bootable: bool,
    pub start: u32,
    pub end: u32,
    pub partition_type: u8,
}

impl MbrPartitionDevice {
    /// 读取分区表，并根据分区表创建分区块设备
    pub async fn mount(
        block_device: Arc<dyn BlockDevice>,
    ) -> Result<[Option<Self>; MBR_PARTITION_ENTRY_COUNT], MountError> {
        let block_count = block_device.block_count();
        let block_size = block_device.block_size();
        if block_size < 512 {
            return Err(MountError::BlockSizeNotExpected);
        }

        let mut buf = alloc::vec![0u8; block_size as usize];
        block_device.read_block(0, &mut buf).await?;

        let mut partitions = from_fn(|_| None);
        for i in 0..MBR_PARTITION_ENTRY_COUNT {
            let slice_start = MBR_PARTITION_TABLE_OFFSET + i * MBR_PARTITION_ENTRY_SIZE;
            let slice_end = slice_start + MBR_PARTITION_ENTRY_SIZE;
            let entry = &buf[slice_start..slice_end];
            let partition_type = entry[4];
            if partition_type == 0 {
                continue;
            }
            let start = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
            let count = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]);
            let Some(end) = start.checked_add(count) else {
                continue;
            };
            if start == 0 || start as u64 >= block_count || end as u64 > block_count {
                continue;
            }
            partitions[i] = Some(Self {
                inner: block_device.clone(),
                partition_type,
                start,
                end,
            });
        }

        Ok(partitions)
    }

    /// 创建（覆盖）分区表
    pub async fn format(
        block_device: Arc<dyn BlockDevice>,
        partition: [Option<MbrPartitionEntry>; MBR_PARTITION_ENTRY_COUNT],
    ) -> Result<[Option<Self>; MBR_PARTITION_ENTRY_COUNT], FormatError> {
        let block_count = block_device.block_count();
        let block_size = block_device.block_size();
        if block_size < 512 {
            return Err(FormatError::BlockSizeNotExpected);
        }

        let mut buf = alloc::vec![0u8; block_size as usize];
        block_device.read_block(0, &mut buf).await?;

        let mut partitions = from_fn(|_| None);
        for i in 0..MBR_PARTITION_ENTRY_COUNT {
            let slice_start = MBR_PARTITION_TABLE_OFFSET + i * MBR_PARTITION_ENTRY_SIZE;
            let slice_end = slice_start + MBR_PARTITION_ENTRY_SIZE;
            let entry = &mut buf[slice_start..slice_end];

            let Some(partition) = partition[i] else {
                entry.fill(0);
                continue;
            };
            if partition.partition_type == 0
                || partition.start == 0
                || partition.start > partition.end
                || partition.end as u64 > block_count
            {
                return Err(FormatError::BadArgument);
            }

            entry[0] = if partition.bootable { 0x80 } else { 0 };
            (entry[1], entry[2], entry[3]) = lba_to_chs(partition.start);
            entry[4] = partition.partition_type;
            (entry[5], entry[6], entry[7]) = lba_to_chs(partition.end);
            let start_array = partition.start.to_le_bytes();
            entry[8] = start_array[0];
            entry[9] = start_array[1];
            entry[10] = start_array[2];
            entry[11] = start_array[3];
            let count_array = (partition.end - partition.start).to_le_bytes();
            entry[12] = count_array[0];
            entry[13] = count_array[1];
            entry[14] = count_array[2];
            entry[15] = count_array[3];

            partitions[i] = Some(Self {
                inner: block_device.clone(),
                partition_type: partition.partition_type,
                start: partition.start,
                end: partition.end,
            });
        }

        block_device.write_block(0, &buf).await?;

        Ok(partitions)
    }

    /// 获取分区类型（文件系统提示）
    pub fn get_partition_type(&self) -> u8 {
        self.partition_type
    }
}

impl BlockDevice for MbrPartitionDevice {
    fn block_size(&self) -> u64 {
        self.inner.block_size()
    }

    fn block_count(&self) -> u64 {
        (self.end - self.start) as u64
    }

    fn write_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index > self.end as u64 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            self.inner
                .write_block(block_index + self.start as u64, buf)
                .await
        })
    }

    fn read_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index > self.end as u64 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            self.inner
                .read_block(block_index + self.start as u64, buf)
                .await
        })
    }

    fn write_blocks<'fut>(
        &'fut self,
        block_index: u64,
        count: u64,
        buf: &'fut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index + count > self.end as u64 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            self.inner
                .write_blocks(block_index + self.start as u64, count, buf)
                .await
        })
    }

    fn read_blocks<'fut>(
        &'fut self,
        block_index: u64,
        count: u64,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index + count > self.end as u64 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            self.inner
                .read_blocks(block_index + self.start as u64, count, buf)
                .await
        })
    }

    fn write_zeros(
        &self,
        block_index: u64,
        count: u64,
    ) -> BoxFuture<'_, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index + count > self.end as u64 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            self.inner
                .write_zeros(block_index + self.start as u64, count)
                .await
        })
    }

    fn clear_blocks(
        &self,
        block_index: u64,
        count: u64,
    ) -> BoxFuture<'_, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index + count > self.end as u64 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            self.inner
                .clear_blocks(block_index + self.start as u64, count)
                .await
        })
    }
}

/// LBA逻辑地址转CHS柱面/磁头/扇区地址
fn lba_to_chs(lba: u32) -> (u8, u8, u8) {
    const HEAD_PER_CYLINDER: u32 = 256;
    const SECTOR_PER_TRACK: u32 = 63;
    let mut cylinder = lba / (HEAD_PER_CYLINDER * SECTOR_PER_TRACK);
    let mut head = lba % (HEAD_PER_CYLINDER * SECTOR_PER_TRACK) / SECTOR_PER_TRACK;
    let mut sectors = lba % SECTOR_PER_TRACK + 1;
    if cylinder > 1023 || head > 255 || sectors > 63 {
        cylinder = 1023;
        head = 255;
        sectors = 63;
    }
    (
        head as u8,
        ((sectors & 0x3f) as u8) | (((cylinder >> 6) & 0xc0) as u8),
        (cylinder & 0xff) as u8,
    )
}
