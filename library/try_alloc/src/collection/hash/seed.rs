use core::sync::atomic::{AtomicU64, Ordering};

use crate::{clone::TryClone, error::AllocError};

pub trait SeedSupplier {
    fn gen_seed(&self) -> u64;
}

#[derive(Debug, Clone, Copy)]
pub struct SimpleGlobalSeed;

impl SeedSupplier for SimpleGlobalSeed {
    fn gen_seed(&self) -> u64 {
        static SEED: AtomicU64 = AtomicU64::new(0);

        let x = SEED.fetch_add(1, Ordering::Relaxed);
        x ^ (x << 13) ^ (x >> 7)
    }
}

impl TryClone for SimpleGlobalSeed {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(*self)
    }
}
