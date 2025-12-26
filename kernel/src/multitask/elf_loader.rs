use core::num::NonZeroU64;

use alloc::vec::Vec;

use crate::{
    multitask::{
        self,
        process::{Process, ProcessMemoryError, ProcessPageType},
    },
    sync::spin::SpinLock,
};

pub struct ElfLoader<'loader> {
    process: &'loader SpinLock<Process>,
    allocated_page: Vec<(u64, u64)>,
}

impl<'loader> ElfLoader<'loader> {
    pub fn new(process: &'loader SpinLock<Process>) -> Self {
        Self {
            process,
            allocated_page: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub enum ElfLoaderError {
    // 页表被保留
    PageReserved,
    // 页表保护不符预期
    PageProtection,
    // 进程内存访问错误
    ProcessMemoryError(ProcessMemoryError),
    // 内存分配失败
    AllocFail,
}

impl From<ProcessMemoryError> for ElfLoaderError {
    fn from(value: ProcessMemoryError) -> Self {
        Self::ProcessMemoryError(value)
    }
}

impl elf::Loader for ElfLoader<'_> {
    type LoaderError = ElfLoaderError;

    async fn alloc_static(
        &mut self,
        addr: u64,
        size: u64,
        readable: bool,
        writable: bool,
        executable: bool,
    ) -> Result<(), Self::LoaderError> {
        if !readable {
            return Err(ElfLoaderError::PageProtection);
        }
        if writable && executable {
            return Err(ElfLoaderError::PageProtection);
        }

        // 保护内核页
        if addr < 0x0000_0080_0000_0000 || addr.saturating_add(size) >= 0xFFFF_FF80_0000_0000 {
            return Err(ElfLoaderError::PageReserved);
        }

        let mut new_addr = addr & !0xfff;
        let mut size = size + addr - new_addr;

        // 链接器可能会将页拼到前一个页上，兼容下这种情况
        // FIXME: 这不是一个好的实现，但我们先这样做，以兼容目前的问题
        if self
            .allocated_page
            .iter()
            .any(|(allocate_addr, allocate_size)| {
                new_addr >= *allocate_addr && new_addr < *allocate_addr + *allocate_size
            })
        {
            new_addr += 0x1000;
            size = size.saturating_sub(0x1000);
        }

        if size == 0 {
            return Ok(());
        }

        let vaddr = NonZeroU64::new(new_addr).unwrap();
        let page_type = if writable {
            ProcessPageType::StaticData(vaddr)
        } else if executable {
            ProcessPageType::StaticCode(vaddr)
        } else {
            ProcessPageType::StaticConst(vaddr)
        };
        let size = (size + 0xfff) & !0xfff;
        if multitask::process::create_process_page(self.process, size as usize, page_type).is_none()
        {
            return Err(ElfLoaderError::AllocFail);
        }

        self.allocated_page.push((new_addr, size));

        Ok(())
    }

    async fn clear_memory(&mut self, addr: u64, len: u64) -> Result<(), Self::LoaderError> {
        // 保护内核页
        if addr < 0x0000_0080_0000_0000 || addr.saturating_add(len) >= 0xFFFF_FF80_0000_0000 {
            return Err(ElfLoaderError::PageReserved);
        }

        unsafe {
            multitask::process::write_user_process_memory_bytes(
                self.process,
                addr,
                0,
                len as usize,
            )?;
        }

        Ok(())
    }

    async fn write_to_memory(&mut self, addr: u64, data: &[u8]) -> Result<(), Self::LoaderError> {
        // 保护内核页
        if addr < 0x0000_0080_0000_0000
            || addr.saturating_add(data.len() as u64) >= 0xFFFF_FF80_0000_0000
        {
            return Err(ElfLoaderError::PageReserved);
        }

        unsafe {
            multitask::process::write_user_process_memory(
                self.process,
                addr,
                data.as_ptr(),
                data.len(),
            )?;
        }

        Ok(())
    }
}
