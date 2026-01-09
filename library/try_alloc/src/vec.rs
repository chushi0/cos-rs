use core::mem::{ManuallyDrop, MaybeUninit, forget};

use alloc::{
    collections::{BinaryHeap, VecDeque},
    vec::Vec,
};

use crate::{clone::TryClone, error::AllocError, iter::TryFromIterator};

pub trait TryVec<T> {
    fn try_with_capacity(capacity: usize) -> Result<Self, AllocError>
    where
        Self: Sized;

    fn try_push(&mut self, value: T) -> Result<(), AllocError>;
}

pub trait TryVecDeque<T> {
    fn try_with_capacity(capacity: usize) -> Result<Self, AllocError>
    where
        Self: Sized;

    fn try_push_front(&mut self, value: T) -> Result<(), AllocError>;
    fn try_push_back(&mut self, value: T) -> Result<(), AllocError>;
}

pub trait TryBinaryHeap<T> {
    fn try_with_capacity(capacity: usize) -> Result<Self, AllocError>
    where
        Self: Sized;

    fn try_push(&mut self, value: T) -> Result<(), AllocError>;
}

impl<T> TryVec<T> for Vec<T> {
    fn try_with_capacity(capacity: usize) -> Result<Self, AllocError>
    where
        Self: Sized,
    {
        let mut s = Self::new();
        s.try_reserve(capacity).map_err(|_| AllocError)?;

        Ok(s)
    }

    fn try_push(&mut self, value: T) -> Result<(), AllocError> {
        self.try_reserve(1)
            .map(|_| {
                self.push(value);
            })
            .map_err(|_| AllocError)
    }
}

impl<T> TryVecDeque<T> for VecDeque<T> {
    fn try_with_capacity(capacity: usize) -> Result<Self, AllocError>
    where
        Self: Sized,
    {
        let mut s = Self::new();
        s.try_reserve(capacity).map_err(|_| AllocError)?;

        Ok(s)
    }

    fn try_push_front(&mut self, value: T) -> Result<(), AllocError> {
        self.try_reserve(1)
            .map(|_| self.push_front(value))
            .map_err(|_| AllocError)
    }

    fn try_push_back(&mut self, value: T) -> Result<(), AllocError> {
        self.try_reserve(1)
            .map(|_| self.push_back(value))
            .map_err(|_| AllocError)
    }
}

impl<T: Ord> TryBinaryHeap<T> for BinaryHeap<T> {
    fn try_with_capacity(capacity: usize) -> Result<Self, AllocError>
    where
        Self: Sized,
    {
        let mut s = Self::new();
        s.try_reserve(capacity).map_err(|_| AllocError)?;

        Ok(s)
    }

    fn try_push(&mut self, value: T) -> Result<(), AllocError> {
        self.try_reserve(1).map_err(|_| AllocError)?;
        self.push(value);

        Ok(())
    }
}

impl<T: TryClone> TryClone for Vec<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        try_clone_slice_to_vec(self)
    }

    fn try_clone_from(self, source: &Self) -> Result<Self, AllocError> {
        try_clone_slice_into_vec(self, source)
    }
}

pub(crate) fn try_clone_slice_to_vec<T: TryClone>(source: &[T]) -> Result<Vec<T>, AllocError> {
    let mut vec = <Vec<T> as TryVec<T>>::try_with_capacity(source.len())?;
    for item in source {
        vec.push(item.try_clone()?);
    }
    Ok(vec)
}

pub(crate) fn try_clone_slice_into_vec<T: TryClone>(
    mut vec: Vec<T>,
    source: &[T],
) -> Result<Vec<T>, AllocError> {
    vec.truncate(source.len());
    if source.len() > vec.len() {
        vec.try_reserve(source.len() - vec.len())
            .map_err(|_| AllocError)?;
    }

    // 原地更新
    let mut this = unsafe {
        let mut v = ManuallyDrop::new(vec);
        let ptr = v.as_mut_ptr().cast::<MaybeUninit<T>>();
        let len = v.len();
        let cap = v.capacity();
        Vec::from_raw_parts(ptr, len, cap)
    };

    struct Guard<'a, T> {
        ptr: &'a mut Vec<MaybeUninit<T>>,
        current: usize,
    }

    impl<T> Drop for Guard<'_, T> {
        fn drop(&mut self) {
            for (i, item) in self.ptr.iter_mut().enumerate() {
                if i == self.current {
                    continue;
                }

                unsafe {
                    item.assume_init_drop();
                }
            }
        }
    }

    let mut guard = Guard {
        current: this.len(),
        ptr: &mut this,
    };

    for i in 0..guard.ptr.len() {
        guard.current = i;
        let value = unsafe { guard.ptr[i].as_ptr().read() };
        let value = value.try_clone_from(&source[i])?;
        guard.ptr[i].write(value);
    }

    forget(guard);

    vec = unsafe {
        let mut v = ManuallyDrop::new(this);
        let ptr = v.as_mut_ptr().cast::<T>();
        let len = v.len();
        let cap = v.capacity();
        Vec::from_raw_parts(ptr, len, cap)
    };

    // 追加更新

    if source.len() > vec.len() {
        for i in vec.len()..source.len() {
            vec.push(source[i].try_clone()?);
        }
    }

    Ok(vec)
}

impl<T: TryClone> TryClone for VecDeque<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        let mut new_vec = <Self as TryVecDeque<T>>::try_with_capacity(self.len())?;
        for item in self {
            new_vec.push_back(item.try_clone()?);
        }
        Ok(new_vec)
    }
}

impl<T: TryClone + Ord> TryClone for BinaryHeap<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        try_clone_slice_to_vec(self.as_slice()).map(Into::into)
    }

    fn try_clone_from(self, source: &Self) -> Result<Self, AllocError> {
        try_clone_slice_into_vec(self.into_vec(), source.as_slice()).map(Into::into)
    }
}

impl<A> TryFromIterator<A> for Vec<A> {
    fn try_from_iter<T: IntoIterator<Item = A>>(iter: T) -> Result<Self, AllocError> {
        let mut iter = iter.into_iter();
        let mut vector = Vec::new();
        loop {
            let Some(next) = iter.next() else {
                break;
            };
            let (lower, _) = iter.size_hint();
            vector
                .try_reserve(lower.saturating_add(1))
                .map_err(|_| AllocError)?;
            vector.push(next);
            for _ in 0..lower {
                let Some(next) = iter.next() else {
                    break;
                };
                vector.push(next);
            }
        }
        Ok(vector)
    }
}

impl<A> TryFromIterator<A> for VecDeque<A> {
    fn try_from_iter<T: IntoIterator<Item = A>>(iter: T) -> Result<Self, AllocError> {
        <Vec<A> as TryFromIterator<A>>::try_from_iter(iter).map(Into::into)
    }
}

impl<A: Ord> TryFromIterator<A> for BinaryHeap<A> {
    fn try_from_iter<T: IntoIterator<Item = A>>(iter: T) -> Result<Self, AllocError> {
        <Vec<A> as TryFromIterator<A>>::try_from_iter(iter).map(Into::into)
    }
}
