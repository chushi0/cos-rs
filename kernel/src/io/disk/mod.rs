use alloc::{collections::btree_map::BTreeMap, sync::Arc};
use filesystem::{
    device::mbr::{MbrPartitionDevice, PARTITION_TYPE_FAT32},
    fs::{FileSystem, fat32::Fat32FileSystem},
};

use crate::{
    io::disk::ata_lba::AtaLbaDriver,
    sync::{int::IrqGuard, spin::SpinLock},
};

pub mod ata_lba;

pub static FILE_SYSTEMS: SpinLock<BTreeMap<u32, Arc<dyn FileSystem>>> =
    SpinLock::new(BTreeMap::new());

pub struct InitDiskError;

// 初始化磁盘
pub async fn init_disk(startup_disk: u8) -> Result<(), InitDiskError> {
    let disk = AtaLbaDriver::new(startup_disk)
        .await
        .map_err(|_| InitDiskError)?;
    let mbr_disk = MbrPartitionDevice::mount(disk)
        .await
        .map_err(|_| InitDiskError)?;

    for disk in mbr_disk {
        let Some(disk) = disk else {
            continue;
        };
        if disk.get_partition_type() != PARTITION_TYPE_FAT32 {
            continue;
        }
        let fs = Fat32FileSystem::mount(Arc::new(disk))
            .await
            .map_err(|_| InitDiskError)?;

        let _guard = unsafe { IrqGuard::cli() };
        FILE_SYSTEMS.lock().insert(0, Arc::new(fs));
    }

    Ok(())
}
