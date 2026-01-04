#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

extern crate alloc;
extern crate rlibc;

use cos_sys::{
    debug::{get_char, put_char},
    file::{close, open, read},
    multitask::{exit, sleep_thread},
};

cos_heap::default_heap!();

#[unsafe(export_name = "_start")]
fn main() -> ! {
    print_welcome_file();
    print(b"\n> ");

    let mut buffer = [0u8; 70];
    let mut len = 0;

    loop {
        let char = get_char().expect("failed to get char");

        // 特殊char处理
        match char {
            b'\n' => {
                put_char(b'\n').expect("failed to new line");
                let should_exit = process_command(&buffer[..len]);
                if should_exit {
                    break;
                }
                len = 0;
                print(b"> ");
                continue;
            }
            0x08 => {
                if len > 0 {
                    print(&[0x08, b' ', 0x08]);
                    len -= 1;
                }
                continue;
            }
            _ => (),
        }

        // 追加到缓冲
        if len < buffer.len() {
            buffer[len] = char;
            len += 1;
            put_char(char).expect("failed to put char");
            continue;
        }
    }

    exit(0);
}

fn process_command(cmd: &[u8]) -> bool {
    if cmd.len() == 0 {
        return false;
    }

    if cmd == b"help" {
        print(b"COS Shell Helper:\n");
        print(b"  help - print this message\n");
        print(b"  exit - exit shell interactive\n");
        print(b"         (currently this will trigger kernel panic)\n");
        print(b"  echo <msg> - print message after `echo` words\n");
        print(b"\n");
        return false;
    }

    if cmd == b"exit" {
        return true;
    }

    if let Some(msg) = cmd.strip_prefix(b"echo ") {
        print(msg);
        print(b"\n");
        return false;
    }

    if let Some(time) = cmd.strip_prefix(b"sleep ") {
        if let Some(time_in_ms) = str::from_utf8(time)
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
        {
            sleep_thread(time_in_ms / 1000, time_in_ms % 1000 * 1000).unwrap();
        }
    }

    print(b"Unsupported Command, type `help` to see help message.\n\n");
    false
}

fn print(string: &[u8]) {
    for &ch in string {
        put_char(ch).expect("failed to print string");
    }
}

fn print_welcome_file() {
    let file = open(b"/system/welcome.txt").unwrap();
    let mut buffer = alloc::vec![0u8; 8192];
    loop {
        let read_count = read(file, buffer.as_mut_slice()).unwrap() as usize;
        if read_count == 0 {
            break;
        }
        print(&buffer[..read_count]);
    }
    close(file).unwrap();
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(3);
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
