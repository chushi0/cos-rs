use core::{
    mem::ManuallyDrop,
    ptr::{copy_nonoverlapping, read_unaligned, write_unaligned},
    slice, u16,
};

use alloc::{
    boxed::Box,
    collections::BTreeSet,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use async_locks::rwlock::RwLock;

use crate::{
    BoxFuture,
    device::{BlockDevice, BlockDeviceError},
    fs::{FileHandle, FileMetadata, FileSystem, FileSystemError},
    internal::DiskStruct,
    path::Path,
};

/// FAT32文件系统实现
///
/// FAT32格式假设整个磁盘没有分区，包含了系统引导部分。如果磁盘有分区，引导区代码可以提供给BIOS引导。
/// 本实现没有处理引导代码部分，仅实现了文件相关内容。
///
/// FAT32的存储分为三部分：
///
/// - 引导区。
///     
///     该区包含引导代码和元数据。[`BPB`] 结构定定义了第一个扇区中的内容。由于BIOS引导时，会将代码复制到0x7c00并从扇区开始执行代码，
///     因此 [`BPB`] 以3字节的跳转指令开头，以便跳转到引导代码部分。元数据区还包括一个可选的[`FSInfo`]，通常位于第二扇区，
///     用于存储文件系统的统计信息，如空闲簇数等，系统无需扫描整个硬盘即可获取这些数据。
///
///     此外，引导区还可以包含任意数量的保留扇区。这些扇区通常用于引导用途，数量被保存在[`BPB::reserved_sector_count`]字段中。
///     该字段表示为整个引导区所占用的扇区数，包括[`BPB`]和[`FSInfo`]。因此其值通常至少为2。
///
///     [`BPB::sectors_per_cluster`]是一个重要字段，表示每簇包含的扇区数。在数据区中，我们始终使用簇为单位管理数据，
///     此值标定了簇的大小。
///
/// - FAT区。
///
///     该区域为bitmap和链表的结合体，表示了各个簇的使用情况和链接关系。通常有两个FAT表，互为备份，以防止在遇到坏块时丢失数据。
///     
///     FAT表是紧凑排列的u32数组，每个元素表示簇的使用情况。当值为[`FatEntry::FAT_ENTRY_FREE`]时，表示此簇处于空闲状态，
///     当值小于[`FatEntry::FAT_ENTRY_RESERVED_START`]时，表示此簇被占用，且值为下一个簇的索引，形成链表。
///     当值在[`FatEntry::FAT_ENTRY_EOC_START`]到[`FatEntry::FAT_ENTRY_EOC_END`]范围内时，表示此簇为链表的结尾。
///     其他值如[`FatEntry::FAT_ENTRY_BAD_CLUSTER`]表示坏簇，[`FatEntry::FAT_ENTRY_RESERVED_START`]及以上的值为保留值，不应被使用。
///
///     在数据区，簇从2开始编号，因此FAT表中，0号簇和1号簇被保留。
///
/// - 数据区。
///
///     该区域包含目录结构和文件内容。根目录所在的簇由[`BPB::root_cluster`]字段指定，其内容为目录项的集合。
///     目录项必须有一个短文件名条目[`DirectoryEntryShort`]，在其之前可以有多个长文件名条目[`DirectoryEntryLong`]进行扩展。
///     目录项中包含了下一层目录或文件的元数据。对于目录，它会指向下一个簇的位置。对于文件，它会指向文件起始簇的位置。
///
///     数据区的文件会直接存储其二进制数据。
///
/// 此结构提供了 [`Fat32FileSystem::mount`] 和 [`Fat32FileSystem::with_format`] 方法，
/// 分别用于挂载已有的FAT32文件系统和格式化一个块设备为FAT32文件系统。
pub struct Fat32FileSystem {
    inner: Arc<RwLock<Fat32Inner>>,
}

struct Fat32Inner {
    device: Arc<dyn BlockDevice>,
    bpb: Box<BPB>,
    fs_info: Option<Box<FSInfo>>,
    max_cluster: u32,             // 磁盘能容纳的最大簇数，不包含前两个虚拟簇
    occupied_file: BTreeSet<u32>, // 正在占用的文件，记录的是起始簇号
}

/// 引导记录，固定为第一个扇区
#[repr(C, packed)]
struct BPB {
    reserved_code: [u8; 3],     // 保留，通常为跳转指令
    oem_id: [u8; 8],            // OEM标识，如果字符串少于8字节，用空格填充
    bytes_per_sector: u16,      // 每扇区字节数
    sectors_per_cluster: u8,    // 每簇扇区数
    reserved_sector_count: u16, // 保留扇区数，包含引导记录扇区
    num_fats: u8,               // FAT表数量，通常为2
    root_entry_count: u16,      // 根目录项数（FAT12/16使用，FAT32为0）
    total_sectors_16: u16,      // 总扇区数（如果为0，则使用total_sectors_32）
    media: u8,                  // 媒体描述符
    fat_size_16: u16,           // 每个FAT表的扇区数（FAT12/16使用，FAT32为0）
    sectors_per_track: u16,     // 每磁道扇区数
    num_heads: u16,             // 磁头数
    hidden_sectors: u32,        // 隐藏扇区数
    total_sectors_32: u32,      // 总扇区数（如果total_sectors_16为0，则使用此字段）
    fat_size_32: u32,           // 每个FAT表的扇区数（FAT32使用）
    ext_flags: u16,             // 扩展标志
    fs_version: u16,            // 文件系统版本
    root_cluster: u32,          // 根目录起始簇号
    fs_info: u16,               // FSInfo结构所在扇区号
    bk_boot_sector: u16,        // 备份引导扇区号
    reserved: [u8; 12],         // 保留，格式化时，此值为0
    drive_number: u8,           // 驱动器号，软盘为0x00，硬盘为0x80
    reserved1: u8,              // 保留
    boot_signature: u8,         // 扩展引导签名（0x28或0x29），0x29表示有卷序列号和卷标字符串
    volume_id: u32,             // 卷序列号
    volume_label: [u8; 11],     // 卷标字符串，用空格填充
    fs_type: [u8; 8],           // 文件系统类型字符串，始终为FAT32
    code: [u8; 420],            // 引导代码
    boot_sector_signature: u16, // 引导扇区签名（0x55AA）
}

/// FSInfo结构，通常为第二个扇区
#[repr(C, packed)]
struct FSInfo {
    lead_signature: u32,     // 主签名，固定为0x41615252
    reserved1: [u8; 480],    // 保留，格式化时，此值为0
    struct_signature: u32,   // 结构签名，固定为0x61417272
    free_cluster_count: u32, // 空闲簇数，如果未知，则为0xFFFFFFFF
    next_free_cluster: u32,  // 下一个可用簇号的建议值，如果未知，则为0xFFFFFFFF
    reserved2: [u8; 12],     // 保留，格式化时，此值为0
    trail_signature: u32,    // 尾部签名，固定为0xAA55
}

/// FAT表项
struct FatEntry(u32);

/// 文件目录表项（短文件名）
#[repr(C, packed)]
#[derive(Debug, Clone)]
struct DirectoryEntryShort {
    name: [u8; 8],           // 文件名，空格填充
    ext: [u8; 3],            // 文件扩展名，空格填充
    attr: u8,                // 文件属性
    reserved: u8,            // 保留
    create_time_tenths: u8, // 创建时间的百分之一秒（微软FAT规范为十分之一秒，但Ubuntu16.10存储0或100，Windows7存储0-199）
    create_time: u16,       // 创建时间，将秒数乘以2（小时5位，分钟6位，秒5位）
    create_date: u16,       // 创建日期（年7位，月4位，日5位）
    last_access_date: u16,  // 最后访问日期，格式与创建日期相同
    first_cluster_high: u16, // 首簇号高16位
    write_time: u16,        // 最后修改时间，格式与创建时间相同
    write_date: u16,        // 最后修改日期，格式与创建日期相同
    first_cluster_low: u16, // 首簇号低16位
    file_size: u32,         // 文件大小，单位为字节
}

/// 文件目录表项（长文件名）
#[repr(C, packed)]
#[derive(Debug, Clone)]
struct DirectoryEntryLong {
    order: u8,                                   // 顺序号，最后一个条目的最高位为1
    name1: [u16; DirectoryEntryLong::NAME1_LEN], // 文件名字符1（前5个UTF-16字符）
    attr: u8,                                    // 文件属性，固定为0x0F
    type_: u8,                                   // 类型，保留，格式化时为0
    checksum: u8,                                // 短文件名校验和
    name2: [u16; DirectoryEntryLong::NAME2_LEN], // 文件名字符2（中间6个UTF-16字符）
    first_cluster_low: u16,                      // 首簇号低16位，始终为0
    name3: [u16; DirectoryEntryLong::NAME3_LEN], // 文件名字符3（最后2个UTF-16字符）
}

#[repr(C, packed)]
union DirectoryEntry {
    short: ManuallyDrop<DirectoryEntryShort>,
    long: ManuallyDrop<DirectoryEntryLong>,
}

const _: () = {
    assert!(size_of::<BPB>() == 512);
    assert!(size_of::<FSInfo>() == 512);
    assert!(size_of::<FatEntry>() == 4);
    assert!(size_of::<DirectoryEntryShort>() == 32);
    assert!(size_of::<DirectoryEntryLong>() == 32);
    assert!(size_of::<DirectoryEntry>() == 32);
};

impl BPB {
    const RESERVED_CODE: [u8; 3] = [0xeb, 0xfe, 0x90];
    const OEM_ID: [u8; 8] = *b"cosfs1.0";
    const FS_TYPE: [u8; 8] = *b"FAT32   ";
    const BOOT_SECTOR_SIGNATURE: u16 = 0xAA55;
}

impl FSInfo {
    const LEAD_SIGNATURE: u32 = 0x41615252;
    const STRUCT_SIGNATURE: u32 = 0x61417272;
    const TRAIL_SIGNATURE: u32 = 0xAA550000;
}

impl FatEntry {
    const FAT_ENTRY_FREE: u32 = 0x00000000;
    const FAT_ENTRY_RESERVED_START: u32 = 0x0FFFFFF0;
    // const FAT_ENTRY_BAD_CLUSTER: u32 = 0x0FFFFFF7;
    const FAT_ENTRY_EOC_START: u32 = 0x0FFFFFF8;
    // const FAT_ENTRY_EOC_END: u32 = 0x0FFFFFFF;
}

impl DirectoryEntryShort {
    const ATTR_READ_ONLY: u8 = 0x01;
    const ATTR_HIDDEN: u8 = 0x02;
    const ATTR_SYSTEM: u8 = 0x04;
    const ATTR_VOLUME_ID: u8 = 0x08;
    const ATTR_DIRECTORY: u8 = 0x10;
    const ATTR_ARCHIVE: u8 = 0x20;
    const ATTR_LONG_NAME: u8 =
        Self::ATTR_READ_ONLY | Self::ATTR_HIDDEN | Self::ATTR_SYSTEM | Self::ATTR_VOLUME_ID;
}

impl DirectoryEntryLong {
    const ATTR: u8 = DirectoryEntryShort::ATTR_LONG_NAME;
    const NAME1_LEN: usize = 5;
    const NAME2_LEN: usize = 6;
    const NAME3_LEN: usize = 2;
}

#[derive(Debug)]
pub enum MountError {
    IoError(BlockDeviceError),
    InvalidFormat,
}

impl From<BlockDeviceError> for MountError {
    fn from(value: BlockDeviceError) -> Self {
        Self::IoError(value)
    }
}

#[derive(Debug)]
pub enum FormatError {
    IoError(BlockDeviceError),
    DeviceTooSmall,
}

impl From<BlockDeviceError> for FormatError {
    fn from(value: BlockDeviceError) -> Self {
        Self::IoError(value)
    }
}

impl Fat32FileSystem {
    pub async fn mount(device: Arc<dyn BlockDevice>) -> Result<Self, MountError> {
        // 对磁盘容量和扇区大小进行检查
        let block_size = device.block_size();
        if block_size < 512 {
            return Err(MountError::InvalidFormat);
        }
        let block_count = device.block_count();
        if block_count < 2 {
            return Err(MountError::InvalidFormat);
        }

        // 读取引导扇区
        // Safety: BPB是#[repr(C, packed)]，没有实现Drop，且不会产生无效值的类型
        let bpb = unsafe {
            let mut bpb = DiskStruct::new(block_size as usize);
            device.read_block(0, bpb.as_slice_mut()).await?;
            Box::new(bpb.into_inner())
        };
        // 检查bpb数据
        check_bpb(&bpb, block_size, block_count)?;

        // 如果FSInfo结构存在，也读取FSInfo
        let fs_info = if bpb.fs_info < bpb.reserved_sector_count {
            // Safety: FSInfo是#[repr(C, packed)]，没有实现Drop，且不会产生无效值的类型
            unsafe {
                let mut fs_info = DiskStruct::new(block_size as usize);
                device
                    .read_block(bpb.fs_info as u64, fs_info.as_slice_mut())
                    .await?;
                Some(Box::new(fs_info.into_inner()))
            }
        } else {
            None
        };
        // 检查fs_info数据
        if let Some(fs_info) = &fs_info {
            check_fs_info(fs_info)?;
        }

        let max_cluster = {
            let fat_limit = (bpb.fat_size_32 as usize * bpb.bytes_per_sector as usize
                / size_of::<FatEntry>()) as u64
                - 2;
            let device_limit = (device.block_count()
                - bpb.reserved_sector_count as u64
                - bpb.fat_size_32 as u64 * bpb.num_fats as u64)
                / bpb.sectors_per_cluster as u64;
            fat_limit.min(device_limit) as u32
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(Fat32Inner {
                device,
                bpb,
                fs_info,
                max_cluster,
                occupied_file: BTreeSet::new(),
            })),
        })
    }

    pub async fn with_format(device: Arc<dyn BlockDevice>) -> Result<Self, FormatError> {
        // 对磁盘容量和扇区大小进行检查
        let block_size = device.block_size();
        if block_size < 512 {
            return Err(FormatError::DeviceTooSmall);
        }
        let block_count = device.block_count();
        if block_count < 2 {
            return Err(FormatError::DeviceTooSmall);
        }

        // 保留2扇区
        let reserved_sector_count = 2;
        // 每扇区字节数，对齐到2的整数次幂
        let bytes_per_sector = if block_size.is_power_of_two() {
            block_size
        } else {
            1 << (63 - block_size.leading_zeros())
        };
        // 每簇占用8个扇区
        let sectors_per_cluster = 8u64;
        // 计算总簇数
        let (total_cluster_count, remain_block) = calc_cluster_count(
            reserved_sector_count,
            bytes_per_sector,
            sectors_per_cluster,
            block_count,
        );
        if total_cluster_count < 2 {
            return Err(FormatError::DeviceTooSmall);
        }
        // 剩余扇区加到保留扇区上
        let reserved_sector_count = reserved_sector_count + remain_block;
        // 需要的FAT空间
        let fat_size_32 = {
            let cluster_per_fat = bytes_per_sector / size_of::<FatEntry>() as u64;
            (total_cluster_count + cluster_per_fat - 1) / cluster_per_fat
        };

        // BPB
        let bpb = Box::new(BPB {
            reserved_code: BPB::RESERVED_CODE,
            oem_id: BPB::OEM_ID,
            bytes_per_sector: bytes_per_sector as u16,
            sectors_per_cluster: sectors_per_cluster as u8,
            reserved_sector_count: reserved_sector_count as u16,
            num_fats: 2,
            root_entry_count: 0,
            total_sectors_16: 0,
            media: 0,
            fat_size_16: 0,
            sectors_per_track: 0,
            num_heads: 0,
            hidden_sectors: 0,
            total_sectors_32: block_count as u32,
            fat_size_32: fat_size_32 as u32,
            ext_flags: 0,
            fs_version: 0,
            root_cluster: 2,
            fs_info: 1,
            bk_boot_sector: u16::MAX,
            reserved: [0; 12],
            drive_number: 0x80,
            reserved1: 0,
            boot_signature: 0x28,
            volume_id: 0,
            volume_label: [0; 11],
            fs_type: BPB::FS_TYPE,
            code: [0; 420],
            boot_sector_signature: BPB::BOOT_SECTOR_SIGNATURE,
        });

        // FSInfo
        let fs_info = Box::new(FSInfo {
            lead_signature: FSInfo::LEAD_SIGNATURE,
            reserved1: [0; 480],
            struct_signature: FSInfo::STRUCT_SIGNATURE,
            free_cluster_count: (total_cluster_count - 1) as u32,
            next_free_cluster: 3,
            reserved2: [0; 12],
            trail_signature: FSInfo::TRAIL_SIGNATURE,
        });

        // 将引导区写入磁盘
        {
            let mut buffer = alloc::vec![0u8; block_size as usize * 2];
            // Safety:
            // src和dst均有效
            // bpb、fs_info均无对齐要求
            // 内存为堆上的不同区域，没有重叠
            unsafe {
                copy_nonoverlapping(
                    bpb.as_ref() as *const BPB as *const u8,
                    buffer.as_mut_ptr(),
                    block_size as usize,
                );
                copy_nonoverlapping(
                    fs_info.as_ref() as *const FSInfo as *const u8,
                    buffer.as_mut_ptr().add(block_size as usize),
                    block_size as usize,
                );
            }
            device.write_blocks(0, 2, &buffer).await?;
        }

        // 格式化FAT区
        device
            .write_zeros(reserved_sector_count, fat_size_32 * 2)
            .await?;
        {
            let mut buffer = alloc::vec![0u8; block_size as usize];
            // Safety:
            // 写入目标位置有效
            unsafe {
                write_unaligned(
                    (buffer.as_mut_ptr() as *mut FatEntry).add(0),
                    FatEntry(0xffffff00),
                );
                write_unaligned(
                    (buffer.as_mut_ptr() as *mut FatEntry).add(1),
                    FatEntry(0xffffffff),
                );
                write_unaligned(
                    (buffer.as_mut_ptr() as *mut FatEntry).add(2),
                    FatEntry(FatEntry::FAT_ENTRY_EOC_START),
                );
            }
            device.write_block(reserved_sector_count, &buffer).await?;
            device
                .write_block(reserved_sector_count + fat_size_32, &buffer)
                .await?;
        }

        // 格式化根目录
        device
            .write_zeros(reserved_sector_count + fat_size_32 * 2, sectors_per_cluster)
            .await?;

        Ok(Self {
            inner: Arc::new(RwLock::new(Fat32Inner {
                device,
                bpb,
                fs_info: Some(fs_info),
                max_cluster: total_cluster_count as u32,
                occupied_file: BTreeSet::new(),
            })),
        })
    }
}

