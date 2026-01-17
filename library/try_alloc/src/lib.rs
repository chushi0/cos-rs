#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod boxed;
pub mod clone;
pub mod collection;
pub mod error;
pub mod fmt;
pub mod iter;
pub mod rc;
pub mod string;
pub mod vec;

pub(crate) mod ptr;