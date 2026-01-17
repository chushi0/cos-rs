//! 可分配失败的引用计数，[TryRc] 为单线程计数，[TryArc] 为多线程计数。
//!
//! 与 [alloc::rc::Rc] 和 [alloc::sync::Arc] 相同，[TryRc] 和 [TryArc]
//! 均实现了弱引用、原始指针转换等功能。除了在创建引用计数时可能失败以外，用法没有区别。
//!
//! 受限于stable rust，当前实现不支持 DST。如果需要DST，
//! 请考虑 [TryRc<Box<dyn Trait>>] 和 [TryArc<Box<dyn Trait>>]

use core::{
    cell::Cell,
    marker::PhantomData,
    mem::{forget, offset_of},
    ops::Deref,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::boxed::Box;

use crate::{
    boxed::{TryBox, dealloc_box},
    clone::TryClone,
    error::AllocError,
    ptr::{dangling, is_dangling},
};

/// 共享强引用的通用表示
///
/// 一个强引用可以通过 [AsRef] 或 [Deref] 获取其原始值的不可变借用。
/// 无法通过强引用获取可变借用。
pub trait StrongLike<T>: AsRef<T> + Deref<Target = T> + Clone {
    /// 此强引用对应的弱引用类型
    type Weak: WeakLike<T>;

    /// 尝试在堆上创建一个强引用
    ///
    /// 如果分配失败，返回 `Err(AllocError)`
    fn try_new(value: T) -> Result<Self, AllocError>
    where
        Self: Sized;

    /// 将此强引用转为T的原始指针
    ///
    /// 生成原始指针时，不改变引用计数
    fn into_raw(this: Self) -> *const T;

    /// 将T的原始指针转回强引用
    ///
    /// 恢复强引用时，不改变引用计数。
    /// 参数必须是此前通过 [into_raw] 产生的指针。
    unsafe fn from_raw(ptr: *const T) -> Self;

    /// 获取对应的弱引用
    fn downgrade(this: &Self) -> Self::Weak;
}

/// 共享强引用的通用表示
///
/// 无法通过弱引用获取值。必须先通过 [WeakLike::upgrade] 获取强引用，
/// 然后才能获取值。
pub trait WeakLike<T>: Clone {
    /// 对应的强引用
    type Strong: StrongLike<T>;

    /// 创建一个不关联任何对象的弱引用
    fn new() -> Self;

    /// 将此弱引用转为T的原始指针
    ///
    /// 生成原始指针时，不改变引用计数。
    fn into_raw(this: Self) -> *const T;

    /// 将T的原始指针转回弱引用
    ///
    /// 恢复弱引用时，不改变引用计数。
    /// 参数必须是此前通过 [into_raw] 产生的指针。
    unsafe fn from_raw(ptr: *const T) -> Self;

    /// 获取对应的强引用
    fn upgrade(&self) -> Option<Self::Strong>;
}

trait InnerLike<T> {
    fn inc_strong(&self) -> bool;
    fn inc_weak(&self);
    fn dec_strong(&self) -> bool;
    fn dec_weak(&self) -> bool;

    unsafe fn drop_value_in_place(this: *mut Self);
}

struct RcInner<T> {
    strong: Cell<usize>,
    weak: Cell<usize>,
    value: T,
}

pub struct TryRc<T> {
    ptr: NonNull<RcInner<T>>,
    phantom: PhantomData<RcInner<T>>,
}

pub struct WeakRc<T> {
    ptr: NonNull<RcInner<T>>,
}

struct ArcInner<T> {
    strong: AtomicUsize,
    weak: AtomicUsize,
    value: T,
}

pub struct TryArc<T> {
    ptr: NonNull<ArcInner<T>>,
    phantom: PhantomData<ArcInner<T>>,
}

pub struct WeakArc<T> {
    ptr: NonNull<ArcInner<T>>,
}

unsafe impl<T: Send + Sync> Send for TryArc<T> {}
unsafe impl<T: Send + Sync> Sync for TryArc<T> {}
unsafe impl<T: Send + Sync> Send for WeakArc<T> {}
unsafe impl<T: Send + Sync> Sync for WeakArc<T> {}

impl<T> TryRc<T> {
    #[inline(never)]
    unsafe fn drop_slow(&mut self) {
        let _weak = WeakRc { ptr: self.ptr };

        unsafe { RcInner::drop_value_in_place(self.ptr.as_ptr()) }
    }
}

impl<T> WeakRc<T> {
    #[inline(never)]
    unsafe fn drop_slow(&mut self) {
        unsafe {
            dealloc_box(self.ptr.as_ptr());
        }
    }
}

impl<T> AsRef<T> for TryRc<T> {
    fn as_ref(&self) -> &T {
        unsafe { &self.ptr.as_ref().value }
    }
}

impl<T> Deref for TryRc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<T> StrongLike<T> for TryRc<T> {
    type Weak = WeakRc<T>;

    fn try_new(value: T) -> Result<Self, AllocError>
    where
        Self: Sized,
    {
        let inner = <Box<RcInner<T>> as TryBox<RcInner<T>>>::try_new(RcInner {
            strong: Cell::new(1),
            weak: Cell::new(2),
            value,
        })?;
        let inner = Box::leak(inner).into();
        Ok(Self {
            ptr: inner,
            phantom: PhantomData,
        })
    }

    fn into_raw(this: Self) -> *const T {
        unsafe {
            let ptr = this.ptr.as_ptr().byte_add(offset_of!(RcInner<T>, value));

            forget(this);

            ptr as *const T
        }
    }

    unsafe fn from_raw(ptr: *const T) -> Self {
        unsafe {
            let ptr = ptr.byte_sub(offset_of!(RcInner<T>, value));
            Self {
                ptr: NonNull::new(ptr as *mut RcInner<T>).unwrap(),
                phantom: PhantomData,
            }
        }
    }

    fn downgrade(this: &Self) -> Self::Weak {
        let inner = this.ptr;
        unsafe {
            inner.as_ref().inc_weak();
        }
        Self::Weak { ptr: inner }
    }
}

impl<T> Clone for TryRc<T> {
    fn clone(&self) -> Self {
        unsafe {
            self.ptr.as_ref().inc_strong();
        }

        Self {
            ptr: self.ptr,
            phantom: PhantomData,
        }
    }
}

impl<T> TryClone for TryRc<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(self.clone())
    }
}

