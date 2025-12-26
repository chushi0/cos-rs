#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]

use cos_sys::{
    debug::{get_char, put_char},
    multitask::exit,
};

#[unsafe(export_name = "_start")]
fn main() -> ! {
    print(b"Welcome to COS shell!\n> ");

    let mut buffer = [0u8; 70];
    let mut len = 0;

    loop {
        let char = get_char().expect("failed to get char");

        // 特殊char处理
        match char {
            b'\n' => {
                put_char(0x1B).expect("failed to new line");
                let should_exit = process_command(&buffer[..len]);
                if should_exit {
                    break;
                }
                len = 0;
                print(b"> ");
                continue;
            }
            0x1B => {
                put_char(0x1B).expect("failed to back char");
                len -= 1;
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
    if cmd == b"exit" {
        return true;
    }
    print(b"Unsupported Command\n");
    false
}

fn print(string: &[u8]) {
    for &ch in string {
        put_char(ch).expect("failed to print string");
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(3);
}

#[cfg(test)]
fn test_runner(_test_cases: &[&dyn Fn()]) {}
