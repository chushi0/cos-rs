use core::{alloc::Layout, mem::forget};

use alloc::{
    alloc::{alloc, dealloc},
    boxed::Box,
};

use crate::{clone::TryClone, error::AllocError};

pub trait TryBox<T> {
    fn try_new(value: T) -> Result<Self, AllocError>
    where
        Self: Sized;
}

pub(crate) unsafe fn dealloc_box<T>(ptr: *mut T) {
    // 对于 ZST，box默认不会申请内存，而是使用特殊指针代替，因此不能释放
    if size_of::<T>() == 0 {
        return;
    }

    unsafe {
        dealloc(ptr.cast(), Layout::new::<T>());
    }
}

impl<T> TryBox<T> for Box<T> {
    fn try_new(value: T) -> Result<Self, AllocError> {
        if size_of::<T>() == 0 {
            return Ok(Box::new(value));
        }

        unsafe {
            let layout = Layout::new::<T>();
            let p = alloc(layout) as *mut T;
            if p.is_null() {
                return Err(AllocError);
            }
            p.write(value);
            Ok(Self::from_raw(p))
        }
    }
}

impl<T: TryClone> TryClone for Box<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        (**self)
            .try_clone()
            .and_then(<Box<T> as TryBox<T>>::try_new)
    }

    fn try_clone_from(self, source: &Self) -> Result<Self, AllocError> {
        // 将box转为原始指针以备用
        let ptr = Box::into_raw(self);

        struct Guard<T> {
            ptr: *mut T,
        }

        impl<T> Drop for Guard<T> {
            fn drop(&mut self) {
                unsafe {
                    dealloc_box(self.ptr);
                }
            }
        }

        unsafe {
            // 我们需要通过Guard来回收内存，防止return Err或panic unwind时产生内存泄漏
            let guard = Guard { ptr };
            // 通过 ptr::read 读取，它仅对T区域进行逐字节复制，不调用Drop/Clone等内容
            let this = ptr.read();
            // try_clone_from会获取所有权后返回新值
            // 如果其返回Err或panic，T::drop由被调用者负责执行（rust默认保证），
            // 如果其返回新值，旧值已经被
            let this = this.try_clone_from(source)?;
            // 成功的情况，将T写入Box对应的内存。如果T存在间接引用，这一步也不会影响
            // ptr::write不会调用drop/clone，并获取所有权，因此我们实现了将所有权转移给Box指针
            ptr.write(this);
            // 忘记guard，因为我们不再需要释放box内存
            forget(guard);
            // 重新构造box并返回
            Ok(Box::from_raw(ptr))
        }
    }
}