impl<T> Drop for TryRc<T> {
    fn drop(&mut self) {
        unsafe {
            let strong_dropped = (*self.ptr.as_ptr()).dec_strong();
            if strong_dropped {
                self.drop_slow();
            }
        }
    }
}

impl<T> WeakLike<T> for WeakRc<T> {
    type Strong = TryRc<T>;

    fn new() -> Self {
        Self { ptr: dangling() }
    }

    fn into_raw(this: Self) -> *const T {
        unsafe {
            let ptr = this.ptr.as_ptr().byte_add(offset_of!(RcInner<T>, value));

            forget(this);

            ptr as *const T
        }
    }

    unsafe fn from_raw(ptr: *const T) -> Self {
        unsafe {
            let ptr = ptr.byte_sub(offset_of!(RcInner<T>, value));
            Self {
                ptr: NonNull::new(ptr as *mut RcInner<T>).unwrap(),
            }
        }
    }

    fn upgrade(&self) -> Option<Self::Strong> {
        if is_dangling(self.ptr.as_ptr()) {
            return None;
        }

        unsafe {
            if self.ptr.as_ref().inc_strong() {
                Some(Self::Strong {
                    ptr: self.ptr,
                    phantom: PhantomData,
                })
            } else {
                None
            }
        }
    }
}

impl<T> Clone for WeakRc<T> {
    fn clone(&self) -> Self {
        if !is_dangling(self.ptr.as_ptr()) {
            unsafe {
                self.ptr.as_ref().inc_weak();
            }
        }
        Self { ptr: self.ptr }
    }
}

impl<T> TryClone for WeakRc<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(self.clone())
    }
}

impl<T> Drop for WeakRc<T> {
    fn drop(&mut self) {
        if is_dangling(self.ptr.as_ptr()) {
            return;
        }

        unsafe {
            let weak_dropped = self.ptr.as_ref().dec_weak();
            if weak_dropped {
                self.drop_slow();
            }
        }
    }
}

impl<T> InnerLike<T> for RcInner<T> {
    fn inc_strong(&self) -> bool {
        let s = self.strong.get();
        if s > 0 {
            self.strong.set(self.strong.get() + 1);
            self.weak.set(self.weak.get() + 1);

            // overflow check
            assert!(self.strong.get() != 0);
            assert!(self.weak.get() != 0);

            true
        } else {
            false
        }
    }

    fn inc_weak(&self) {
        self.weak.set(self.weak.get() + 1);

        // overflow check
        assert!(self.weak.get() != 0);
    }

    fn dec_strong(&self) -> bool {
        assert!(self.strong.get() != 0);
        assert!(self.weak.get() != 0);

        self.strong.set(self.strong.get() - 1);
        self.weak.set(self.weak.get() - 1);

        self.strong.get() == 0
    }

