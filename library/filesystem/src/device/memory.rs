use core::fmt::{self, Write};

use alloc::{boxed::Box, vec::Vec};
use async_locks::mutex::Mutex;

use crate::device::BlockDevice;

/// 基于内存的块设备实现
pub struct MemoryDevice {
    // 数据
    data: Mutex<Vec<u8>>,
    // 块大小
    block_size: u64,
    // 块数量
    block_count: u64,
}

impl MemoryDevice {
    /// 创建一个新的基于内存的块设备
    ///
    /// `size_in_bytes`为设备的总大小，单位为字节。
    /// `block_size`为每个块的大小，单位为字节。
    pub fn new(size_in_bytes: u64, block_size: u64) -> Self {
        let aligned_size = (size_in_bytes + block_size - 1) / block_size * block_size;
        let mut data = Vec::new();
        data.resize(aligned_size as usize, 0);
        Self {
            data: Mutex::new(data),
            block_size,
            block_count: aligned_size / block_size,
        }
    }

    /// 将块设备数据格式化并输出到指定区域
    ///
    /// 块设备的数据将以十六进制的格式输出，类似如下格式：
    ///
    /// ```txt
    ///             0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    /// 0x00000000 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 | ................
    /// 0x00000010 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 | ................
    /// 0x00000020 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 | ................
    /// 0x00000030 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 | ................
    /// 0x00000040 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 | ................
    /// 0x00000050 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 | ................
    /// ```
    ///
    /// 最右侧的为字符预览，仅ascii alphanumeric会被展示，其余字符均显示为.
    pub async fn dump<W: Write>(&self, out: &mut W) -> Result<(), fmt::Error> {
        let data = self.data.lock().await;

        // dump line
        writeln!(
            out,
            "            0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F"
        )?;

        // dump data
        for (chunk_seq, chunk) in data.chunks(16).enumerate() {
            write!(out, "0x{chunk_seq:07X}0")?;
            for data in chunk {
                write!(out, " {data:02X}")?;
            }
            write!(out, " | ")?;
            for data in chunk {
                if data.is_ascii_alphanumeric() {
                    write!(out, "{}", *data as char)?;
                } else {
                    write!(out, ".")?;
                }
            }
            writeln!(out)?;
        }

        Ok(())
    }
}

impl BlockDevice for MemoryDevice {
    fn block_size(&self) -> u64 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn write_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut [u8],
    ) -> crate::BoxFuture<'fut, Result<(), super::BlockDeviceError>> {
        Box::pin(async move {
            if block_index >= self.block_count() {
                return Err(super::BlockDeviceError::OutOfBounds);
            }
            if (buf.len() as u64) < self.block_size {
                return Err(super::BlockDeviceError::OutOfBounds);
            }

            let start = (block_index * self.block_size) as usize;
            let end = start + self.block_size as usize;

            (*self.data.lock().await)[start..end].copy_from_slice(&buf[..self.block_size as usize]);
            Ok(())
        })
    }

    fn read_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut mut [u8],
    ) -> crate::BoxFuture<'fut, Result<(), super::BlockDeviceError>> {
        Box::pin(async move {
            if block_index >= self.block_count() {
                return Err(super::BlockDeviceError::OutOfBounds);
            }
            if (buf.len() as u64) < self.block_size {
                return Err(super::BlockDeviceError::OutOfBounds);
            }

            let start = (block_index * self.block_size) as usize;
            let end = start + self.block_size as usize;
            (&mut buf[..self.block_size as usize])
                .copy_from_slice(&(*self.data.lock().await)[start..end]);

            Ok(())
        })
    }
}
