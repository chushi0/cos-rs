#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use async_io::{AsyncRead, AsyncReadExt, ReadExactError, Seekable};

pub struct ElfFile<Io> {
    io: Io,
    header: ElfHeader,
    program: Vec<ElfProgram>,
}

#[derive(Debug)]
pub struct ElfHeader {
    pub bits: u8,         // 位数
    pub endian: u8,       // 大小端
    pub abi: u8,          // ABI
    pub elf_type: u16,    // 可重定位/可执行/共享/核心
    pub instruction: u16, // 指令集
    pub entry_point: u64, // 入口点
}

pub struct ElfProgram {
    program_type: u32, // 类型，1LOAD/2DYNAMIC/3INTERP/4NOTE
    flag: u32,         // 标志，1X/2W/3R
    p_offset: u64,
    p_vaddr: u64,
    p_filesz: u64,
    p_memsz: u64,
}

// 读取ELF错误
pub enum ElfReadError<RE, SE> {
    // 在读取时发生错误
    Read(ReadExactError<RE>),
    // 在移动指针时发生错误
    Seek(SE),
    // 格式错误，无法识别的ELF文件
    Format,
    // 不支持的ELF文件
    Unsupport,
}

// 加载ELF错误
#[derive(Debug)]
pub enum ElfLoadError<RE, SE, LE> {
    // 在读取时发生错误
    Read(ReadExactError<RE>),
    // 在移动指针时发生错误
    Seek(SE),
    // 在加载到内存时发生错误
    Load(LE),
}

pub trait Loader {
    type LoaderError;

    fn alloc_static(
        &mut self,
        addr: u64,
        size: u64,
        readable: bool,
        writable: bool,
        executable: bool,
    ) -> impl Future<Output = Result<(), Self::LoaderError>> + Send;

    fn write_to_memory(
        &mut self,
        addr: u64,
        data: &[u8],
    ) -> impl Future<Output = Result<(), Self::LoaderError>> + Send;

    fn clear_memory(
        &mut self,
        addr: u64,
        len: u64,
    ) -> impl Future<Output = Result<(), Self::LoaderError>> + Send;
}

impl<Io> ElfFile<Io>
where
    Io: Seekable + AsyncRead + Send,
{
    pub async fn from_io(mut io: Io) -> Result<Self, ElfReadError<Io::ReadError, Io::SeekError>> {
        io.seek(0).await.map_err(ElfReadError::Seek)?;

        let mut header_buffer = [0u8; 64];
        io.read_exact(&mut header_buffer)
            .await
            .map_err(ElfReadError::Read)?;

        if header_buffer[0..4] != [0x7f, b'E', b'L', b'F'] {
            return Err(ElfReadError::Format);
        }
        let bits = header_buffer[4];
        let endian = header_buffer[5];
        let header_version = header_buffer[6];
        let abi = header_buffer[7];
        let elf_type = u16::from_le_bytes(header_buffer[16..18].try_into().unwrap());
        let instruction = u16::from_le_bytes(header_buffer[18..20].try_into().unwrap());
        let elf_version = u32::from_le_bytes(header_buffer[20..24].try_into().unwrap());
        let entry_point = u64::from_le_bytes(header_buffer[24..32].try_into().unwrap());
        let program_offset = u64::from_le_bytes(header_buffer[32..40].try_into().unwrap());
        let program_size = u16::from_le_bytes(header_buffer[54..56].try_into().unwrap());
        let program_entry_count = u16::from_le_bytes(header_buffer[56..58].try_into().unwrap());

        if bits != 2 {
            if bits == 1 {
                return Err(ElfReadError::Unsupport);
            }
            return Err(ElfReadError::Format);
        }
        if endian != 1 {
            if endian == 2 {
                return Err(ElfReadError::Unsupport);
            }
            return Err(ElfReadError::Format);
        }
        if header_version != 1 {
            return Err(ElfReadError::Unsupport);
        }
        if abi != 0 {
            return Err(ElfReadError::Unsupport);
        }
        if elf_type != 2 {
            return Err(ElfReadError::Unsupport);
        }
        if instruction != 0x3e {
            return Err(ElfReadError::Unsupport);
        }
        if elf_version != 1 {
            return Err(ElfReadError::Unsupport);
        }
        if program_size != 56 {
            return Err(ElfReadError::Unsupport);
        }

        io.seek(program_offset).await.map_err(ElfReadError::Seek)?;
        let mut header_buffer = [0u8; 56];
        let mut program = Vec::with_capacity(program_entry_count as usize);
        for _ in 0..program_entry_count {
            io.read_exact(&mut header_buffer)
                .await
                .map_err(ElfReadError::Read)?;
            let program_type = u32::from_le_bytes(header_buffer[0..4].try_into().unwrap());
            let flag = u32::from_le_bytes(header_buffer[4..8].try_into().unwrap());
            let p_offset = u64::from_le_bytes(header_buffer[8..16].try_into().unwrap());
            let p_vaddr = u64::from_le_bytes(header_buffer[16..24].try_into().unwrap());
            let p_filesz = u64::from_le_bytes(header_buffer[32..40].try_into().unwrap());
            let p_memsz = u64::from_le_bytes(header_buffer[40..48].try_into().unwrap());
            let align = u64::from_le_bytes(header_buffer[48..56].try_into().unwrap());

            if p_filesz > p_memsz {
                return Err(ElfReadError::Format);
            }
            if align > 0x1000 {
                return Err(ElfReadError::Unsupport);
            }

            program.push(ElfProgram {
                program_type,
                flag,
                p_offset,
                p_vaddr,
                p_filesz,
                p_memsz,
            });
        }

        Ok(Self {
            io,
            header: ElfHeader {
                bits,
                endian,
                abi,
                elf_type,
                instruction,
                entry_point,
            },
            program,
        })
    }

    pub fn header(&self) -> &ElfHeader {
        &self.header
    }

    pub async fn load<L: Loader + Send>(
        &mut self,
        loader: &mut L,
    ) -> Result<(), ElfLoadError<Io::ReadError, Io::SeekError, L::LoaderError>> {
        let mut buf = [0u8; 512];
        for program in &self.program {
            if program.program_type != 1 {
                continue;
            }

            let readable = (program.flag & 4) != 0;
            let writable = (program.flag & 2) != 0;
            let executable = (program.flag & 1) != 0;

            loader
                .alloc_static(
                    program.p_vaddr,
                    program.p_memsz,
                    readable,
                    writable,
                    executable,
                )
                .await
                .map_err(ElfLoadError::Load)?;
            self.io
                .seek(program.p_offset)
                .await
                .map_err(ElfLoadError::Seek)?;
            let mut addr = program.p_vaddr;
            let mut remain = program.p_filesz;
            while remain > 0 {
                let step = buf.len().min(remain as usize);
                self.io
                    .read_exact(&mut buf[0..step])
                    .await
                    .map_err(ElfLoadError::Read)?;
                loader
                    .write_to_memory(addr, &buf[0..step])
                    .await
                    .map_err(ElfLoadError::Load)?;

                addr += step as u64;
                remain -= step as u64;
            }

            if program.p_filesz < program.p_memsz {
                loader
                    .clear_memory(
                        program.p_vaddr + program.p_filesz,
                        program.p_memsz - program.p_filesz,
                    )
                    .await
                    .map_err(ElfLoadError::Load)?;
            }
        }

        Ok(())
    }
}
