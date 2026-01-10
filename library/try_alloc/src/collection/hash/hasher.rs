use core::hash::{Hash, Hasher};

use crate::{clone::TryClone, error::AllocError};

pub trait KeyHasher {
    fn hash_with_seed<H: Hash>(&self, key: H, seed: u64) -> u64;
}

#[derive(Debug, Clone, Copy)]
pub struct RollingKeyHasher;

struct RollingHasher {
    val: u64,
    seed: u64,
}

impl KeyHasher for RollingKeyHasher {
    fn hash_with_seed<H: Hash>(&self, key: H, seed: u64) -> u64 {
        let mut hasher = RollingHasher { val: 0, seed };
        key.hash(&mut hasher);
        hasher.finish()
    }
}

impl Hasher for RollingHasher {
    fn finish(&self) -> u64 {
        self.val
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.val = self
                .val
                .wrapping_mul(31)
                .wrapping_add((byte as u64).wrapping_mul(self.seed));
            self.seed = self.seed.rotate_left(7).wrapping_mul(31);
        }
    }
}

impl TryClone for RollingKeyHasher {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(*self)
    }
}