struct Fat32FileMetadata {
    short: DirectoryEntryShort,
    long: Vec<DirectoryEntryLong>,
    start_cluster: u32,
    start_sector: u64,
    start_sector_offset: u32,
    short_cluster: u32,
    short_sector: u64,
    short_sector_offset: u32,
}

impl Fat32FileMetadata {
    fn new(name: &str, start_cluster: u32, attr: u8) -> Self {
        debug_assert!(!name.is_empty());

        // TODO: 我们的实现是固定短名和必填长名，以简化设计
        // TODO: 同时，我们没有考虑校验和，我们假设所有校验和都是满足的

        let short = DirectoryEntryShort {
            name: *b"????????",
            ext: *b"???",
            attr,
            reserved: 0,
            create_time_tenths: 0,
            create_time: 0,
            create_date: 0,
            last_access_date: 0,
            first_cluster_high: ((start_cluster >> 16) & 0xffff) as u16,
            write_time: 0,
            write_date: 0,
            first_cluster_low: (start_cluster & 0xffff) as u16,
            file_size: 0,
        };

        let utf16 = name.encode_utf16();
        let mut long = Vec::new();
        let mut current_long = None;
        let mut i = 0;
        for ch in utf16 {
            if current_long.is_none() {
                current_long = Some(DirectoryEntryLong {
                    order: long.len() as u8,
                    name1: [0x0; DirectoryEntryLong::NAME1_LEN],
                    attr: DirectoryEntryLong::ATTR,
                    type_: 0,
                    checksum: 0,
                    name2: [0x0; DirectoryEntryLong::NAME2_LEN],
                    first_cluster_low: 0,
                    name3: [0x0; DirectoryEntryLong::NAME3_LEN],
                });
                i = 0;
            }
            if let Some(current) = &mut current_long {
                if i < DirectoryEntryLong::NAME1_LEN {
                    current.name1[i] = ch;
                } else if i < DirectoryEntryLong::NAME1_LEN + DirectoryEntryLong::NAME2_LEN {
                    current.name2[i - DirectoryEntryLong::NAME1_LEN] = ch;
                } else {
                    current.name3
                        [i - DirectoryEntryLong::NAME1_LEN - DirectoryEntryLong::NAME2_LEN] = ch;
                }
                i += 1;

                if i == DirectoryEntryLong::NAME1_LEN
                    + DirectoryEntryLong::NAME2_LEN
                    + DirectoryEntryLong::NAME3_LEN
                {
                    if let Some(current) = current_long.take() {
                        long.push(current);
                    }
                }
            }
        }
        if let Some(current) = current_long {
            long.push(current);
        }

        Self {
            short,
            long,
            start_cluster: 0,
            start_sector: 0,
            start_sector_offset: 0,
            short_cluster: 0,
            short_sector: 0,
            short_sector_offset: 0,
        }
    }

