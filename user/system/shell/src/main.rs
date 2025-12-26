#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

use cos_sys::multitask::exit;

#[unsafe(export_name = "_start")]
fn main() -> ! {
    unsafe {
        cos_sys::syscall!(0);
    }

    exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(3);
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
