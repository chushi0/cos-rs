#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

use cos_sys::multitask::{create_process, exit, wait_process};

#[unsafe(export_name = "_start")]
fn main() -> ! {
    let mut count = 0;
    while count < 10 {
        let handle = create_process("/system/shell").expect("failed to start shell process");
        let code = wait_process(handle).expect("failed to wait for shell process");
        if code != 0 {
            panic!("shell process exit with code {code}");
        }
        count += 1;
    }

    panic!("shell exit too many times");
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(3);
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