    fn start_cluster(&self) -> u32 {
        (self.short.first_cluster_low as u32) | ((self.short.first_cluster_high as u32) << 16)
    }

    fn name_to_string(&self) -> String {
        if self.long.is_empty() {
            let start = self.short.name.trim_ascii();
            let end = self.short.ext.trim_ascii();
            if end.is_empty() {
                String::from_utf8_lossy(start).to_string()
            } else {
                String::from_utf8_lossy(start).to_string() + "." + &String::from_utf8_lossy(end)
            }
        } else {
            let name_utf16 = self.long.iter().fold(Vec::new(), |mut buffer, entry| {
                let name1 = entry.name1;
                let name2 = entry.name2;
                let name3 = entry.name3;
                [&name1 as &[u16], &name2, &name3]
                    .into_iter()
                    .flat_map(|slice| slice.iter().copied())
                    .filter(|&utf16| utf16 != 0)
                    .for_each(|utf16| buffer.push(utf16));

                buffer
            });

            String::from_utf16_lossy(&name_utf16).to_string()
        }
    }
}

impl Fat32Inner {
    /// 根据文件路径，获取文件元信息
    ///
    /// 文件元信息即为目录表上的项，如果为根目录，返回Nono
    ///
    /// 如果文件不存在，返回 [`FileSystemError::FileNotFound`]
    /// 如果查找的是根目录，返回 [`None`]
    async fn get_file_metadata(
        &self,
        path: Path<'_>,
    ) -> Result<Option<Fat32FileMetadata>, FileSystemError> {
        let mut cluster = self.bpb.root_cluster;
        let mut file_metadata = None::<Fat32FileMetadata>;

        for name in path.iter() {
            if let Some(metadata) = &file_metadata {
                if (metadata.short.attr & DirectoryEntryShort::ATTR_DIRECTORY) == 0 {
                    return Err(FileSystemError::FileNotFound);
                }
            }

            let next = self.get_file_metadata_by_cluster(cluster, name).await?;
            cluster = next.start_cluster();
            file_metadata = Some(next);
        }

        Ok(file_metadata)
    }

    /// 根据簇号，遍历文件元信息
    /// yield_fn返回值指示是否应当继续遍历，true表示继续，false表示停止
    async fn walk_file_meta_by_cluster<F>(
        &self,
        cluster: u32,
        mut yield_fn: F,
    ) -> Result<(), FileSystemError>
    where
        F: FnMut(Fat32FileMetadata) -> Result<bool, FileSystemError> + Send,
    {
        let mut cluster = cluster;
        let mut entry_long = Vec::new();
        let mut start_cluster = 0;
        let mut start_sector = 0;
        let mut start_sector_offset = 0;

        // 预分配簇空间
        let mut cluster_buffer = alloc::vec![0u8; self.device.block_size() as usize * self.bpb.sectors_per_cluster as usize];

        // 循环获取每簇信息，解析并查找文件元信息
        while cluster != FatEntry::FAT_ENTRY_FREE && cluster < FatEntry::FAT_ENTRY_RESERVED_START {
            // 读盘，获取该簇的信息
            let sector = self.get_sector_by_cluster(cluster);
            self.device
                .read_blocks(
                    sector,
                    self.bpb.sectors_per_cluster as u64,
                    &mut cluster_buffer,
                )
                .await?;

            // 尽管扇区通常为512，但我们没有限制扇区必须与硬件扇区一致，所以这里cluster_buffer可能是有间隙的
            // 我们使用for循环来处理
            for i in 0..self.bpb.sectors_per_cluster as usize {
                let buffer = &cluster_buffer[i * self.device.block_size() as usize..];
                for j in 0..(self.bpb.bytes_per_sector as usize) / size_of::<DirectoryEntry>() {
                    // Safety:
                    // buffer中指定内存为从磁盘上读取的目录项数据，短文件名和长文件名有同样的长度，可以直接读取
                    let entry = unsafe {
                        read_unaligned(
                            &raw const buffer[j * size_of::<DirectoryEntry>()]
                                as *const DirectoryEntry,
                        )
                    };

                    // 检查判断当前条目为短文件名还是长文件名
                    // Safety: 不论是short还是long，attr字段的offset/size/type都是一致的，我们可以任取一个
                    let attr = unsafe { entry.short.attr };
                    if attr == DirectoryEntryLong::ATTR {
                        // 长字段先push到vec里，等全部取完后再检查文件名是否一致
                        if entry_long.is_empty() {
                            start_cluster = cluster;
                            start_sector = sector as u64 + i as u64;
                            start_sector_offset = j as u32;
                        }
                        // Safety: 我们已经通过attr确认entry为long类型
                        unsafe {
                            entry_long.push(ManuallyDrop::into_inner(entry.long));
                        }
                        continue;
                    }

                    // Safety: 我们已经通过attr确认entry为short类型或空条目，我们统一转换为short类型进行处理
                    let entry = unsafe { ManuallyDrop::into_inner(entry.short) };

                    // 判断是否为空条目
                    // 根据首字节进行判断，如果为0x00或0xe5，则为空
                    if entry.name[0] == 0x00 || entry.name[0] == 0xE5 {
                        // 清空之前的长文件名，根据规范，必须后面紧跟短文件名才能匹配
                        entry_long.clear();
                        continue;
                    }

                    // 此时我们拿到了一个完整的条目，其可能为短条目，也可能为长条目
                    // 我们将结果回调给yield_fn
                    let should_continue = yield_fn(Fat32FileMetadata {
                        short: entry,
                        long: core::mem::take(&mut entry_long),
                        start_cluster,
                        start_sector,
                        start_sector_offset,
                        short_cluster: cluster,
                        short_sector: sector as u64 + i as u64,
                        short_sector_offset: j as u32,
                    })?;
                    if !should_continue {
                        return Ok(());
                    }
                }
            }

            // 进入下一个簇继续寻找
            cluster = self.get_next_cluster(cluster).await?.0;
        }

        Ok(())
    }