    fn dec_weak(&self) -> bool {
        assert!(self.weak.get() != 0);

        self.weak.set(self.weak.get() - 1);
        self.weak.get() == 0
    }

    unsafe fn drop_value_in_place(this: *mut Self) {
        unsafe {
            core::ptr::drop_in_place(&mut (*this).value);
        }
    }
}

impl<T> TryArc<T> {
    #[inline(never)]
    unsafe fn drop_slow(&mut self) {
        let _weak = WeakArc { ptr: self.ptr };

        unsafe { ArcInner::drop_value_in_place(self.ptr.as_ptr()) }
    }
}

impl<T> WeakArc<T> {
    #[inline(never)]
    unsafe fn drop_slow(&mut self) {
        unsafe {
            dealloc_box(self.ptr.as_ptr());
        }
    }
}

impl<T> AsRef<T> for TryArc<T> {
    fn as_ref(&self) -> &T {
        unsafe { &self.ptr.as_ref().value }
    }
}

impl<T> Deref for TryArc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<T> StrongLike<T> for TryArc<T> {
    type Weak = WeakArc<T>;

    fn try_new(value: T) -> Result<Self, AllocError>
    where
        Self: Sized,
    {
        let inner = <Box<ArcInner<T>> as TryBox<ArcInner<T>>>::try_new(ArcInner {
            strong: AtomicUsize::new(1),
            weak: AtomicUsize::new(2),
            value,
        })?;
        let inner = Box::leak(inner).into();
        Ok(Self {
            ptr: inner,
            phantom: PhantomData,
        })
    }

    fn into_raw(this: Self) -> *const T {
        unsafe {
            let ptr = this.ptr.as_ptr().byte_add(offset_of!(ArcInner<T>, value));

            forget(this);

            ptr as *const T
        }
    }

    unsafe fn from_raw(ptr: *const T) -> Self {
        unsafe {
            let ptr = ptr.byte_sub(offset_of!(ArcInner<T>, value));
            Self {
                ptr: NonNull::new(ptr as *mut ArcInner<T>).unwrap(),
                phantom: PhantomData,
            }
        }
    }

    fn downgrade(this: &Self) -> Self::Weak {
        let inner = this.ptr;
        unsafe {
            inner.as_ref().inc_weak();
        }
        Self::Weak { ptr: inner }
    }
}

impl<T> Clone for TryArc<T> {
    fn clone(&self) -> Self {
        unsafe {
            self.ptr.as_ref().inc_strong();
        }

        Self {
            ptr: self.ptr,
            phantom: PhantomData,
        }
    }
}

impl<T> TryClone for TryArc<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(self.clone())
    }
}

impl<T> Drop for TryArc<T> {
    fn drop(&mut self) {
        unsafe {
            let strong_dropped = (*self.ptr.as_ptr()).dec_strong();
            if strong_dropped {
                self.drop_slow();
            }
        }
    }
}

impl<T> WeakLike<T> for WeakArc<T> {
    type Strong = TryArc<T>;

    fn new() -> Self {
        Self { ptr: dangling() }
    }

    fn into_raw(this: Self) -> *const T {
        unsafe {
            let ptr = this.ptr.as_ptr().byte_add(offset_of!(ArcInner<T>, value));

            forget(this);

            ptr as *const T
        }
    }

    unsafe fn from_raw(ptr: *const T) -> Self {
        unsafe {
            let ptr = ptr.byte_sub(offset_of!(ArcInner<T>, value));
            Self {
                ptr: NonNull::new(ptr as *mut ArcInner<T>).unwrap(),
            }
        }
    }

    fn upgrade(&self) -> Option<Self::Strong> {
        if is_dangling(self.ptr.as_ptr()) {
            return None;
        }

        unsafe {
            if self.ptr.as_ref().inc_strong() {
                Some(Self::Strong {
                    ptr: self.ptr,
                    phantom: PhantomData,
                })
            } else {
                None
            }
        }
    }
}

impl<T> Clone for WeakArc<T> {
    fn clone(&self) -> Self {
        if !is_dangling(self.ptr.as_ptr()) {
            unsafe {
                self.ptr.as_ref().inc_weak();
            }
        }
        Self { ptr: self.ptr }
    }
}

impl<T> TryClone for WeakArc<T> {
    fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(self.clone())
    }
}

impl<T> Drop for WeakArc<T> {
    fn drop(&mut self) {
        if is_dangling(self.ptr.as_ptr()) {
            return;
        }

        unsafe {
            let weak_dropped = self.ptr.as_ref().dec_weak();
            if weak_dropped {
                self.drop_slow();
            }
        }
    }
}

