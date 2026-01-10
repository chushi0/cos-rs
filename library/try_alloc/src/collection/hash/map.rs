use core::{borrow::Borrow, hash::Hash, mem};

use alloc::vec::Vec;

use crate::{
    clone::TryClone,
    collection::hash::{KeyHasher, SeedSupplier},
    error::AllocError,
    vec::TryVec,
};

#[derive(Debug, Clone)]
pub struct HashMap<K, V, H, S> {
    vec: Vec<Entry<K, V>>,
    key_hasher: H,
    seed_supplier: S,
    seed: u64,
    len: usize,
    tombstone: usize,
}

#[derive(Debug, Clone)]
enum Entry<K, V> {
    Empty,
    Occupied { key: K, value: V },
    Tombstone,
}

impl<K, V, H, S> HashMap<K, V, H, S>
where
    K: Hash + Eq,
    H: KeyHasher,
    S: SeedSupplier,
{
    pub const fn new(key_hasher: H, seed_supplier: S) -> Self {
        Self {
            vec: Vec::new(),
            key_hasher,
            seed_supplier,
            seed: 0,
            len: 0,
            tombstone: 0,
        }
    }

    fn hash_key<Q: ?Sized>(&self, key: &Q) -> u64
    where
        K: Borrow<Q>,
        Q: Hash,
    {
        self.key_hasher.hash_with_seed(key, self.seed)
    }

    fn resize_and_rehash(&mut self, target: usize) -> Result<(), AllocError> {
        if self.vec.len() >= target {
            return Ok(());
        }

        // alloc
        let mut new_vec = <Vec<Entry<K, V>> as TryVec<Entry<K, V>>>::try_with_capacity(target)?;
        new_vec.resize_with(target, || Entry::Empty);
        mem::swap(&mut new_vec, &mut self.vec);
        self.tombstone = 0;

        // re-seed
        self.seed = self.seed_supplier.gen_seed();

        // rehash
        for entry in new_vec {
            let Entry::Occupied { key, value } = entry else {
                continue;
            };
            self.do_insert(key, value);
        }

        Ok(())
    }

    fn do_insert(&mut self, k: K, v: V) {
        assert!(self.vec.len() > 0);
        let mut slot = self.hash_key(&k) as usize;
        loop {
            let index = slot % self.vec.len();
            match &mut self.vec[index] {
                Entry::Empty => {
                    self.len += 1;
                    self.vec[index] = Entry::Occupied { key: k, value: v };
                    break;
                }
                Entry::Tombstone => {
                    self.len += 1;
                    self.tombstone -= 1;
                    self.vec[index] = Entry::Occupied { key: k, value: v };
                    break;
                }
                Entry::Occupied { key, .. } => {
                    if *key == k {
                        unreachable!()
                    }
                }
            }
            slot += 1;
        }
    }

    pub fn insert(&mut self, k: K, v: V) -> Result<Option<V>, AllocError> {
        self.grow_if_need()?;
        let removed = self.remove(&k);
        self.do_insert(k, v);
        Ok(removed)
    }

    fn grow_if_need(&mut self) -> Result<(), AllocError> {
        if self.len + 1 > (self.vec.len() as f32 * 0.75) as usize {
            self.resize_and_rehash(self.vec.len() * 2)?;
        }
        if self.tombstone > (self.vec.len() as f32 * 0.25) as usize {
            self.resize_and_rehash(self.vec.len())?;
        }
        Ok(())
    }

    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let mut slot = self.hash_key(k) as usize;
        let mut searched = 0;
        while searched < self.vec.len() {
            let index = slot % self.vec.len();
            match &self.vec[index] {
                Entry::Empty => {
                    return None;
                }
                Entry::Occupied { key, value } => {
                    if key.borrow() == k {
                        return Some(value);
                    }
                }
                Entry::Tombstone => {}
            }
            slot += 1;
            searched += 1;
        }

        None
    }

    pub fn remove<Q: ?Sized>(&mut self, k: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let mut slot = self.hash_key(k) as usize;
        let mut searched = 0;
        while searched < self.vec.len() {
            let index = slot % self.vec.len();
            match &self.vec[index] {
                Entry::Empty => {
                    return None;
                }
                Entry::Occupied { key, .. } => {
                    if key.borrow() == k {
                        let entry = mem::replace(&mut self.vec[index], Entry::Tombstone);
                        let Entry::Occupied { value, .. } = entry else {
                            unreachable!()
                        };

                        self.len -= 1;
                        self.tombstone += 1;
                        return Some(value);
                    }
                }
                Entry::Tombstone => {}
            }
            slot += 1;
            searched += 1;
        }

        None
    }
}

impl<K, V, H, S> TryClone for HashMap<K, V, H, S>
where
    K: TryClone,
    V: TryClone,
    H: TryClone,
    S: TryClone,
{
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(HashMap {
            vec: self.vec.try_clone()?,
            key_hasher: self.key_hasher.try_clone()?,
            seed_supplier: self.seed_supplier.try_clone()?,
            seed: self.seed.try_clone()?,
            len: self.len,
            tombstone: self.tombstone,
        })
    }

    fn try_clone_from(mut self, source: &Self) -> Result<Self, AllocError> {
        self.vec = self.vec.try_clone_from(&source.vec)?;
        self.key_hasher = self.key_hasher.try_clone_from(&source.key_hasher)?;
        self.seed_supplier = self.seed_supplier.try_clone_from(&source.seed_supplier)?;
        self.seed = source.seed;
        self.len = source.len;
        self.tombstone = source.tombstone;
        Ok(self)
    }
}

impl<K, V> TryClone for Entry<K, V>
where
    K: TryClone,
    V: TryClone,
{
    fn try_clone(&self) -> Result<Self, AllocError> {
        match self {
            Entry::Empty => Ok(Entry::Empty),
            Entry::Occupied { key, value } => Ok(Entry::Occupied {
                key: key.try_clone()?,
                value: value.try_clone()?,
            }),
            Entry::Tombstone => Ok(Entry::Tombstone),
        }
    }

    fn try_clone_from(self, source: &Self) -> Result<Self, AllocError> {
        match (self, source) {
            (Entry::Occupied { key: k, value: v }, Entry::Occupied { key, value }) => {
                let key = k.try_clone_from(key)?;
                let value = v.try_clone_from(value)?;
                Ok(Entry::Occupied { key, value })
            }
            _ => source.try_clone(),
        }
    }
}
