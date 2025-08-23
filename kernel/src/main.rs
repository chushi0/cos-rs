#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

use core::{arch::asm, panic::PanicInfo, ptr::copy_nonoverlapping, slice};

#[unsafe(no_mangle)]
pub extern "C" fn kmain() -> ! {
    const HELLO_KERNRL: &[u8] =
        b"h\x07e\x07l\x07l\x07o\x07,\x07 \x07k\x07e\x07r\x07n\x07e\x07l\x07!\x07";
    unsafe {
        let vga = slice::from_raw_parts_mut(0xb8000 as *mut u16, 25 * 80);
        vga.fill(0x0700);

        copy_nonoverlapping(
            HELLO_KERNRL.as_ptr(),
            vga.as_mut_ptr() as *mut u8,
            HELLO_KERNRL.len(),
        );
    }

    loop_hlt();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop_hlt();
}

fn loop_hlt() -> ! {
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
