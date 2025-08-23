use core::{fmt::Write, ptr, slice};

/// loader中便于输出文字信息的工具类
pub struct VgaText {
    /// 映射到VGA文字缓冲区
    memory: &'static mut [u16],
    /// 当前写入位置，仅内部表示，没有与显示器同步
    cursor: usize,
}

impl VgaText {
    /// 创建一个VgaText并清空屏幕
    /// 由于Vga文字缓冲区的内存是固定位置，多次调用此函数会创建多个缓冲区的可变借用，因此该函数是unsafe的
    ///
    /// Safety: 调用方需保证不会创建多个VgaText
    pub unsafe fn new() -> Self {
        let memory = unsafe { slice::from_raw_parts_mut(0xb8000 as *mut u16, 25 * 80) };
        memory.fill(0);
        Self { memory, cursor: 0 }
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.cursor >= self.memory.len() {
                self.cursor = 0;
            }
            match b {
                // 回车，光标回到行首
                b'\r' => {
                    self.cursor -= self.cursor % 80;
                }
                // 换行，光标到下一行
                b'\n' => {
                    self.cursor -= self.cursor % 80;
                    self.cursor += 80;
                }
                // 其他字符正常输出
                b => {
                    unsafe {
                        ptr::write_volatile(&mut self.memory[self.cursor], (b as u16) | 0x0700);
                    }
                    self.cursor += 1;
                }
            }
        }
    }
}

impl Write for VgaText {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_bytes(s.as_bytes());
        Ok(())
    }
}