    /// 根据簇号和文件名，获取文件元信息
    ///
    /// 簇号为对应目录的簇，此函数将在这个簇中寻找文件元信息
    ///
    /// 如果文件不存在，返回 [`FileSystemError::FileNotFound`]
    async fn get_file_metadata_by_cluster(
        &self,
        cluster: u32,
        name: &str,
    ) -> Result<Fat32FileMetadata, FileSystemError> {
        let mut file = None;
        self.walk_file_meta_by_cluster(cluster, |mut metadata| {
            let name_matches = if metadata.long.is_empty() {
                name_match_short(&metadata.short, name)
            } else {
                metadata.long.sort_unstable_by_key(|entry| entry.order);
                name_match_long(&metadata.long, name)
            };

            if name_matches {
                file = Some(metadata);
                Ok(false)
            } else {
                Ok(true)
            }
        })
        .await?;

        file.ok_or(FileSystemError::FileNotFound)
    }

    /// 根据簇号获取所有文件信息
    async fn get_all_filemeta_by_cluster(
        &self,
        cluster: u32,
    ) -> Result<Vec<Fat32FileMetadata>, FileSystemError> {
        let mut files = Vec::new();

        self.walk_file_meta_by_cluster(cluster, |metadata| {
            files.push(metadata);
            Ok(true)
        })
        .await?;

        Ok(files)
    }

    /// 根据簇号获取扇区号
    fn get_sector_by_cluster(&self, cluster: u32) -> u64 {
        self.bpb.reserved_sector_count as u64
            + self.bpb.fat_size_32 as u64 * 2
            + (cluster as u64 - 2) * self.bpb.sectors_per_cluster as u64
    }

    /// 根据簇号，获取下一个簇的簇号
    async fn get_next_cluster(&self, cluster: u32) -> Result<FatEntry, FileSystemError> {
        // 计算该信息所在的位置
        let cluster_per_sector =
            (self.bpb.bytes_per_sector as usize / size_of::<FatEntry>()) as u64;
        let sector_offset = (cluster as u64) / cluster_per_sector;
        let offset = (cluster as u64) % cluster_per_sector;
        let block_index = self.bpb.reserved_sector_count as u64 + sector_offset;

        // 读取该扇区
        let mut buffer = alloc::vec![0u8; self.device.block_size() as usize];
        self.device.read_block(block_index, &mut buffer).await?;

        // 获取下一个簇的簇号
        // Safety: buffer为刚刚读盘拿到的数据，我们已经计算得出FatEntry在此扇区的偏移
        let next_cluster =
            unsafe { read_unaligned((buffer.as_ptr() as *const FatEntry).add(offset as usize)) };

        Ok(next_cluster)
    }

    /// 查找可用簇
    ///
    /// 返回的是对应簇号。如果没有可用簇，返回 [`FileSystemError::DiskFull`]
    /// 此函数会同时将该簇标记为EOF
    async fn find_available_cluster(&mut self) -> Result<u32, FileSystemError> {
        // 如果当前提示已经满了，那就不要找了
        if self
            .fs_info
            .as_ref()
            .is_some_and(|fs_info| fs_info.free_cluster_count == 0)
        {
            return Err(FileSystemError::DiskFull);
        }

        // 如果有提示，我们使用提示的建议值；否则，我们从2开始扫描
        let hint = self
            .fs_info
            .as_ref()
            .map(|fs_info| fs_info.next_free_cluster);

        let mut current_cluster = hint.unwrap_or(2);
        let mut end = self.max_cluster + 2;

        let mut cache_block_index = None;
        let mut cache_buffer = alloc::vec![0u8; self.device.block_size() as usize];

        loop {
            // 扫描到end了
            if current_cluster >= end {
                // 如果是因为提示，我们没有从2开始扫描，那么我们就重新扫一次
                if let Some(hint) = hint {
                    end = hint as u32;
                    current_cluster = 2;
                    continue;
                }

                // 如果不是因为提示，那说明磁盘满了
                // 更新提示信息，然后报错
                if let Some(fs_info) = &mut self.fs_info {
                    fs_info.free_cluster_count = 0;
                    self.write_fs_info().await?;
                }
                return Err(FileSystemError::DiskFull);
            }

            // 计算簇可用信息所在的位置
            let cluster_per_sector =
                (self.bpb.bytes_per_sector as usize / size_of::<FatEntry>()) as u64;
            let sector_offset = (current_cluster as u64) / cluster_per_sector;
            let offset = (current_cluster as u64) % cluster_per_sector;
            let block_index = self.bpb.reserved_sector_count as u64 + sector_offset;

            // 如果缓存可用，使用缓存，否则进行读盘
            if cache_block_index != Some(block_index) {
                cache_block_index = Some(block_index);
                self.device
                    .read_block(block_index, &mut cache_buffer)
                    .await?;
            }
            let fat_entry = unsafe {
                read_unaligned((cache_buffer.as_ptr() as *const FatEntry).add(offset as usize))
            };

            // 如果该簇不空闲，则继续检查下一个
            if fat_entry.0 != FatEntry::FAT_ENTRY_FREE {
                current_cluster += 1;
                continue;
            }

            // 该簇空闲，写入EOF，并返回簇
            unsafe {
                write_unaligned(
                    (cache_buffer.as_ptr() as *mut FatEntry).add(offset as usize),
                    FatEntry(FatEntry::FAT_ENTRY_EOC_START),
                );
            }
            for i in 0..self.bpb.num_fats {
                self.device
                    .write_block(
                        block_index + i as u64 * self.bpb.fat_size_32 as u64,
                        &cache_buffer,
                    )
                    .await?;
            }

            if let Some(fs_info) = &mut self.fs_info {
                fs_info.free_cluster_count -= 1;
                self.write_fs_info().await?;
            }

            return Ok(current_cluster);
        }
    }

    /// 归还使用完毕的簇
    async fn free_cluster(&mut self, cluster: u32) -> Result<(), FileSystemError> {
        self.update_cluster(cluster, FatEntry(FatEntry::FAT_ENTRY_FREE))
            .await?;

        if let Some(fs_info) = &mut self.fs_info {
            fs_info.free_cluster_count += 1;
            self.write_fs_info().await?;
        }

        Ok(())
    }

    /// 更新簇在FAT表中的信息
    async fn update_cluster(
        &mut self,
        cluster: u32,
        entry: FatEntry,
    ) -> Result<(), FileSystemError> {
        // 计算该信息所在的位置
        let cluster_per_sector =
            (self.bpb.bytes_per_sector as usize / size_of::<FatEntry>()) as u64;
        let sector_offset = (cluster as u64) / cluster_per_sector;
        let offset = (cluster as u64) % cluster_per_sector;
        let block_index = self.bpb.reserved_sector_count as u64 + sector_offset;

        // 读取该扇区
        let mut buffer = alloc::vec![0u8; self.device.block_size() as usize];
        self.device.read_block(block_index, &mut buffer).await?;

        // 更新
        // Safety: buffer为刚刚读盘拿到的数据，我们已经计算得出FatEntry在此扇区的偏移
        unsafe {
            write_unaligned(
                (buffer.as_ptr() as *mut FatEntry).add(offset as usize),
                entry,
            );
        };

        // 写回扇区，包含备份扇区
        for i in 0..self.bpb.num_fats {
            self.device
                .write_block(
                    block_index + self.bpb.fat_size_32 as u64 * i as u64,
                    &buffer,
                )
                .await?;
        }

        Ok(())
    }

