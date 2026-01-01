use core::{
    arch::asm,
    pin::Pin,
    ptr::copy_nonoverlapping,
    task::{Context, Poll, Waker},
};

use alloc::{boxed::Box, collections::vec_deque::VecDeque, sync::Arc, vec::Vec};
use filesystem::{
    BoxFuture,
    device::{BlockDevice, BlockDeviceError},
};

use crate::sync::{int::IrqGuard, spin::SpinLock};

/// 全局等待队列
/// (inflight, queue)
static ATA_QUEUE: SpinLock<(Option<SyncRequest>, VecDeque<SyncRequest>)> =
    SpinLock::new((None, VecDeque::new()));

/// ATA LBA 异步读盘驱动
pub struct AtaLbaDriver {
    /// 硬盘号
    disk: u8,
    /// 大小
    size: u32,
}

const ATA_DATA: u16 = 0x1F0;
const ATA_SECTOR_COUNT: u16 = 0x1F2;
const ATA_SECTOR: u16 = 0x1F3;
const ATA_CYL_LO: u16 = 0x1F4;
const ATA_CYL_HI: u16 = 0x1F5;
const ATA_HEAD: u16 = 0x1F6;
const ATA_STATUS: u16 = 0x1F7;
const ATA_COMMAND: u16 = 0x1F7;
const ATA_INTERRUPT_ENABLE: u16 = 0x3F6;

type SyncRequest = Arc<SpinLock<Request>>;

struct Request {
    /// 硬盘号
    disk: u8,
    /// LBA逻辑地址
    lba: u64,
    /// 异步唤醒
    waker: Waker,
    /// 任务状态（例如取消）
    status: u8,
    /// 操作
    operate: Operation,
    /// 缓冲区
    buffer: Vec<u16>,
    /// 是否发生了错误
    error: bool,
}

enum Operation {
    Identify,
    Read,
    Write,
}

enum IdentifyDeviceFuture {
    Init { disk: u8 },
    WaitDevice { request: SyncRequest },
    Done,
}

enum WriteBlockFuture<'a> {
    Init {
        driver: &'a AtaLbaDriver,
        block_index: u64,
        buf: &'a [u8],
    },
    WaitDevice {
        request: SyncRequest,
    },
    Done,
}

enum ReadBlockFuture<'a> {
    Init {
        driver: &'a AtaLbaDriver,
        block_index: u64,
        buf: &'a mut [u8],
    },
    WaitDevice {
        buf: &'a mut [u8],
        request: SyncRequest,
    },
    Done,
}

impl AtaLbaDriver {
    pub async fn new(disk: u8) -> Result<Arc<Self>, BlockDeviceError> {
        let identity_information = IdentifyDeviceFuture::Init { disk }.await?;
        // 60 低16位
        // 61 高16位
        let block_count =
            (identity_information[60] as u32) | ((identity_information[61] as u32) << 16);

        let driver = AtaLbaDriver {
            disk,
            size: block_count,
        };

        Ok(Arc::new(driver))
    }
}

impl Request {
    const STATUS_PENDING: u8 = 1;
    const STATUS_OK: u8 = 2;
}

impl BlockDevice for AtaLbaDriver {
    fn block_size(&self) -> u64 {
        512
    }

    fn block_count(&self) -> u64 {
        self.size as u64
    }

    fn write_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(WriteBlockFuture::Init {
            driver: self,
            block_index,
            buf,
        })
    }

    fn read_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(ReadBlockFuture::Init {
            driver: self,
            block_index,
            buf,
        })
    }
}

impl Future for WriteBlockFuture<'_> {
    type Output = Result<(), BlockDeviceError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.as_mut().get_mut() {
                Self::Init {
                    driver,
                    block_index,
                    buf,
                } => {
                    // 准备缓冲区
                    assert!(buf.len() == 512);
                    let mut buffer = alloc::vec![0u16; 256];
                    unsafe {
                        copy_nonoverlapping(buf.as_ptr(), buffer.as_mut_ptr() as *mut u8, 512);
                    }

                    // 构造请求
                    let request = Request {
                        disk: driver.disk,
                        lba: *block_index,
                        waker: cx.waker().clone(),
                        status: Request::STATUS_PENDING,
                        operate: Operation::Write,
                        buffer,
                        error: false,
                    };
                    let request = Arc::new(SpinLock::new(request));

                    // 排队
                    let _guard = IrqGuard::cli();
                    let mut queue = ATA_QUEUE.lock();
                    if queue.0.is_some() {
                        queue.1.push_back(request.clone());
                    } else {
                        send_io_command(&request);
                        queue.0 = Some(request.clone());
                    }
                    *self.as_mut().get_mut() = Self::WaitDevice { request }
                }
                Self::WaitDevice { request } => {
                    let _guard = IrqGuard::cli();
                    let request = request.lock();
                    if request.status != Request::STATUS_OK {
                        return Poll::Pending;
                    }

                    let result = if request.error {
                        Poll::Ready(Err(BlockDeviceError::IoError))
                    } else {
                        Poll::Ready(Ok(()))
                    };

                    drop(request);
                    *self.as_mut().get_mut() = Self::Done;

                    return result;
                }
                Self::Done => panic!("future polled after complete"),
            }
        }
    }
}

