use core::{num::NonZeroUsize, ptr::NonNull};

pub const fn dangling<T>() -> NonNull<T> {
    NonNull::without_provenance(NonZeroUsize::MAX)
}

pub fn is_dangling<T: ?Sized>(ptr: *const T) -> bool {
    (ptr.cast::<()>()).addr() == usize::MAX
}
