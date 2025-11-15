use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{Arc, Mutex},
};

use filesystem::{
    BoxFuture,
    device::{BlockDevice, BlockDeviceError},
};

pub struct HostFileBlockDevice {
    file: Mutex<File>,
    file_size: u64,
}

impl HostFileBlockDevice {
    pub fn new<P>(path: P, file_size: u64) -> Result<Arc<Self>, io::Error>
    where
        P: AsRef<Path>,
    {
        assert!(file_size % 512 == 0);
        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(file_size)?;
        Ok(Arc::new(Self {
            file: Mutex::new(file),
            file_size,
        }))
    }
}

impl BlockDevice for HostFileBlockDevice {
    fn block_size(&self) -> u64 {
        512
    }

    fn block_count(&self) -> u64 {
        self.file_size
    }

    fn write_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index >= self.file_size / 512 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            assert!(buf.len() == 512);
            let mut file = self.file.lock().unwrap();
            file.seek(SeekFrom::Start(block_index * 512))?;
            file.write_all(buf)?;
            Ok(())
        })
    }

    fn read_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index >= self.file_size / 512 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            assert!(buf.len() == 512);
            let mut file = self.file.lock().unwrap();
            file.seek(SeekFrom::Start(block_index * 512))?;
            file.read_exact(buf)?;
            Ok(())
        })
    }

    fn write_blocks<'fut>(
        &'fut self,
        block_index: u64,
        count: u64,
        buf: &'fut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index + count > self.file_size / 512 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            assert!(buf.len() as u64 == 512 * count);
            let mut file = self.file.lock().unwrap();
            file.seek(SeekFrom::Start(block_index * 512))?;
            file.write_all(buf)?;
            Ok(())
        })
    }

    fn read_blocks<'fut>(
        &'fut self,
        block_index: u64,
        count: u64,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(async move {
            if block_index + count > self.file_size / 512 {
                return Err(BlockDeviceError::OutOfBounds);
            }
            assert!(buf.len() as u64 == 512 * count);
            let mut file = self.file.lock().unwrap();
            file.seek(SeekFrom::Start(block_index * 512))?;
            file.read_exact(buf)?;
            Ok(())
        })
    }
}
