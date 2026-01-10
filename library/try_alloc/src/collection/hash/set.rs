use core::{borrow::Borrow, hash::Hash};

use crate::{
    clone::TryClone,
    collection::hash::{HashMap, KeyHasher, SeedSupplier},
    error::AllocError,
};

#[derive(Debug, Clone)]
pub struct HashSet<V, H, S>(HashMap<V, (), H, S>);

impl<V, H, S> HashSet<V, H, S>
where
    V: Hash + Eq,
    H: KeyHasher,
    S: SeedSupplier,
{
    pub const fn new(key_hasher: H, seed_supplier: S) -> Self {
        Self(HashMap::new(key_hasher, seed_supplier))
    }

    pub fn insert(&mut self, v: V) -> Result<(), AllocError> {
        self.0.insert(v, ())?;
        Ok(())
    }

    pub fn contains<Q: ?Sized>(&self, v: &Q) -> bool
    where
        V: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.0.get(v).is_some()
    }

    pub fn remove<Q: ?Sized>(&mut self, v: &Q) -> bool
    where
        V: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.0.remove(v).is_some()
    }
}

impl<V, H, S> TryClone for HashSet<V, H, S>
where
    V: TryClone,
    H: TryClone,
    S: TryClone,
{
    fn try_clone(&self) -> Result<Self, AllocError> {
        self.0.try_clone().map(Self)
    }

    fn try_clone_from(self, source: &Self) -> Result<Self, AllocError> {
        self.0.try_clone_from(&source.0).map(Self)
    }
}