impl Future for ReadBlockFuture<'_> {
    type Output = Result<(), BlockDeviceError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.as_mut().get_mut() {
                Self::Init {
                    driver,
                    block_index,
                    buf,
                } => {
                    // 构造请求
                    assert!(buf.len() == 512);
                    let request = Request {
                        disk: driver.disk,
                        lba: *block_index,
                        waker: cx.waker().clone(),
                        status: Request::STATUS_PENDING,
                        operate: Operation::Read,
                        buffer: alloc::vec![0u16; 256],
                        error: false,
                    };
                    let request = Arc::new(SpinLock::new(request));

                    // 排队
                    let _guard = IrqGuard::cli();
                    let mut queue = ATA_QUEUE.lock();
                    if queue.0.is_some() {
                        queue.1.push_back(request.clone());
                    } else {
                        send_io_command(&request);
                        queue.0 = Some(request.clone());
                    }

                    let buf = core::mem::take(buf);
                    *self.as_mut().get_mut() = Self::WaitDevice { buf, request };
                }
                Self::WaitDevice { buf, request } => {
                    let _guard = IrqGuard::cli();
                    let request = request.lock();
                    if request.status != Request::STATUS_OK {
                        return Poll::Pending;
                    }

                    let result = if request.error {
                        Poll::Ready(Err(BlockDeviceError::IoError))
                    } else {
                        // 复制数据
                        unsafe {
                            copy_nonoverlapping(
                                request.buffer.as_ptr() as *const u8,
                                buf.as_mut_ptr(),
                                512,
                            );
                        }
                        Poll::Ready(Ok(()))
                    };

                    drop(request);
                    *self.as_mut().get_mut() = Self::Done;
                    return result;
                }
                Self::Done => panic!("future polled after complete"),
            }
        }
    }
}

impl Future for IdentifyDeviceFuture {
    type Output = Result<Vec<u16>, BlockDeviceError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.as_mut().get_mut() {
                Self::Init { disk } => {
                    // 构造请求
                    let request = Request {
                        disk: *disk,
                        lba: 0,
                        waker: cx.waker().clone(),
                        status: Request::STATUS_PENDING,
                        operate: Operation::Identify,
                        buffer: alloc::vec![0u16; 256],
                        error: false,
                    };
                    let request = Arc::new(SpinLock::new(request));

                    // 排队
                    let _guard = IrqGuard::cli();
                    let mut queue = ATA_QUEUE.lock();
                    if queue.0.is_some() {
                        queue.1.push_back(request.clone());
                    } else {
                        send_io_command(&request);
                        queue.0 = Some(request.clone());
                    }

                    *self.as_mut().get_mut() = Self::WaitDevice { request };
                }
                Self::WaitDevice { request } => {
                    let _guard = IrqGuard::cli();
                    let mut request = request.lock();
                    if request.status != Request::STATUS_OK {
                        return Poll::Pending;
                    }

                    let result = if request.error {
                        Poll::Ready(Err(BlockDeviceError::IoError))
                    } else {
                        Poll::Ready(Ok(core::mem::take(&mut request.buffer)))
                    };

                    drop(request);
                    *self.as_mut().get_mut() = Self::Done;

                    return result;
                }
                Self::Done => panic!("future polled after complete"),
            }
        }
    }
}

fn send_io_command(request: &SyncRequest) {
    // 设置中断
    let mut interrupt_enable: u8;
    unsafe {
        asm!(
            "in al, dx",
            in("dx") ATA_INTERRUPT_ENABLE,
            out("al") interrupt_enable,
            options(nostack, preserves_flags),
        );
    }
    interrupt_enable &= 0xfe;
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_INTERRUPT_ENABLE,
            in("al") interrupt_enable,
            options(nostack, preserves_flags),
        );
    }

    let _guard = IrqGuard::cli();
    let request = request.lock();
    match request.operate {
        Operation::Identify => send_identify_command(&request),
        Operation::Read => send_read_command(&request),
        Operation::Write => send_write_command(&request),
    }
    io_wait();
}

