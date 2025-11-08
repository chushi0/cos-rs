//! 文件系统的实现
//!
//! 此crate实现了文件系统的核心功能，包括文件创建、删除、修改等操作。
//!
//! 此crate没有使用特权指令——它仅实现了文件系统的逻辑部分，没有实现块设备的访问操作。
//! 对于用户态程序，可以使用文件来模拟块设备。对于内核态程序，可以使用特权指令直接对磁盘设备进行操作。
#![no_std]
use core::pin::Pin;

use alloc::boxed::Box;

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod device;
pub mod fs;
pub mod path;

#[allow(unused)]
pub(crate) mod internal;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(test)]
pub(crate) fn run_task<F: Future>(f: F) -> F::Output {
    use core::{
        pin::pin,
        task::{Context, Poll, Waker},
    };

    let waker = Waker::noop();
    let mut ctx = Context::from_waker(&waker);
    let mut f = pin!(f);
    loop {
        match f.as_mut().poll(&mut ctx) {
            Poll::Ready(v) => return v,
            Poll::Pending => {}
        }
    }
}
