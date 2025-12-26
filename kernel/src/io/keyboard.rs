use core::num::NonZeroU8;

use alloc::sync::Arc;
use async_locks::{channel::spsc, mutex::Mutex};

use crate::sync::spin::SpinLock;

static mut KEYBOARD_SPSC: Option<KeyboardSpsc> = None;

const CODE_ASCII_MAPPING: [Option<NonZeroU8>; 0x80] = const_generate_code_ascii_mapping();
const CODE_ASCII_SHIFT_MAPPING: [Option<NonZeroU8>; 0x80] =
    const_generate_code_ascii_shift_mapping();

const LEFT_SHIFT: u8 = 0x2A;
const RIGHT_SHIFT: u8 = 0x36;

static SPEC_KEY_STATUS: SpinLock<SpecKeyStatus> = SpinLock::new(SpecKeyStatus::new());

struct KeyboardSpsc {
    sender: SpinLock<spsc::Sender<u8>>,
    receiver: Arc<Mutex<spsc::Receiver<u8>>>,
}

impl KeyboardSpsc {
    fn new(buffer: usize) -> Self {
        let (sender, receiver) = spsc::channel(buffer);
        Self {
            sender: SpinLock::new(sender),
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }
}

struct SpecKeyStatus {
    left_shift: bool,
    right_shift: bool,
}

impl SpecKeyStatus {
    const fn new() -> Self {
        Self {
            left_shift: false,
            right_shift: false,
        }
    }
}

pub unsafe fn init() {
    unsafe { KEYBOARD_SPSC = Some(KeyboardSpsc::new(0x80)) }
}

const fn const_generate_code_ascii_mapping() -> [Option<NonZeroU8>; 0x80] {
    let mut mapping = [None::<NonZeroU8>; 0x80];
    mapping[0x01] = NonZeroU8::new(0x1b); // Esc
    mapping[0x02] = NonZeroU8::new(b'1');
    mapping[0x03] = NonZeroU8::new(b'2');
    mapping[0x04] = NonZeroU8::new(b'3');
    mapping[0x05] = NonZeroU8::new(b'4');
    mapping[0x06] = NonZeroU8::new(b'5');
    mapping[0x07] = NonZeroU8::new(b'6');
    mapping[0x08] = NonZeroU8::new(b'7');
    mapping[0x09] = NonZeroU8::new(b'8');
    mapping[0x0A] = NonZeroU8::new(b'9');
    mapping[0x0B] = NonZeroU8::new(b'0');
    mapping[0x0C] = NonZeroU8::new(b'-');
    mapping[0x0D] = NonZeroU8::new(b'=');
    mapping[0x0E] = NonZeroU8::new(0x08); // Backspace
    mapping[0x0F] = NonZeroU8::new(b'\t');
    mapping[0x10] = NonZeroU8::new(b'q');
    mapping[0x11] = NonZeroU8::new(b'w');
    mapping[0x12] = NonZeroU8::new(b'e');
    mapping[0x13] = NonZeroU8::new(b'r');
    mapping[0x14] = NonZeroU8::new(b't');
    mapping[0x15] = NonZeroU8::new(b'y');
    mapping[0x16] = NonZeroU8::new(b'u');
    mapping[0x17] = NonZeroU8::new(b'i');
    mapping[0x18] = NonZeroU8::new(b'o');
    mapping[0x19] = NonZeroU8::new(b'p');
    mapping[0x1A] = NonZeroU8::new(b'[');
    mapping[0x1B] = NonZeroU8::new(b']');
    mapping[0x1C] = NonZeroU8::new(b'\n');
    // mapping[0x1D] CTRL
    mapping[0x1E] = NonZeroU8::new(b'a');
    mapping[0x1F] = NonZeroU8::new(b's');
    mapping[0x20] = NonZeroU8::new(b'd');
    mapping[0x21] = NonZeroU8::new(b'f');
    mapping[0x22] = NonZeroU8::new(b'g');
    mapping[0x23] = NonZeroU8::new(b'h');
    mapping[0x24] = NonZeroU8::new(b'j');
    mapping[0x25] = NonZeroU8::new(b'k');
    mapping[0x26] = NonZeroU8::new(b'l');
    mapping[0x27] = NonZeroU8::new(b';');
    mapping[0x28] = NonZeroU8::new(b'\'');
    mapping[0x29] = NonZeroU8::new(b'`');
    // mapping[0x2A] LEFT SHIFT
    mapping[0x2B] = NonZeroU8::new(b'\\');
    mapping[0x2C] = NonZeroU8::new(b'z');
    mapping[0x2D] = NonZeroU8::new(b'x');
    mapping[0x2E] = NonZeroU8::new(b'c');
    mapping[0x2F] = NonZeroU8::new(b'v');
    mapping[0x30] = NonZeroU8::new(b'b');
    mapping[0x31] = NonZeroU8::new(b'n');
    mapping[0x32] = NonZeroU8::new(b'm');
    mapping[0x33] = NonZeroU8::new(b',');
    mapping[0x34] = NonZeroU8::new(b'.');
    mapping[0x35] = NonZeroU8::new(b'/');
    // mapping[0x36] RIGHT SHIFT
    // mapping[0x37] Keypad *
    // mapping[0x38] Alt
    mapping[0x39] = NonZeroU8::new(b' ');
    // mapping[0x3A] CapsLock
    // 0x3B - 0x44 F1~F10
    // 0x45 NumLock
    mapping
}

const fn const_generate_code_ascii_shift_mapping() -> [Option<NonZeroU8>; 0x80] {
    let mut mapping = [None::<NonZeroU8>; 0x80];
    mapping[0x01] = NonZeroU8::new(0x1b); // Esc
    mapping[0x02] = NonZeroU8::new(b'!');
    mapping[0x03] = NonZeroU8::new(b'@');
    mapping[0x04] = NonZeroU8::new(b'#');
    mapping[0x05] = NonZeroU8::new(b'$');
    mapping[0x06] = NonZeroU8::new(b'%');
    mapping[0x07] = NonZeroU8::new(b'^');
    mapping[0x08] = NonZeroU8::new(b'&');
    mapping[0x09] = NonZeroU8::new(b'*');
    mapping[0x0A] = NonZeroU8::new(b'(');
    mapping[0x0B] = NonZeroU8::new(b')');
    mapping[0x0C] = NonZeroU8::new(b'_');
    mapping[0x0D] = NonZeroU8::new(b'+');
    mapping[0x0E] = NonZeroU8::new(0x08); // Backspace
    mapping[0x0F] = NonZeroU8::new(b'\t');
    mapping[0x10] = NonZeroU8::new(b'Q');
    mapping[0x11] = NonZeroU8::new(b'W');
    mapping[0x12] = NonZeroU8::new(b'E');
    mapping[0x13] = NonZeroU8::new(b'R');
    mapping[0x14] = NonZeroU8::new(b'T');
    mapping[0x15] = NonZeroU8::new(b'Y');
    mapping[0x16] = NonZeroU8::new(b'U');
    mapping[0x17] = NonZeroU8::new(b'I');
    mapping[0x18] = NonZeroU8::new(b'O');
    mapping[0x19] = NonZeroU8::new(b'P');
    mapping[0x1A] = NonZeroU8::new(b'{');
    mapping[0x1B] = NonZeroU8::new(b'}');
    mapping[0x1C] = NonZeroU8::new(b'\n');
    // mapping[0x1D] CTRL
    mapping[0x1E] = NonZeroU8::new(b'A');
    mapping[0x1F] = NonZeroU8::new(b'S');
    mapping[0x20] = NonZeroU8::new(b'D');
    mapping[0x21] = NonZeroU8::new(b'F');
    mapping[0x22] = NonZeroU8::new(b'G');
    mapping[0x23] = NonZeroU8::new(b'H');
    mapping[0x24] = NonZeroU8::new(b'J');
    mapping[0x25] = NonZeroU8::new(b'K');
    mapping[0x26] = NonZeroU8::new(b'L');
    mapping[0x27] = NonZeroU8::new(b':');
    mapping[0x28] = NonZeroU8::new(b'"');
    mapping[0x29] = NonZeroU8::new(b'~');
    // mapping[0x2A] LEFT SHIFT
    mapping[0x2B] = NonZeroU8::new(b'|');
    mapping[0x2C] = NonZeroU8::new(b'Z');
    mapping[0x2D] = NonZeroU8::new(b'X');
    mapping[0x2E] = NonZeroU8::new(b'C');
    mapping[0x2F] = NonZeroU8::new(b'V');
    mapping[0x30] = NonZeroU8::new(b'B');
    mapping[0x31] = NonZeroU8::new(b'N');
    mapping[0x32] = NonZeroU8::new(b'M');
    mapping[0x33] = NonZeroU8::new(b'<');
    mapping[0x34] = NonZeroU8::new(b'>');
    mapping[0x35] = NonZeroU8::new(b'?');
    // mapping[0x36] RIGHT SHIFT
    // mapping[0x37] Keypad *
    // mapping[0x38] Alt
    mapping[0x39] = NonZeroU8::new(b' ');
    // mapping[0x3A] CapsLock
    // 0x3B - 0x44 F1~F10
    // 0x45 NumLock
    mapping
}

pub fn handle_keyboard_scan(code: u8) {
    let pressed = (code & 0x80) == 0;
    let button = code & 0x7f;

    match (button, pressed) {
        // LShift
        (LEFT_SHIFT, pressed) => {
            SPEC_KEY_STATUS.lock().left_shift = pressed;
        }
        // RShift
        (RIGHT_SHIFT, pressed) => {
            SPEC_KEY_STATUS.lock().right_shift = pressed;
        }
        // other key pressed
        (button, true) => {
            // is shift pressed?
            let shift_pressed = {
                let key_status = SPEC_KEY_STATUS.lock();
                key_status.left_shift || key_status.right_shift
            };
            let mapping = if shift_pressed {
                &CODE_ASCII_SHIFT_MAPPING
            } else {
                &CODE_ASCII_MAPPING
            };

            if let Some(ascii) = mapping[button as usize] {
                unsafe {
                    // ignore buffer full
                    #[allow(static_mut_refs)]
                    let _ = KEYBOARD_SPSC
                        .as_ref()
                        .unwrap()
                        .sender
                        .lock()
                        .try_send(ascii.get());
                }
            }
        }
        // ignore other key not pressed
        (_, false) => (),
    }
}

pub fn receiver() -> Arc<Mutex<spsc::Receiver<u8>>> {
    unsafe {
        #[allow(static_mut_refs)]
        KEYBOARD_SPSC.as_ref().unwrap().receiver.clone()
    }
}