    /// 将fs_info同步到磁盘
    async fn write_fs_info(&self) -> Result<(), FileSystemError> {
        if let Some(fs_info) = &self.fs_info {
            let mut buffer = alloc::vec![0u8; self.device.block_size() as usize];
            unsafe {
                copy_nonoverlapping(
                    fs_info.as_ref() as *const FSInfo as *const u8,
                    buffer.as_mut_ptr(),
                    size_of::<FSInfo>(),
                );
            }
            let block_index = self.bpb.fs_info as u64;
            self.device.write_block(block_index, &buffer).await?;
        }
        Ok(())
    }

    /// 在文件目录中创建文件或目录
    async fn create_file_meta(
        &mut self,
        directory_cluster: u32,
        to_create: &Fat32FileMetadata,
    ) -> Result<(), FileSystemError> {
        // 条目不能太多，否则我们下面的实现可能找不到合适的位置写入
        if to_create.long.len() + 1
            > self.bpb.bytes_per_sector as usize / size_of::<DirectoryEntry>()
        {
            return Err(FileSystemError::NameTooLang);
        }

        // 缓存值，因为device是dyn的，编译器不会自动优化
        let block_size = self.device.block_size() as usize;

        // 遍历空余位置，插入我们的文件目录
        let mut last_cluster = 0;
        let mut cluster = directory_cluster;
        let mut buffer = alloc::vec![0u8; self.bpb.sectors_per_cluster as usize * block_size];
        while cluster != FatEntry::FAT_ENTRY_FREE && cluster < FatEntry::FAT_ENTRY_RESERVED_START {
            let block_index = self.get_sector_by_cluster(cluster);
            // 读取这个簇
            self.device
                .read_blocks(
                    block_index,
                    self.bpb.sectors_per_cluster as u64,
                    &mut buffer,
                )
                .await?;

            // 逐扇区搜索，因为我们的实现没有要求fat32使用的扇区与硬件扇区一致，中间可能有碎片
            for sector in 0..self.bpb.sectors_per_cluster as usize {
                // 将内容转为目录
                // Safety:
                // 1. buffer是我们刚刚读取的数据，其内容有效
                // 2. 我们计算的长度是一个扇区所能容纳的目录数量，不会越界
                // 3. DirectoryEntry无需对齐
                let directory_entries = unsafe {
                    slice::from_raw_parts_mut(
                        buffer.as_mut_ptr().add(sector * block_size) as *mut DirectoryEntry,
                        self.bpb.bytes_per_sector as usize / size_of::<DirectoryEntry>(),
                    )
                };

                // TODO: 我们简单地实现为：只看单一的扇区是否能容纳目录，如果不能容纳目录，我们就跳过这个扇区，看下一个扇区
                // 虽然跨扇区甚至跨簇是可以创建的，但为了简化，我们暂时不考虑
                // 这样我们可以使用双指针检查是否可以插入
                // TODO: 此外，我们也没有在此检查文件是否已经存在，通常文件系统不允许出现同名文件
                let mut start = 0;
                let need = to_create.long.len() + 1;
                for i in 0..directory_entries.len() {
                    // 判断当前位置是否为空
                    // Safety: 只访问首字节判断是否为空的情况下，DirectoryEntry的两个variant是一致的含义
                    let is_free = unsafe { directory_entries[i].short.name[0] == 0 };

                    // 如果当前entry非空，则重置start
                    if !is_free {
                        start = i + 1;
                        continue;
                    }

                    // 检查是否已经足够写入
                    if i - start + 1 >= need {
                        // 写入
                        for j in 0..need - 1 {
                            directory_entries[start + j].long =
                                ManuallyDrop::new(to_create.long[j].clone());
                        }
                        directory_entries[start + need - 1].short =
                            ManuallyDrop::new(to_create.short.clone());

                        // 将此扇区内容写入
                        self.device
                            .write_block(
                                block_index + sector as u64,
                                &buffer[sector * block_size..(sector + 1) * block_size],
                            )
                            .await?;

                        // 返回
                        return Ok(());
                    }
                }
            }

            // 查看下一个簇
            last_cluster = cluster;
            cluster = self.get_next_cluster(cluster).await?.0;
        }

        // 我们再找一个新的簇
        cluster = self.find_available_cluster().await?;

        // 清空这个簇
        self.device
            .write_zeros(
                self.get_sector_by_cluster(cluster),
                self.bpb.sectors_per_cluster as u64,
            )
            .await?;

        // 然后把扇区写进去
        buffer = alloc::vec![0u8; block_size];
        // Safety:
        // 1. buffer是我们刚刚读取的数据，其内容有效
        // 2. 我们计算的长度是一个扇区所能容纳的目录数量，不会越界
        // 3. DirectoryEntry无需对齐
        let directory_entries = unsafe {
            slice::from_raw_parts_mut(
                buffer.as_mut_ptr() as *mut DirectoryEntry,
                self.bpb.bytes_per_sector as usize / size_of::<DirectoryEntry>(),
            )
        };
        // 写入
        for j in 0..to_create.long.len() {
            directory_entries[j].long = ManuallyDrop::new(to_create.long[j].clone());
        }
        directory_entries[to_create.long.len()].short = ManuallyDrop::new(to_create.short.clone());

        // 将此扇区内容写入
        self.device
            .write_block(self.get_sector_by_cluster(cluster), &buffer)
            .await?;

        // 更新文件目录
        self.update_cluster(last_cluster, FatEntry(cluster)).await?;

        Ok(())
    }

    /// 将 [`Fat32FileMetadata`] 转为 [`FileMetadata`]
    async fn get_fs_metadata(
        &self,
        file: &Fat32FileMetadata,
    ) -> Result<FileMetadata, FileSystemError> {
        let name = file.name_to_string();
        let allocated_size = self.get_allocated_size(file.start_cluster()).await?;

        Ok(FileMetadata {
            name,
            size: file.short.file_size as u64,
            is_directory: (file.short.attr & DirectoryEntryShort::ATTR_DIRECTORY) != 0,
            allocated_size,
        })
    }

    /// 获取整个簇链的占用空间
    ///
    /// 返回的是以字节为单位的总大小
    async fn get_allocated_size(&self, mut cluster: u32) -> Result<u64, FileSystemError> {
        let mut cluster_count = 0;
        while cluster != FatEntry::FAT_ENTRY_FREE && cluster < FatEntry::FAT_ENTRY_RESERVED_START {
            cluster_count += 1;
            cluster = self.get_next_cluster(cluster).await?.0;
        }

        Ok(cluster_count * self.bpb.bytes_per_sector as u64 * self.bpb.sectors_per_cluster as u64)
    }

    /// 删除文件条目
    ///
    /// 此函数仅删除条目，不删除占用的簇信息
    async fn delete_file_meta(
        &mut self,
        metadata: &Fat32FileMetadata,
    ) -> Result<(), FileSystemError> {
        // metadata 中包含了条目所在位置，可以直接删除
        // 删除前需要同步把之前的长条目也删除

        let block_size = self.device.block_size();
        let mut cluster = metadata.start_cluster;
        let mut cluster_buffer =
            alloc::vec![0u8; block_size as usize * self.bpb.sectors_per_cluster as usize];
        let mut loop_finish = false;

        // 循环各簇
        while cluster != FatEntry::FAT_ENTRY_FREE && cluster < FatEntry::FAT_ENTRY_EOC_START {
            // 读盘
            let sector = self.get_sector_by_cluster(cluster);
            self.device
                .read_blocks(
                    sector,
                    self.bpb.sectors_per_cluster as u64,
                    &mut cluster_buffer,
                )
                .await?;

            // 循环各扇区
            let sector_start = if cluster == metadata.start_cluster {
                (metadata.start_sector - sector) as usize
            } else {
                0
            };
            'sector_loop: for i in sector_start..self.bpb.sectors_per_cluster as usize {
                let buffer = &mut cluster_buffer[i * block_size as usize..];

                // 循环各条目
                let offset_start = if i as u64 + sector == metadata.short_sector {
                    metadata.start_sector_offset as usize
                } else {
                    0
                };
                for j in
                    offset_start..(self.bpb.bytes_per_sector as usize) / size_of::<DirectoryEntry>()
                {
                    // 清空此条目
                    buffer[j * size_of::<DirectoryEntry>()..(j + 1) * size_of::<DirectoryEntry>()]
                        .fill(0);

                    // 如果达到终止条件，退出循环
                    if cluster == metadata.short_cluster
                        && sector + i as u64 == metadata.short_sector
                        && j as u32 == metadata.short_sector_offset
                    {
                        loop_finish = true;
                        break 'sector_loop;
                    }
                }
            }

            // 存盘
            self.device
                .write_blocks(sector, self.bpb.sectors_per_cluster as u64, &cluster_buffer)
                .await?;

            if loop_finish {
                break;
            }

            cluster = self.get_next_cluster(cluster).await?.0;
        }

