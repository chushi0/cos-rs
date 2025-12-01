#![no_std]

extern crate alloc;

// 异步锁需要一个同步锁作为基础，目前先使用自旋锁，后续应替换为操作系统的内核级锁
mod spin;

type SyncLock<T> = spin::SpinLock<T>;

pub mod channel;
pub mod condvar;
pub mod mutex;
pub mod rwlock;
pub mod semaphore;
