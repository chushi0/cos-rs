use core::num::NonZeroU64;

use crate::{
    kprintln,
    multitask::{
        self,
        process::{ProcessMemoryError, ProcessPageType},
    },
};

pub struct ElfLoader {
    pub process_id: u64,
}

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

impl elf::Loader for ElfLoader {
    type LoaderError = ElfLoaderError;

    async fn alloc_static(
        &mut self,
        addr: u64,
        size: u64,
        readable: bool,
        writable: bool,
        executable: bool,
    ) -> Result<(), Self::LoaderError> {
        kprintln!("alloc static");
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

        let vaddr = NonZeroU64::new(addr).unwrap();
        let page_type = if writable {
            ProcessPageType::StaticData(vaddr)
        } else if executable {
            ProcessPageType::StaticCode(vaddr)
        } else {
            ProcessPageType::StaticConst(vaddr)
        };
        let size = (size + 0xfff) & !0xfff;
        if multitask::process::create_process_page(self.process_id, size as usize, page_type)
            .is_none()
        {
            return Err(ElfLoaderError::AllocFail);
        }

        Ok(())
    }

    async fn clear_memory(&mut self, addr: u64, len: u64) -> Result<(), Self::LoaderError> {
        kprintln!("clear memory");
        // 保护内核页
        if addr < 0x0000_0080_0000_0000 || addr.saturating_add(len) >= 0xFFFF_FF80_0000_0000 {
            return Err(ElfLoaderError::PageReserved);
        }

        unsafe {
            multitask::process::write_user_process_memory_bytes(
                self.process_id,
                addr,
                0,
                len as usize,
            )?;
        }

        Ok(())
    }

    async fn write_to_memory(&mut self, addr: u64, data: &[u8]) -> Result<(), Self::LoaderError> {
        kprintln!("write to memory");
        // 保护内核页
        if addr < 0x0000_0080_0000_0000
            || addr.saturating_add(data.len() as u64) >= 0xFFFF_FF80_0000_0000
        {
            return Err(ElfLoaderError::PageReserved);
        }

        unsafe {
            multitask::process::write_user_process_memory(
                self.process_id,
                addr,
                data.as_ptr(),
                data.len(),
            )?;
        }

        Ok(())
    }
}
