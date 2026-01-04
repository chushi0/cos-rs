use core::ops::Deref;

use alloc::{boxed::Box, sync::Weak};
use async_locks::{mutex::Mutex, watch};
use filesystem::fs::FileHandle;

use crate::{
    multitask::{self, process::Process, thread::Thread},
    sync::spin::SpinLock,
};

pub enum HandleObject {
    Process {
        process: Weak<SpinLock<Process>>,
        exit: watch::Subscriber<u64>,
    },
    Thread {
        thread: Weak<SpinLock<Thread>>,
        exit: watch::Subscriber<u64>,
    },
    File(FileHandleObject),
}

pub struct FileHandleObject {
    handle: Option<Mutex<Box<dyn FileHandle>>>,
}

impl FileHandleObject {
    pub fn new(handle: Box<dyn FileHandle>) -> Self {
        Self {
            handle: Some(Mutex::new(handle)),
        }
    }
}

impl Drop for FileHandleObject {
    fn drop(&mut self) {
        let handle = self.handle.take().unwrap();
        multitask::async_rt::spawn(async move {
            // ignore close error, including duplicate close
            let _ = handle.lock().await.close().await;
        });
    }
}

impl Deref for FileHandleObject {
    type Target = Mutex<Box<dyn FileHandle>>;

    fn deref(&self) -> &Self::Target {
        self.handle.as_ref().unwrap()
    }
}
