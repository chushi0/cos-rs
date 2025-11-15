#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

use core::{arch::asm, fmt::Write, slice};

use crate::{
    bit64::{enable_64bit_mode, test_cpu_is_support_64bit},
    loader::load_kernel,
    memory::{MemoryRegion, normalize_memory_region},
    vga::VgaText,
};

mod bit64;
mod loader;
mod memory;
mod vga;

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct RealBiosInfo {
    memory: *mut MemoryRegion,
    memory_region_size: u16,
    startup_disk: u8,
}

// Safety: CPU已经正确设置GDT、开启A20并进入32位保护模式
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(bios_info: &RealBiosInfo) -> ! {
    // 输出一点东西，表示我们已经进入了loader
    // Safety: 在主要逻辑中，我们仅创建一个VgaText
    let mut vga = unsafe { VgaText::new() };
    writeln!(vga, "COS Entered 32-bit mode").unwrap();

    // Safety: boot传递给我们的指针，一定可读
    let mut memory_region = unsafe {
        slice::from_raw_parts_mut(bios_info.memory, bios_info.memory_region_size as usize)
    };

    // 规整内存
    memory_region = normalize_memory_region(memory_region);
    writeln!(vga, "memory ptr: {:?}", memory_region.as_ptr()).unwrap();
    for memory_region in memory_region.iter() {
        writeln!(
            vga,
            "memory: 0x{:x} - 0x{:x}",
            memory_region.base_addr,
            memory_region.base_addr + memory_region.length
        )
        .unwrap();
    }

    // 检测CPU是否为64位
    if !test_cpu_is_support_64bit() {
        panic!("cpu is not support 64bit mode");
    }

    // 开启sse
    enable_sse();

    // 加载内核
    load_kernel(bios_info.startup_disk);
    writeln!(vga, "finish read kernel").unwrap();

    // 启用长模式
    // Safety: 我们已经加载完内核，并判断CPU支持长模式，可以进入长模式
    unsafe {
        enable_64bit_mode(memory_region, bios_info.startup_disk);
    }
}

fn enable_sse() {
    unsafe {
        // CR0 控制寄存器：清除 EM（bit 2）位，允许 x87/SSE
        let mut cr0: u32;
        asm!("mov {}, cr0", out(reg) cr0);
        cr0 &= !(1 << 2); // 清除 EM 位
        cr0 |= 1 << 1; // 设置 MP 位（可选，用于 x87）
        asm!("mov cr0, {}", in(reg) cr0);

        // CR4 控制寄存器：开启 OSFXSR (bit 9) 和 OSXMMEXCPT (bit 10)
        let mut cr4: u32;
        asm!("mov {}, cr4", out(reg) cr4);
        cr4 |= (1 << 9) | (1 << 10);
        asm!("mov cr4, {}", in(reg) cr4);
    }
}

fn loop_halt() -> ! {
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut vga = unsafe { VgaText::new() };
    writeln!(vga, "loader panic: {}", info.message()).unwrap();

    if let Some(loc) = info.location() {
        writeln!(vga, "file: {}", loc.file()).unwrap();
        writeln!(vga, "line {} col {}", loc.line(), loc.column()).unwrap();
    }

    loop_halt()
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