        Ok(())
    }
}

impl FileSystem for Fat32FileSystem {
    fn total_space(&self) -> BoxFuture<'_, Result<u64, FileSystemError>> {
        Box::pin(async {
            let inner = self.inner.read().await;
            Ok(inner.max_cluster as u64
                * inner.bpb.bytes_per_sector as u64
                * inner.bpb.sectors_per_cluster as u64)
        })
    }

    fn free_space(&self) -> BoxFuture<'_, Result<u64, FileSystemError>> {
        Box::pin(async {
            let inner = self.inner.read().await;

            // TODO: 我们应该自己单独维护剩余空间，而非完全依赖fs_info
            Ok(inner
                .fs_info
                .as_ref()
                .map(|fs_info| fs_info.free_cluster_count as u64)
                .unwrap_or(0)
                * inner.bpb.bytes_per_sector as u64
                * inner.bpb.sectors_per_cluster as u64)
        })
    }

    fn create_file<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>> {
        Box::pin(async move {
            // 文件名
            let Some(name) = path.last_segment() else {
                return Err(FileSystemError::FileExists);
            };

            let mut inner = self.inner.write().await;
            // 父文件目录（可能为根目录）
            let directory_metadata = inner.get_file_metadata(path.parent()).await?;
            if let Some(directory_metadata) = &directory_metadata {
                if (directory_metadata.short.attr & DirectoryEntryShort::ATTR_DIRECTORY) == 0 {
                    return Err(FileSystemError::FileTypeMismatch);
                }
            }

            // 父目录起始簇
            let directory_cluster = if let Some(directory_metadata) = &directory_metadata {
                directory_metadata.start_cluster()
            } else {
                inner.bpb.root_cluster
            };

            // 检查文件是否存在
            // TODO: 应该移动到 inner.create_file_meta
            match inner
                .get_file_metadata_by_cluster(directory_cluster, name)
                .await
            {
                Ok(_) => return Err(FileSystemError::FileExists),
                Err(FileSystemError::FileNotFound) => (),
                Err(e) => return Err(e),
            }

            // 获取一个可用簇
            let cluster = inner.find_available_cluster().await?;

            // 创建fat32_metadata
            let file_metadata =
                Fat32FileMetadata::new(name, cluster, DirectoryEntryShort::ATTR_ARCHIVE);

            // 写入
            if let Err(e) = inner
                .create_file_meta(directory_cluster, &file_metadata)
                .await
            {
                inner.free_cluster(cluster).await?;
                return Err(e);
            }

            Ok(())
        })
    }

    fn create_directory<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>> {
        Box::pin(async move {
            // 文件名
            let Some(name) = path.last_segment() else {
                return Err(FileSystemError::FileExists);
            };

            let mut inner = self.inner.write().await;
            // 父文件目录（可能为根目录）
            let directory_metadata = inner.get_file_metadata(path.parent()).await?;
            if let Some(directory_metadata) = &directory_metadata {
                if (directory_metadata.short.attr & DirectoryEntryShort::ATTR_DIRECTORY) == 0 {
                    return Err(FileSystemError::FileTypeMismatch);
                }
            }

            // 父目录起始簇
            let directory_cluster = if let Some(directory_metadata) = &directory_metadata {
                directory_metadata.start_cluster()
            } else {
                inner.bpb.root_cluster
            };

            // 检查文件是否存在
            // TODO: 应该移动到 inner.create_file_meta
            match inner
                .get_file_metadata_by_cluster(directory_cluster, name)
                .await
            {
                Ok(_) => return Err(FileSystemError::FileExists),
                Err(FileSystemError::FileNotFound) => (),
                Err(e) => return Err(e),
            }

            // 获取一个可用簇
            let cluster = inner.find_available_cluster().await?;

            // 清空簇内容
            if let Err(e) = inner
                .device
                .write_zeros(
                    inner.get_sector_by_cluster(cluster),
                    inner.bpb.sectors_per_cluster as u64,
                )
                .await
            {
                _ = inner.free_cluster(cluster).await;
                return Err(e.into());
            }

            // 创建fat32_metadata
            let file_metadata =
                Fat32FileMetadata::new(name, cluster, DirectoryEntryShort::ATTR_DIRECTORY);

            // 写入
            if let Err(e) = inner
                .create_file_meta(directory_cluster, &file_metadata)
                .await
            {
                _ = inner.free_cluster(cluster).await;
                return Err(e);
            }

            Ok(())
        })
    }

    fn open_file<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<Box<dyn FileHandle>, FileSystemError>> {
        Box::pin(async move {
            let mut inner = self.inner.write().await;

            // 获取文件信息
            let Some(file) = inner.get_file_metadata(path).await? else {
                return Err(FileSystemError::FileTypeMismatch);
            };

            // 检查是不是文件
            if (file.short.attr & DirectoryEntryShort::ATTR_DIRECTORY) != 0 {
                return Err(FileSystemError::FileTypeMismatch);
            }

            // 文件占用检查 & 添加占用
            if !inner.occupied_file.insert(file.start_cluster()) {
                return Err(FileSystemError::FileOccupied);
            }

            // 返回文件句柄
            Ok(Box::new(Fat32FileHandle {
                inner: Arc::downgrade(&self.inner),
                metadata: file,
                pointer: 0,
                closed: false,
            }) as Box<dyn FileHandle>)
        })
    }

    fn delete_file<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>> {
        Box::pin(async move {
            let mut inner = self.inner.write().await;

            // 获取文件信息
            let Some(file) = inner.get_file_metadata(path).await? else {
                return Err(FileSystemError::FileTypeMismatch);
            };

            // 检查是不是文件
            if (file.short.attr & DirectoryEntryShort::ATTR_DIRECTORY) != 0 {
                return Err(FileSystemError::FileTypeMismatch);
            }

            // 文件占用检查
            if inner.occupied_file.contains(&file.start_cluster()) {
                return Err(FileSystemError::FileOccupied);
            }

            // 清空文件条目
            inner.delete_file_meta(&file).await?;

            // 清空簇
            let mut cluster = file.start_cluster();
            while cluster != FatEntry::FAT_ENTRY_FREE
                && cluster < FatEntry::FAT_ENTRY_RESERVED_START
            {
                let next_cluster = inner.get_next_cluster(cluster).await?;
                inner.free_cluster(cluster).await?;
                cluster = next_cluster.0;
            }

            // 完成
            Ok(())
        })
    }

    fn delete_directory<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>> {
        Box::pin(async move {
            let mut inner = self.inner.write().await;

            // 获取文件信息
            let Some(file) = inner.get_file_metadata(path).await? else {
                return Err(FileSystemError::OperationNotSupport);
            };

            // 检查是不是文件夹
            if (file.short.attr & DirectoryEntryShort::ATTR_DIRECTORY) == 0 {
                return Err(FileSystemError::FileTypeMismatch);
            }

            // 检查是否有子文件/子文件夹
            let files = inner
                .get_all_filemeta_by_cluster(file.start_cluster())
                .await?;
            if !files.is_empty() {
                return Err(FileSystemError::FileExists);
            }

            // 删除文件夹
            inner.delete_file_meta(&file).await?;

            // 清空簇
            let mut cluster = file.start_cluster();
            while cluster != FatEntry::FAT_ENTRY_FREE
                && cluster < FatEntry::FAT_ENTRY_RESERVED_START
            {
                let next_cluster = inner.get_next_cluster(cluster).await?;
                inner.free_cluster(cluster).await?;
                cluster = next_cluster.0;
            }

            // 完成
            Ok(())
        })
    }

    fn rename<'fut>(
        &'fut self,
        old_path: Path<'fut>,
        new_path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<(), FileSystemError>> {
        Box::pin(async move {
            let mut inner = self.inner.write().await;
            // 原文件
            let Some(src) = inner.get_file_metadata(old_path).await? else {
                return Err(FileSystemError::OperationNotSupport);
            };

            // 新文件（父级）
            let dst_parent = inner.get_file_metadata(new_path.parent()).await?;
            let dst_parent_cluster = dst_parent
                .as_ref()
                .map_or_else(|| inner.bpb.root_cluster, Fat32FileMetadata::start_cluster);

            // 新文件
            let Some(last_segment) = new_path.last_segment() else {
                return Err(FileSystemError::OperationNotSupport);
            };
            match inner
                .get_file_metadata_by_cluster(dst_parent_cluster, last_segment)
                .await
            {
                Ok(_) => return Err(FileSystemError::FileExists),
                Err(FileSystemError::FileNotFound) => (),
                Err(e) => return Err(e),
            }

            // TODO: 由于移除&新增不是事务操作，这中间发生错误会导致丢数据，
            // 这里没有处理这种情况。为了健壮性，应当进行处理。

            // 移除
            inner.delete_file_meta(&src).await?;
            // 新增
            inner.create_file_meta(dst_parent_cluster, &src).await?;

            Ok(())
        })
    }

    fn get_metadata<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<FileMetadata, FileSystemError>> {
        Box::pin(async move {
            let inner = self.inner.read().await;

            // 获取文件元信息
            let Some(file_metadata) = inner.get_file_metadata(path).await? else {
                return Err(FileSystemError::OperationNotSupport);
            };

            // 获取文件详细信息
            inner.get_fs_metadata(&file_metadata).await
        })
    }

    fn list_directory<'fut>(
        &'fut self,
        path: Path<'fut>,
    ) -> BoxFuture<'fut, Result<Vec<FileMetadata>, FileSystemError>> {
        Box::pin(async move {
            let inner = self.inner.read().await;

            // 获取文件信息
            let file = inner.get_file_metadata(path).await?;

            // 检查是不是文件夹
            if let Some(file) = &file {
                if (file.short.attr & DirectoryEntryShort::ATTR_DIRECTORY) == 0 {
                    return Err(FileSystemError::FileTypeMismatch);
                }
            }

            // 获取子文件/子文件夹
            let files = inner
                .get_all_filemeta_by_cluster(
                    file.map_or_else(|| inner.bpb.root_cluster, |file| file.start_cluster()),
                )
                .await?;

            // 转化、返回数据
            let mut result = Vec::new();
            for file in files {
                result.push(inner.get_fs_metadata(&file).await?);
            }
            Ok(result)
        })
    }

    fn unmount(&self) -> BoxFuture<'_, Result<(), FileSystemError>> {
        // 我们没有在内存中缓存什么，所有数据都是即时刷入块设备的，因此无需处理
        Box::pin(async { Ok(()) })
    }
}

