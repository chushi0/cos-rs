use core::{
    arch::asm,
    fmt::{Arguments, Write},
    ptr, slice,
};

use crate::sync::{IrqGuard, SpinLock};

pub static WRITER: SpinLock<Option<VgaTextWriter>> = SpinLock::new(None);

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {
        $crate::display::vga_text::_kprint(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! kprintln {
    () => {
        $crate::kprint!("\n");
    };
    ($($arg:tt)*) => {
        $crate::kprint!("{}\n", format_args!($($arg)*));
    }
}

pub fn init() {
    let mut writer = WRITER.lock();
    if writer.is_none() {
        writer.replace(unsafe { VgaTextWriter::new() });
    }
}

pub struct VgaTextWriter {
    buffer: &'static mut [u16],
    cursor: (u8, u8), // row, col
    style: u8,
}

impl VgaTextWriter {
    const ADDRESS: usize = 0xb8000; // Buffer 地址
    const WIDTH: usize = 80; // 宽度
    const HEIGHT: usize = 25; // 高度
    const DEFAULT_STYLE: u8 = 0x07; // 默认样式，黑底白字

    /// 创建 VgaTextWriter
    ///
    /// Safety: 保证只创建一个VgaTextWriter，否则会有数据竞争
    /// VGA必须处于文本模式
    unsafe fn new() -> Self {
        // Safety: bootloader已经将此区域加入页表
        let buffer = unsafe {
            slice::from_raw_parts_mut(Self::ADDRESS as *mut u16, Self::WIDTH * Self::HEIGHT)
        };

        // 清空缓冲区
        buffer.fill(Self::with_style(Self::DEFAULT_STYLE, b' '));

        // 复位光标
        Self::hw_set_cursor(0, 0);

        Self {
            buffer,
            cursor: (0, 0),
            style: Self::DEFAULT_STYLE,
        }
    }

    /// 设置光标位置
    fn hw_set_cursor(row: u8, col: u8) {
        #[inline]
        unsafe fn outb(port: u16, val: u8) {
            unsafe {
                asm!(
                    "out dx, al",
                    in("dx") port,
                    in("al") val,
                    options(nostack, preserves_flags)
                );
            }
        }

        let pos = (row as usize * Self::WIDTH + col as usize) as u16;
        unsafe {
            outb(0x3D4, 0x0F);
            outb(0x3D5, (pos & 0xFF) as u8);
            outb(0x3D4, 0x0E);
            outb(0x3D5, (pos >> 8) as u8);
        }
    }

    pub fn row(&self) -> u8 {
        self.cursor.0
    }

    pub fn col(&self) -> u8 {
        self.cursor.1
    }

    pub fn set_cursor(&mut self, row: u8, col: u8) {
        assert!((row as usize) < Self::HEIGHT);
        assert!((col as usize) < Self::WIDTH);

        self.cursor = (row, col);
        Self::hw_set_cursor(row, col);
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        const TAB_SIZE: u8 = 2;

        for byte in bytes {
            match *byte {
                // 回车
                b'\r' => {
                    self.cursor.1 = 0;
                }

                // 换行
                b'\n' => {
                    self.cursor.0 += 1;
                    self.cursor.1 = 0;
                    self.check_height_overflow();
                }

                // 退格
                0x08 => {
                    self.cursor.1 = self.cursor.1.saturating_sub(1);
                }

                // TAB
                b'\t' => {
                    for _ in 0..TAB_SIZE {
                        self.write_char(b' ');
                        self.cursor.1 += 1;
                        self.check_width_overflow();
                    }
                }

                // 可打印字符
                ch @ 0x20..=0x7E => {
                    self.write_char(ch);
                    self.cursor.1 += 1;
                    self.check_width_overflow();
                }

                _ => {
                    self.write_char(b'.');
                    self.cursor.1 += 1;
                    self.check_width_overflow();
                }
            }
        }

        Self::hw_set_cursor(self.cursor.0, self.cursor.1);
    }

    const fn with_style(style: u8, char: u8) -> u16 {
        ((style as u16) << 8) | (char as u16)
    }

    fn write_char(&mut self, char: u8) {
        self.buffer[self.cursor.0 as usize * Self::WIDTH + self.cursor.1 as usize] =
            Self::with_style(self.style, char);
    }

    fn check_width_overflow(&mut self) {
        if self.cursor.1 as usize >= Self::WIDTH {
            self.cursor.1 = 0;
            self.cursor.0 += 1;
            self.check_height_overflow();
        }
    }

    fn check_height_overflow(&mut self) {
        if self.cursor.0 as usize >= Self::HEIGHT {
            unsafe {
                ptr::copy(
                    self.buffer[Self::WIDTH..].as_mut_ptr(),
                    self.buffer.as_mut_ptr(),
                    Self::WIDTH * (Self::HEIGHT - 1),
                );
            }
            self.buffer[(Self::WIDTH * (Self::HEIGHT - 1))..]
                .fill(Self::with_style(self.style, b' '));
        }
    }
}

impl Write for VgaTextWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_bytes(s.as_bytes());
        Ok(())
    }
}

#[doc(hidden)]
pub fn _kprint(args: Arguments<'_>) {
    let _guard = unsafe { IrqGuard::cli() };
    let mut writer = WRITER.lock();
    writer
        .as_mut()
        .expect("vga_text is not available")
        .write_fmt(args)
        .unwrap();
}
