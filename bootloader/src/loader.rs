use core::{arch::asm, hint::spin_loop, ptr::copy_nonoverlapping};

// 加载内核
pub fn load_kernel(disk: u8) {
    let start_block = unsafe { *((0x7C00 + 446 + 12) as *mut u32) } + 1;
    let block_count = unsafe { *((0x7C00 + 446 + 12 + 16) as *mut u32) } as u32;
    let start_ptr = 0x20_0000 as *mut u8; // 2M，对齐Huge Page

    for i in 0..block_count {
        unsafe {
            ata_read_disk(disk, start_block + i, start_ptr.add((512 * i) as usize));
        }
    }
}

/// 使用ata lba读磁盘
///
/// disk: 硬盘号
/// block: 逻辑扇区，不能超过0x1000_000
/// ptr: 数据写入位置，必须16bit对齐
///
/// Safety: 不能并发读，必须关中断，硬盘指定位置必须存在
unsafe fn ata_read_disk(disk: u8, block: u32, ptr: *mut u8) {
    const ATA_DATA: u16 = 0x1F0;
    const ATA_SECTOR: u16 = 0x1F3;
    const ATA_CYL_LO: u16 = 0x1F4;
    const ATA_CYL_HI: u16 = 0x1F5;
    const ATA_HEAD: u16 = 0x1F6;
    const ATA_STATUS: u16 = 0x1F7;
    const ATA_COMMAND: u16 = 0x1F7;

    assert!(block < 0x1000_0000);
    assert!((ptr as usize & 1) == 0);

    // 设置扇区数
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_SECTOR,
            in("al") 1 as u8,
            options(nostack, preserves_flags),
        );
    }

    // 写LBA低24bit
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_SECTOR,
            in("al") (block & 0xff) as u8,
            options(nostack, preserves_flags),
        );

        asm!(
            "out dx, al",
            in("dx") ATA_CYL_LO,
            in("al") ((block >> 8) & 0xff) as u8,
            options(nostack, preserves_flags),
        );

        asm!(
            "out dx, al",
            in("dx") ATA_CYL_HI,
            in("al") ((block>> 16) & 0xff) as u8,
            options(nostack, preserves_flags),
        );
    }

    // 写高4bit+硬盘号
    let head = 0xE0 | ((disk & 1) << 4) | ((block >> 24) & 0x0F) as u8;
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_HEAD,
            in("al") head,
            options(nostack, preserves_flags),
        );
    }

    // 发送命令
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_COMMAND,
            in("al") 0x20 as u8,
            options(nostack, preserves_flags),
        );
    }

    // 等待
    loop {
        let status: u8;
        unsafe {
            asm!(
                "in al, dx",
                out("al") status,
                in("dx") ATA_STATUS,
                options(nostack, preserves_flags),
            )
        }

        if status & 1 != 0 {
            panic!("read disk error, disk={disk}, block={block}, ptr={ptr:?}");
        }

        if (status & 0x80) == 0 && (status & 0x08) != 0 {
            break;
        }

        spin_loop();
    }

    // 数据传输
    let mut buf = [0u8; 256];
    assert!((buf.as_ptr() as usize) < 0x1_0000);
    unsafe {
        asm!(
            "rep insw",
            in("dx") ATA_DATA,
            in("di") buf.as_mut_ptr() as u16,
            in("cx") 256,
            options(nostack, preserves_flags),
        );

        copy_nonoverlapping(buf.as_ptr(), ptr, 512);
    }
}