struct Fat32FileHandle {
    inner: Weak<RwLock<Fat32Inner>>,
    metadata: Fat32FileMetadata,
    pointer: u64,
    closed: bool,
}

impl FileHandle for Fat32FileHandle {
    fn close(&mut self) -> BoxFuture<'_, Result<(), FileSystemError>> {
        Box::pin(async move {
            // 重复关闭检查
            if self.closed {
                return Err(FileSystemError::FileClosed);
            }

            // 文件系统
            let Some(inner) = self.inner.upgrade() else {
                return Err(FileSystemError::Unmounted);
            };
            let mut inner = inner.write().await;

            // 取消占用
            inner.occupied_file.remove(&self.metadata.short_cluster);

            // 关闭标识
            self.closed = true;

            Ok(())
        })
    }

    fn move_pointer(&mut self, position: u64) -> BoxFuture<'_, Result<(), FileSystemError>> {
        Box::pin(async move {
            if self.closed {
                return Err(FileSystemError::FileClosed);
            }

            let file_size = self.metadata.short.file_size;
            self.pointer = position.max(file_size as u64);
            Ok(())
        })
    }

    fn get_pointer(&mut self) -> BoxFuture<'_, Result<u64, FileSystemError>> {
        Box::pin(async move {
            if self.closed {
                return Err(FileSystemError::FileClosed);
            }

            Ok(self.pointer)
        })
    }

    fn read<'fut>(
        &'fut mut self,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<u64, FileSystemError>> {
        Box::pin(async move {
            let inner = self.inner.upgrade().ok_or(FileSystemError::Unmounted)?;
            let inner = inner.read().await;

            let file_size = self.metadata.short.file_size as u64;
            let read_length = buf.len().min((file_size - self.pointer) as usize);

            // 已经向buf中写入的字节数量
            let mut assign_offset = 0;
            // 循环中已经跳过的文件字节数量
            let mut offset = 0;
            // 剩余要写入buf的字节数量
            let mut remain = read_length as u64;
            // 当前循环的簇
            let mut cluster = self.metadata.start_cluster();
            // 每扇区有效字节数
            let bytes_per_sector = inner.bpb.bytes_per_sector as u64;
            // 每簇有效字节数
            let bytes_per_cluster = bytes_per_sector * inner.bpb.sectors_per_cluster as u64;
            let block_size = inner.device.block_size();
            // 簇缓冲
            let mut cluster_buffer =
                alloc::vec![0u8; block_size as usize* inner.bpb.sectors_per_cluster as usize];

            // 循环各簇，找到要读取的部分
            'cluster_loop: while cluster != FatEntry::FAT_ENTRY_FREE
                && cluster < FatEntry::FAT_ENTRY_EOC_START
                && remain > 0
            {
                if offset + bytes_per_cluster < self.pointer {
                    offset += bytes_per_cluster;
                    cluster = inner.get_next_cluster(cluster).await?.0;
                    continue;
                }

                // 读盘
                inner
                    .device
                    .read_blocks(
                        inner.get_sector_by_cluster(cluster),
                        inner.bpb.sectors_per_cluster as u64,
                        &mut cluster_buffer,
                    )
                    .await?;

                // 循环各扇区
                for i in 0..inner.bpb.sectors_per_cluster as usize {
                    if offset + bytes_per_sector < self.pointer {
                        offset += bytes_per_cluster;
                        continue;
                    }

                    // 复制内容
                    let copy_start = self.pointer - offset;
                    let copy_length = (bytes_per_sector - copy_start).min(remain);
                    assert!(
                        cluster_buffer.len()
                            >= i as usize * block_size as usize
                                + copy_start as usize
                                + copy_length as usize
                    );
                    assert!(buf.len() >= assign_offset as usize + copy_length as usize);
                    // Safety: 我们已靠断言保证复制是范围是安全的
                    unsafe {
                        copy_nonoverlapping(
                            cluster_buffer
                                .as_ptr()
                                .add(i as usize * block_size as usize + copy_start as usize),
                            buf.as_mut_ptr().add(assign_offset as usize),
                            copy_length as usize,
                        );
                    }

                    // 维护循环不变量
                    assign_offset += copy_length;
                    offset += bytes_per_cluster;
                    self.pointer += copy_length;
                    remain -= copy_length;

                    assert!(offset == self.pointer);

                    if remain == 0 {
                        break 'cluster_loop;
                    }
                }

                cluster = inner.get_next_cluster(cluster).await?.0;
            }

            Ok(read_length as u64)
        })
    }

    fn write<'fut>(
        &'fut mut self,
        buf: &'fut [u8],
    ) -> BoxFuture<'fut, Result<(), FileSystemError>> {
        todo!()
    }
}

fn check_bpb(bpb: &BPB, block_size: u64, block_count: u64) -> Result<(), MountError> {
    // 每扇区字节数，不能小于512，不能超过硬件值，必须为2的整数次幂
    if bpb.bytes_per_sector < 512
        || bpb.bytes_per_sector as u64 > block_size
        || !bpb.bytes_per_sector.is_power_of_two()
    {
        return Err(MountError::InvalidFormat);
    }
    // 每簇扇区数，不能为0，必须为2的整数次幂
    if bpb.sectors_per_cluster == 0 || !bpb.sectors_per_cluster.is_power_of_two() {
        return Err(MountError::InvalidFormat);
    }
    // 引导区保留扇区数，不能为0，不能超过总扇区数
    if bpb.reserved_sector_count == 0 || bpb.reserved_sector_count as u64 > block_count {
        return Err(MountError::InvalidFormat);
    }
    // FAT表数量，不能为0
    if bpb.num_fats == 0 {
        return Err(MountError::InvalidFormat);
    }
    // 根目录项数，FAT12/16使用，FAT32固定为0
    if bpb.root_entry_count != 0 {
        return Err(MountError::InvalidFormat);
    }
    // 每个FAT表的扇区数，FAT32固定为0
    if bpb.fat_size_16 != 0 {
        return Err(MountError::InvalidFormat);
    }
    // 总扇区数，两个字段二选一，但另一个必须是0
    if bpb.total_sectors_16 == 0 {
        if bpb.total_sectors_32 as u64 != block_count {
            return Err(MountError::InvalidFormat);
        }
    } else {
        if bpb.total_sectors_16 as u64 != block_count {
            return Err(MountError::InvalidFormat);
        }
        if bpb.total_sectors_32 != 0 {
            return Err(MountError::InvalidFormat);
        }
    }
    // 每个FAT表的扇区数，不能为0
    if bpb.fat_size_32 == 0 {
        return Err(MountError::InvalidFormat);
    }
    // 根目录起始簇号，不能小于2
    if bpb.root_cluster < 2 {
        return Err(MountError::InvalidFormat);
    }
    // 扩展引导签名
    if bpb.boot_signature != 0x28 && bpb.boot_signature != 0x29 {
        return Err(MountError::InvalidFormat);
    }
    // 引导扇区签名
    if bpb.boot_sector_signature != BPB::BOOT_SECTOR_SIGNATURE {
        return Err(MountError::InvalidFormat);
    }

    Ok(())
}

