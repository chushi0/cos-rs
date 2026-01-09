use core::{
    cell::Cell,
    marker::{PhantomData, PhantomPinned},
    mem::{MaybeUninit, forget},
    ops::{Bound, Range, RangeFrom, RangeFull, RangeTo},
    ptr::NonNull,
};

use alloc::borrow::ToOwned;

use crate::{
    error::AllocError,
    string::{try_clone_str_into_string, try_clone_str_to_string},
    vec::{try_clone_slice_into_vec, try_clone_slice_to_vec},
};

pub trait TryClone: Clone {
    fn try_clone(&self) -> Result<Self, AllocError>;

    fn try_clone_from(self, source: &Self) -> Result<Self, AllocError> {
        drop(self);
        source.try_clone()
    }
}

pub trait TryToOwned: ToOwned {
    fn try_to_owned(&self) -> Result<Self::Owned, AllocError>;

    fn try_clone_into(&self, target: Self::Owned) -> Result<Self::Owned, AllocError> {
        drop(target);
        self.try_to_owned()
    }
}

macro_rules! impl_try_clone {
    ($($t:ty)*) => {
        $(
            impl TryClone for $t {
                fn try_clone(&self) -> Result<Self, AllocError> {
                    Ok(*self)
                }
            }
        )*
    };
}

impl_try_clone! {
    usize u8 u16 u32 u64 u128
    isize i8 i16 i32 i64 i128
    f32 f64
    bool char
}

impl<T> TryClone for *const T {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(*self)
    }
}
impl<T> TryClone for *mut T {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(*self)
    }
}
impl<T> TryClone for &T {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(*self)
    }
}
impl<T> TryClone for NonNull<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(*self)
    }
}

impl<T> TryClone for PhantomData<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(*self)
    }
}
impl TryClone for PhantomPinned {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(*self)
    }
}

impl<T: Copy> TryClone for Cell<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(self.clone())
    }
}

macro_rules! impl_try_clone_tuple {
    ($($i:tt:$n:ident),*) => {
        impl<$($n:TryClone,)*> TryClone for ($($n,)*) {
        fn try_clone(&self) -> Result<Self, AllocError> {
            Ok((
                $(
                    self.$i.try_clone()?,
                )*
            ))
        }

        fn try_clone_from(self, #[allow(unused)] source: &Self) -> Result<Self, AllocError> {
            Ok((
                $(
                    self.$i.try_clone_from(&source.$i)?,
                )*
            ))
        }
        }
    };
}

impl_try_clone_tuple!();
impl_try_clone_tuple!(0:A);
impl_try_clone_tuple!(0:A,1:B);
impl_try_clone_tuple!(0:A,1:B,2:C);
impl_try_clone_tuple!(0:A,1:B,2:C,3:D);
impl_try_clone_tuple!(0:A,1:B,2:C,3:D,4:E);
impl_try_clone_tuple!(0:A,1:B,2:C,3:D,4:E,5:F);
impl_try_clone_tuple!(0:A,1:B,2:C,3:D,4:E,5:F,6:G);
impl_try_clone_tuple!(0:A,1:B,2:C,3:D,4:E,5:F,6:G,7:H);

impl<T: TryClone> TryClone for Range<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(Self {
            start: self.start.try_clone()?,
            end: self.end.try_clone()?,
        })
    }

    fn try_clone_from(mut self, source: &Self) -> Result<Self, AllocError> {
        self.start = self.start.try_clone_from(&source.start)?;
        self.end = self.end.try_clone_from(&source.end)?;

        Ok(self)
    }
}

impl<T: TryClone> TryClone for RangeFrom<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(Self {
            start: self.start.try_clone()?,
        })
    }

    fn try_clone_from(mut self, source: &Self) -> Result<Self, AllocError> {
        self.start = self.start.try_clone_from(&source.start)?;

        Ok(self)
    }
}

impl<T: TryClone> TryClone for RangeTo<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(Self {
            end: self.end.try_clone()?,
        })
    }

    fn try_clone_from(mut self, source: &Self) -> Result<Self, AllocError> {
        self.end = self.end.try_clone_from(&source.end)?;

        Ok(self)
    }
}

impl TryClone for RangeFull {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(*self)
    }
}

impl<T: TryClone> TryClone for Bound<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        match self {
            Bound::Included(value) => value.try_clone().map(Bound::Included),
            Bound::Excluded(value) => value.try_clone().map(Bound::Excluded),
            Bound::Unbounded => Ok(Bound::Unbounded),
        }
    }
}

impl<T: TryClone, const N: usize> TryClone for [T; N] {
    fn try_clone(&self) -> Result<Self, AllocError> {
        let mut array = [const { MaybeUninit::uninit() }; N];

        struct Guard<'a, T> {
            array: &'a mut [MaybeUninit<T>],
            offset: usize,
        }

        impl<T> Drop for Guard<'_, T> {
            fn drop(&mut self) {
                for i in 0..self.offset {
                    unsafe {
                        self.array[i].assume_init_drop();
                    }
                }
            }
        }

        let mut guard = Guard {
            array: &mut array,
            offset: 0,
        };

        for i in 0..N {
            guard.array[i].write(self[i].try_clone()?);
            guard.offset += 1;
        }

        forget(guard);

        unsafe { Ok(array.as_ptr().cast::<[T; N]>().read()) }
    }
}

impl<T: TryClone> TryClone for Option<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        match self {
            Some(t) => t.try_clone().map(Some),
            None => Ok(None),
        }
    }

    fn try_clone_from(self, source: &Self) -> Result<Self, AllocError> {
        match (self, source) {
            (None, _) | (_, None) => Ok(None),
            (Some(dst), Some(src)) => dst.try_clone_from(src).map(Some),
        }
    }
}

impl<T: TryClone, E: TryClone> TryClone for Result<T, E> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        match self {
            Ok(t) => t.try_clone().map(Ok),
            Err(e) => e.try_clone().map(Err),
        }
    }

    fn try_clone_from(self, source: &Self) -> Result<Self, AllocError> {
        match (self, source) {
            (Ok(dst), Ok(src)) => dst.try_clone_from(src).map(Ok),
            (Ok(_), Err(e)) => e.try_clone().map(Err),
            (Err(_), Ok(t)) => t.try_clone().map(Ok),
            (Err(dst), Err(src)) => dst.try_clone_from(src).map(Err),
        }
    }
}

impl<T: TryClone> TryToOwned for T {
    fn try_to_owned(&self) -> Result<Self::Owned, AllocError> {
        self.try_clone()
    }

    fn try_clone_into(&self, target: Self::Owned) -> Result<Self::Owned, AllocError> {
        target.try_clone_from(self)
    }
}

impl<T: TryClone> TryToOwned for [T] {
    fn try_to_owned(&self) -> Result<Self::Owned, AllocError> {
        try_clone_slice_to_vec(self)
    }

    fn try_clone_into(&self, target: Self::Owned) -> Result<Self::Owned, AllocError> {
        try_clone_slice_into_vec(target, self)
    }
}

impl TryToOwned for str {
    fn try_to_owned(&self) -> Result<Self::Owned, AllocError> {
        try_clone_str_to_string(self)
    }

    fn try_clone_into(&self, target: Self::Owned) -> Result<Self::Owned, AllocError> {
        try_clone_str_into_string(target, self)
    }
}
