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

use crate::{
    kprintln,
    sync::{int::IrqGuard, spin::SpinLock},
};

/// 全局等待队列
static ATA_QUEUE: SpinLock<(Option<SyncRequest>, VecDeque<SyncRequest>)> =
    SpinLock::new((None, VecDeque::new()));

/// ATA LBA 异步读盘驱动
pub struct AtaLbaDriver {
    /// 硬盘号
    disk: u8,
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
    Read,
    Write,
}

struct WriteBlockFuture<'a> {
    driver: &'a AtaLbaDriver,
    block_index: u64,
    buf: &'a [u8],
    requset: Option<SyncRequest>,
}

struct ReadBlockFuture<'a> {
    driver: &'a AtaLbaDriver,
    block_index: u64,
    buf: &'a mut [u8],
    requset: Option<SyncRequest>,
}

impl AtaLbaDriver {
    pub async fn new(disk: u8) -> Result<Arc<Self>, BlockDeviceError> {
        let driver = AtaLbaDriver { disk };

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
        // TODO: 暂时写死，应当通过IO指令进行查询
        20480
    }

    fn write_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(WriteBlockFuture {
            driver: self,
            block_index,
            buf,
            requset: None,
        })
    }

    fn read_block<'fut>(
        &'fut self,
        block_index: u64,
        buf: &'fut mut [u8],
    ) -> BoxFuture<'fut, Result<(), BlockDeviceError>> {
        Box::pin(ReadBlockFuture {
            driver: self,
            block_index,
            buf,
            requset: None,
        })
    }
}

impl Future for WriteBlockFuture<'_> {
    type Output = Result<(), BlockDeviceError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.requset.as_ref() {
            Some(request) => {
                let _guard = IrqGuard::cli();
                let request = request.lock();
                if request.status != Request::STATUS_OK {
                    return Poll::Pending;
                }
                if request.error {
                    Poll::Ready(Err(BlockDeviceError::IoError))
                } else {
                    Poll::Ready(Ok(()))
                }
            }
            None => {
                assert!(self.buf.len() == 512);
                let mut buffer = alloc::vec![0u16; 256];
                unsafe {
                    copy_nonoverlapping(self.buf.as_ptr(), buffer.as_mut_ptr() as *mut u8, 512);
                }
                let request = Request {
                    disk: self.driver.disk,
                    lba: self.block_index,
                    waker: cx.waker().clone(),
                    status: Request::STATUS_PENDING,
                    operate: Operation::Write,
                    buffer,
                    error: false,
                };
                let request = Arc::new(SpinLock::new(request));
                self.as_mut().requset = Some(request.clone());

                let _guard = IrqGuard::cli();
                let mut queue = ATA_QUEUE.lock();
                if queue.0.is_some() {
                    queue.1.push_back(request);
                } else {
                    send_io_command(&request);
                    queue.0 = Some(request);
                }

                Poll::Pending
            }
        }
    }
}

impl Future for ReadBlockFuture<'_> {
    type Output = Result<(), BlockDeviceError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.requset.clone() {
            Some(request) => {
                let _guard = IrqGuard::cli();
                let request = request.lock();
                if request.status != Request::STATUS_OK {
                    return Poll::Pending;
                }
                if request.error {
                    Poll::Ready(Err(BlockDeviceError::IoError))
                } else {
                    unsafe {
                        copy_nonoverlapping(
                            request.buffer.as_ptr() as *const u8,
                            self.buf.as_mut_ptr(),
                            512,
                        );
                    }
                    Poll::Ready(Ok(()))
                }
            }
            None => {
                assert!(self.buf.len() == 512);
                let request = Request {
                    disk: self.driver.disk,
                    lba: self.block_index,
                    waker: cx.waker().clone(),
                    status: Request::STATUS_PENDING,
                    operate: Operation::Read,
                    buffer: alloc::vec![0u16; 256],
                    error: false,
                };
                let request = Arc::new(SpinLock::new(request));
                self.as_mut().requset = Some(request.clone());

                let _guard = IrqGuard::cli();
                let mut queue = ATA_QUEUE.lock();
                if queue.0.is_some() {
                    queue.1.push_back(request);
                } else {
                    send_io_command(&request);
                    queue.0 = Some(request);
                }

                Poll::Pending
            }
        }
    }
}

fn send_io_command(request: &SyncRequest) {
    kprintln!("send io command");
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

fn send_read_command(request: &Request) {
    send_lba(request.disk, request.lba);
    io_wait();
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
    send_lba(request.disk, request.lba);
    io_wait();
    unsafe {
        asm!(
            "out dx, al",
            in("dx") ATA_COMMAND,
            in("al") 0x30u8,
            options(nostack, preserves_flags)
        );
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
    let drq_reg = (status & 0x08) != 0;
    if !err_reg && !drq_reg {
        return;
    }

    let _guard = IrqGuard::cli();
    let mut queue = ATA_QUEUE.lock();
    if let Some(request) = queue.0.take() {
        let mut request = request.lock();

        request.error = err_reg;
        if !err_reg {
            match request.operate {
                Operation::Read => {
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
                Operation::Write => {
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
            }
        }
        request.status = Request::STATUS_OK;
        request.waker.wake_by_ref();
    }
    if let Some(next) = queue.1.pop_front() {
        send_io_command(&next);
        queue.0 = Some(next);
    }
}