fn check_fs_info(fs_info: &FSInfo) -> Result<(), MountError> {
    if fs_info.lead_signature != FSInfo::LEAD_SIGNATURE {
        return Err(MountError::InvalidFormat);
    }
    if fs_info.struct_signature != FSInfo::STRUCT_SIGNATURE {
        return Err(MountError::InvalidFormat);
    }
    if fs_info.trail_signature != FSInfo::TRAIL_SIGNATURE {
        return Err(MountError::InvalidFormat);
    }
    Ok(())
}

/// 根据保留扇区数、每扇区字节数、每簇扇区数、扇区总数计算总簇数
///
/// 返回：(总簇数, 剩余无法分配的扇区数)
fn calc_cluster_count(
    reserved_sector_count: u64, // 保留扇区数
    bytes_per_sector: u64,      // 每扇区字节数
    sectors_per_cluster: u64,   // 每簇扇区数
    block_count: u64,           // 扇区总数
) -> (u64, u64) {
    // 每个FAT扇区可容纳的簇数量
    let cluster_per_fat = bytes_per_sector / size_of::<FatEntry>() as u64;

    // 我们假设所有的FAT均被填满，那么每一个FAT扇区都会对应cluster_per_sector个簇
    // 由于FAT是两份，所以我们可以计算出每cluster_per_sector个簇对应多少扇区
    // 我们将其称为 一组簇 对应 多少扇区
    let sector_per_group = 2 + cluster_per_fat * sectors_per_cluster;

    // 基于此，我们可以算出全部扇区可以表示容纳多少组簇
    // 需要注意，由于前两个簇实际上不占用空间（保留编号），因此在计算时需要单独处理这些空间
    let virtual_block_count = block_count - reserved_sector_count + sectors_per_cluster * 2;
    let group_count = virtual_block_count / sector_per_group;
    let remain_block_count = virtual_block_count - sector_per_group * group_count;

    // 剩余的块已经不能填满一整组，那么有两种可能：
    // 1. 还能分出2个FAT的扇区，然后再凑至少一个簇
    // 2. 扇区不足以做到1
    // 据此，我们能算出剩下簇的数量
    let (remain_cluster, remain_block_count) = if remain_block_count > 2 {
        let remain_cluster = (remain_block_count - 2) / sectors_per_cluster;
        let remain_block_count = remain_block_count - 2 - remain_cluster * sectors_per_cluster;
        (remain_cluster, remain_block_count)
    } else {
        (0, remain_block_count)
    };

    // 然后我们就得到了总簇数（减去虚拟的两个簇）
    let cluster_total = (group_count * cluster_per_fat + remain_cluster).saturating_sub(2);

    (cluster_total, remain_block_count)
}

fn name_match_short(entry: &DirectoryEntryShort, name: &str) -> bool {
    let mut bytes = name.as_bytes().iter().copied();

    if !match_with_array(&mut bytes, &entry.name) {
        return false;
    }

    let name_has_dot = bytes.next() == Some(b'.');
    let entry_has_ext = entry.ext[0] != 0;

    if name_has_dot ^ entry_has_ext {
        return false;
    }

    if name_has_dot {
        if !match_with_array(&mut bytes, &entry.ext) {
            return false;
        }
    }

    !bytes.next().is_some()
}

// entry: 必须已经排序
fn name_match_long(entry: &[DirectoryEntryLong], name: &str) -> bool {
    let mut utf16 = name.encode_utf16();

    for entry in entry {
        let temp = entry.name1;
        if !match_with_array(&mut utf16, &temp) {
            return false;
        }
        let temp = entry.name2;
        if !match_with_array(&mut utf16, &temp) {
            return false;
        }
        let temp = entry.name3;
        if !match_with_array(&mut utf16, &temp) {
            return false;
        }
    }

    !utf16.next().is_some()
}

fn match_with_array<I, T>(iter: &mut I, array: &[T]) -> bool
where
    I: Iterator<Item = T>,
    T: Default + Eq,
{
    for item in array {
        if *item == T::default() {
            break;
        }

        let Some(next) = iter.next() else {
            return false;
        };

        if *item != next {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use crate::{
        device::memory::MemoryDevice,
        fs::{
            FileSystem, FileSystemError,
            fat32::{Fat32FileSystem, FatEntry, calc_cluster_count},
        },
        path::PathBuf,
        run_task,
    };

    #[test]
    fn test_calc_cluster_count() {
        assert_eq!(calc_cluster_count(32, 512, 8, 65536), (8172, 0));
        assert_eq!(calc_cluster_count(32, 512, 7, 10000), (1420, 4));
    }

    #[test]
    fn test_find_available_cluster() {
        run_task(async {
            // 28扇区 == 2保留扇区 + 2FAT扇区 + 3簇(*8)
            let device = Arc::new(MemoryDevice::new(512 * 28, 512));
            let fs = Fat32FileSystem::with_format(device.clone()).await.unwrap();
            let mut inner = fs.inner.write().await;
            // 2号簇是根路径，已经被占用
            assert_eq!(
                inner.get_next_cluster(2).await.unwrap().0,
                FatEntry::FAT_ENTRY_EOC_START
            );

            assert_eq!(inner.find_available_cluster().await.unwrap(), 3);
            assert_eq!(inner.find_available_cluster().await.unwrap(), 4);

            // 因为我们的内存盘只有3个簇，所以再申请会报错
            assert!(matches!(
                inner.find_available_cluster().await.unwrap_err(),
                FileSystemError::DiskFull
            ));
        })
    }

    #[test]
    fn test_create_file() {
        run_task(async {
            let device = Arc::new(MemoryDevice::new(512 * 28, 512));
            let fs = Fat32FileSystem::with_format(device.clone()).await.unwrap();
            let path = PathBuf::from_str("test.txt").unwrap();
            fs.create_file(path.as_path()).await.unwrap();
            let file = fs.get_metadata(path.as_path()).await.unwrap();
            assert_eq!(file.name, "test.txt");
            assert!(!file.is_directory);
            assert_eq!(file.size, 0);
            assert_eq!(file.allocated_size, 512 * 8);
        });
    }

    #[test]
    fn test_remount() {
        run_task(async {
            let device = Arc::new(MemoryDevice::new(512 * 28, 512));
            let fs = Fat32FileSystem::with_format(device.clone()).await.unwrap();
            let path = PathBuf::from_str("test.txt").unwrap();
            fs.create_file(path.as_path()).await.unwrap();
            fs.unmount().await.unwrap();
            let fs = Fat32FileSystem::mount(device.clone()).await.unwrap();
            let file = fs.get_metadata(path.as_path()).await.unwrap();
            assert_eq!(file.name, "test.txt");
            assert!(!file.is_directory);
            assert_eq!(file.size, 0);
            assert_eq!(file.allocated_size, 512 * 8);
        });
    }

    #[test]
    fn test_create_long_name_file() {
        run_task(async {
            let device = Arc::new(MemoryDevice::new(512 * 28, 512));
            let fs = Fat32FileSystem::with_format(device.clone()).await.unwrap();
            let name = "longlonglonglonglonglonglonglonglonglonglonglonglonglonglonglong.txt";
            let path = PathBuf::from_str(name).unwrap();
            fs.create_file(path.as_path()).await.unwrap();
            let file = fs.get_metadata(path.as_path()).await.unwrap();
            assert_eq!(file.name, name);
            assert!(!file.is_directory);
            assert_eq!(file.size, 0);
            assert_eq!(file.allocated_size, 512 * 8);
        });
    }

    #[test]
    fn test_create_directory() {
        run_task(async {
            let device = Arc::new(MemoryDevice::new(512 * 28, 512));
            let fs = Fat32FileSystem::with_format(device.clone()).await.unwrap();

            let dir_path = PathBuf::from_str("dir").unwrap();
            fs.create_directory(dir_path.as_path()).await.unwrap();
            let dir = fs.get_metadata(dir_path.as_path()).await.unwrap();
            assert_eq!(dir.name, "dir");
            assert!(dir.is_directory);
            assert_eq!(dir.size, 0);
            assert_eq!(dir.allocated_size, 512 * 8);

            let file_path = PathBuf::from_str("dir/test.txt").unwrap();
            fs.create_file(file_path.as_path()).await.unwrap();
            let file = fs.get_metadata(file_path.as_path()).await.unwrap();
            assert_eq!(file.name, "test.txt");
            assert!(!file.is_directory);
            assert_eq!(file.size, 0);
            assert_eq!(file.allocated_size, 512 * 8);
        });
    }
}