impl<T> InnerLike<T> for ArcInner<T> {
    fn inc_strong(&self) -> bool {
        #[inline]
        fn checked_increment(n: usize) -> Option<usize> {
            if n == 0 {
                return None;
            }
            assert!(n < usize::MAX);
            Some(n + 1)
        }

        if self
            .strong
            .fetch_update(Ordering::Acquire, Ordering::Relaxed, checked_increment)
            .is_ok()
        {
            self.weak.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    fn inc_weak(&self) {
        self.weak.fetch_add(1, Ordering::Relaxed);
    }

    fn dec_strong(&self) -> bool {
        self.weak.fetch_sub(1, Ordering::Relaxed);
        self.strong.fetch_sub(1, Ordering::AcqRel) == 1
    }

    fn dec_weak(&self) -> bool {
        self.weak.fetch_sub(1, Ordering::AcqRel) == 1
    }

    unsafe fn drop_value_in_place(this: *mut Self) {
        unsafe {
            core::ptr::drop_in_place(&mut (*this).value);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::ptr;

    /// 通用测试逻辑
    fn strong_weak_basic<S, W>()
    where
        S: StrongLike<i32, Weak = W>,
        W: WeakLike<i32, Strong = S>,
    {
        // try_new / AsRef / Deref
        let s = S::try_new(123).expect("alloc failed in test");
        assert_eq!(*s, 123);
        assert_eq!(*s.as_ref(), 123);

        // downgrade / upgrade
        let w = S::downgrade(&s);
        let s2 = w.upgrade().expect("upgrade should succeed");
        assert_eq!(*s2, 123);

        // clone strong
        let s3 = s.clone();
        assert_eq!(*s3, 123);

        // into_raw / from_raw (strong)
        let raw = S::into_raw(s2);
        assert!(!raw.is_null());

        let s4 = unsafe { S::from_raw(raw) };
        assert_eq!(*s4, 123);

        // 原始指针应稳定
        let raw2 = S::into_raw(s4.clone());
        assert!(ptr::eq(raw, raw2));
        let _ = unsafe { S::from_raw(raw2) };

        // into_raw / from_raw (weak)
        let w2 = S::downgrade(&s3);
        let raw_w = W::into_raw(w2.clone());
        assert!(!raw_w.is_null());

        let w3 = unsafe { W::from_raw(raw_w) };
        let s5 = w3.upgrade().expect("upgrade from restored weak");
        assert_eq!(*s5, 123);
    }

    /// weak 在最后一个 strong drop 后应失效
    fn weak_expires<S, W>()
    where
        S: StrongLike<i32, Weak = W>,
        W: WeakLike<i32, Strong = S>,
    {
        let s = S::try_new(7).unwrap();
        let w = S::downgrade(&s);

        drop(s);
        assert!(w.upgrade().is_none());
    }

    struct Tracker(*const AtomicUsize);

    impl Drop for Tracker {
        fn drop(&mut self) {
            unsafe {
                (*self.0).fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    fn drop_timing<S, W>()
    where
        S: StrongLike<Tracker, Weak = W>,
        W: WeakLike<Tracker, Strong = S>,
    {
        let drops: AtomicUsize = AtomicUsize::new(0);

        let w: W;

        {
            let s1 = S::try_new(Tracker(&raw const drops)).unwrap();
            let s2 = s1.clone();
            w = S::downgrade(&s1);

            assert_eq!(drops.load(Ordering::SeqCst), 0);

            drop(s1);
            assert_eq!(drops.load(Ordering::SeqCst), 0);

            drop(s2);
            assert_eq!(drops.load(Ordering::SeqCst), 1);

            assert!(w.upgrade().is_none());
        }

        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn tryrc_basic() {
        strong_weak_basic::<TryRc<i32>, WeakRc<i32>>();
    }

    #[test]
    fn tryrc_weak_expires() {
        weak_expires::<TryRc<i32>, WeakRc<i32>>();
    }

    #[test]
    fn tryrc_drop_timing() {
        drop_timing::<TryRc<Tracker>, WeakRc<Tracker>>();
    }

    #[test]
    fn tryarc_basic() {
        strong_weak_basic::<TryArc<i32>, WeakArc<i32>>();
    }

    #[test]
    fn tryarc_weak_expires() {
        weak_expires::<TryArc<i32>, WeakArc<i32>>();
    }

    #[test]
    fn tryarc_drop_timing() {
        drop_timing::<TryArc<Tracker>, WeakArc<Tracker>>();
    }
}
