#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

use cos_sys::multitask::{create_process, exit};

#[unsafe(export_name = "_start")]
fn main() -> ! {
    _ = create_process("/system/shell");
    exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(3);
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
