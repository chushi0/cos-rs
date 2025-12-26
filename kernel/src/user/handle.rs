use alloc::{boxed::Box, sync::Weak};
use async_locks::watch;
use filesystem::fs::FileHandle;

use crate::{multitask::process::Process, sync::spin::SpinLock};

pub enum HandleObject {
    Process {
        process: Weak<SpinLock<Process>>,
        exit: watch::Subscriber<u64>,
    },
    File(Box<dyn FileHandle>),
}