fn send_lba(disk: u8, lba: u64) {
    // 设置扇区数
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_SECTOR_COUNT,
            in("al") 1 as u8,
            options(nostack, preserves_flags),
        );
    }

    // 写LBA低24bit
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_SECTOR,
            in("al") (lba & 0xff) as u8,
            options(nostack, preserves_flags),
        );

        asm!(
            "out dx, al",
            in("dx") ATA_CYL_LO,
            in("al") ((lba >> 8) & 0xff) as u8,
            options(nostack, preserves_flags),
        );

        asm!(
            "out dx, al",
            in("dx") ATA_CYL_HI,
            in("al") ((lba >> 16) & 0xff) as u8,
            options(nostack, preserves_flags),
        );
    }

    // 写高4bit+硬盘号
    let head = 0xE0 | ((disk & 1) << 4) | ((lba >> 24) & 0x0F) as u8;
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_HEAD,
            in("al") head,
            options(nostack, preserves_flags),
        );
    }
}

fn io_wait() {
    unsafe {
        asm!("in al, dx", in("dx") 0x3F6);
        asm!("in al, dx", in("dx") 0x3F6);
        asm!("in al, dx", in("dx") 0x3F6);
        asm!("in al, dx", in("dx") 0x3F6);
    }
}

fn send_identify_command(request: &Request) {
    // 发送位置
    // 协议要求清除扇区寄存器和LBA寄存器，此处request.lba为0，刚好满足要求
    assert_eq!(request.lba, 0);
    send_lba(request.disk, request.lba);
    io_wait();
    // 发送identify请求
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_COMMAND,
            in("al") 0xECu8,
            options(nostack, preserves_flags)
        );
    }
}

fn send_read_command(request: &Request) {
    // 发送位置
    send_lba(request.disk, request.lba);
    io_wait();
    // 发送读盘请求
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_COMMAND,
            in("al") 0x20u8,
            options(nostack, preserves_flags)
        );
    }
}

fn send_write_command(request: &Request) {
    // 发送位置
    send_lba(request.disk, request.lba);
    io_wait();
    // 发送写盘请求
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_COMMAND,
            in("al") 0x30u8,
            options(nostack, preserves_flags)
        );
    }
    // 等待DRQ
    wait_drq();
    // PIO方式写数据
    for i in 0..256 {
        unsafe {
            asm!(
                "out dx, ax",
                in("ax") request.buffer[i],
                in("dx") ATA_DATA,
                options(nostack, preserves_flags)
            )
        }
    }
}

fn wait_drq() {
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

        // BSY=1，控制器忙
        if (status & 0x80) != 0 {
            continue;
        }

        // DRQ=1，请求主机写数据
        if (status & 0x08) != 0 {
            return;
        }
    }
}

pub fn ata_irq() {
    let status: u8;
    unsafe {
        asm!(
            "in al, dx",
            out("al") status,
            in("dx") ATA_STATUS,
            options(nostack, preserves_flags),
        )
    }

    // BSY=1，控制器忙
    if (status & 0x80) != 0 {
        return;
    }
    // ERR=1，错误
    let err_reg = (status & 1) != 0;
    // DRQ=1，请求主机写数据
    let drq_reg = (status & 0x08) != 0;

    let _guard = IrqGuard::cli();
    let mut queue = ATA_QUEUE.lock();

    if let Some(raw_request) = queue.0.take() {
        let mut request = raw_request.lock();

        request.error = err_reg;
        match request.operate {
            Operation::Identify => {
                // 读盘需要DRQ=1
                if !err_reg && !drq_reg {
                    drop(request);
                    queue.0 = Some(raw_request);
                    return;
                }

                if !err_reg {
                    // PIO方式读取数据
                    for i in 0..256 {
                        unsafe {
                            asm!(
                                "in ax, dx",
                                out("ax") request.buffer[i],
                                in("dx") ATA_DATA,
                                options(nostack, preserves_flags)
                            );
                        }
                    }
                }
            }
            Operation::Read => {
                // 读盘需要DRQ=1
                if !err_reg && !drq_reg {
                    drop(request);
                    queue.0 = Some(raw_request);
                    return;
                }

                if !err_reg {
                    // PIO方式读取数据
                    for i in 0..256 {
                        unsafe {
                            asm!(
                                "in ax, dx",
                                out("ax") request.buffer[i],
                                in("dx") ATA_DATA,
                                options(nostack, preserves_flags)
                            );
                        }
                    }
                }
            }
            Operation::Write => {}
        }
        request.status = Request::STATUS_OK;
        request.waker.wake_by_ref();
    }
    if let Some(next) = queue.1.pop_front() {
        send_io_command(&next);
        queue.0 = Some(next);
    }
}
