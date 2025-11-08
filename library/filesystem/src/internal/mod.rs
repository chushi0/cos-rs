use core::{
    marker::PhantomData,
    ptr::{copy_nonoverlapping, read},
};

use alloc::vec::Vec;

/// 便于进行字节与结构体转换的工具
///
/// 对于文件系统来说，经常需要将内存中的结构体保存到磁盘，或将磁盘中的数据读取为结构体。
/// 这可以看作结构体与字节数组的转换。[DiskStruct]提供了此转换的一个安全封装。
///
/// 对于需要这种操作的结构体而言，它必须满足以下条件：
///
///  1. 是 Sized 的。显然DST类型不能直接进行存储，通常需要转换为其他结构。
///  2. 是 #[repr(C, packed)] 的。这保证了字段排列始终符合预期，不会被意外改变。
///  3. T 不能包含任何实现Drop的类型
///
/// Rust的类型系统可以保证第1点。#[repr(packed)]的align限制会通过assert检查。
/// 其他条件并没有进行检查，均由调用者保证。
///
/// 使用方需要额外注意的是，任何字节排列都必须是结构体的一个合法值，因此不能出现包括但不限于这些类型：
///  - [`bool`]
///  - [`char`]
///  - `&T` / `&mut T`
///  - enum 类型，包括 [`Option<T>`]、[`Result<T>`] 等
///  - `fn(...) -> ...`
///  - [`core::num::NonZero<T>`]
///  - [`core::ptr::NonNull<T>`]
///
/// ```ignore
/// #[repr(C, packed)]
/// #[derive(Debug)]
/// struct S {
///     a: i32,
///     b: [u8; 2],
///     c: u32,
/// }
///
/// let mut disk_struct = DiskStruct::<S>::new(size_of::<S>());
/// disk_struct.as_slice_mut()[4] = 1;
/// assert_eq!(disk_struct.as_struct().b, [1, 0]);
/// ```
///
pub struct DiskStruct<T> {
    bytes: Vec<u8>,
    _phantom: PhantomData<T>,
}

impl<T: Sized> DiskStruct<T> {
    /// Safety: 调用者必须保证
    /// 1. T 是#[repr(C, packed)]的
    /// 2. T 没有实现 Drop
    pub unsafe fn new(len: usize) -> Self {
        // T 应当 #[repr(packed)]
        // 使用const或许能让这个检查优化为零成本？需要验证
        const {
            assert!(align_of::<T>() == 1);
        }

        Self {
            bytes: alloc::vec![0u8; len.max(size_of::<T>())],
            _phantom: PhantomData,
        }
    }

    /// Safety: 调用者必须保证
    /// 1. T 是#[repr(C, packed)]的
    /// 2. T 没有实现 Drop
    pub unsafe fn from_struct(len: usize, struct_ref: &T) -> Self {
        // Safety: 由调用方保证[Self::new]的安全性
        let mut disk_struct = unsafe { Self::new(len) };

        // Safety: 由于我们已经满足了memcpy的约束，所以是安全的
        // 1. src和dst均通过引用创建
        // 2. src和dst均已对齐（u8的对齐需求为1）
        // 3. src和dst的大小均满足我们复制的数量size_of::<T>
        // 4. 将T视为u8读取是安全的（没有写入）
        // 注意这里不能使用 [`Self::as_struct_mut`]，因为我们并不知道0是否为T的有效值，
        // 而产生一个无效值的可变引用是未定义行为，即便我们在不读取它的情况下立刻使用有效值重新赋值
        unsafe {
            copy_nonoverlapping(
                struct_ref as *const T as *const u8,
                disk_struct.as_slice_mut().as_mut_ptr(),
                size_of::<T>(),
            );
        }
        disk_struct
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    /// Safety: 调用方需确保当前字节数据不会产生T的无效值
    pub unsafe fn as_struct(&self) -> &T {
        // Safety: 由于我们已经满足了下面的约束，所以产生&T是安全的
        // 1. [`new`]函数已保证数组大小至少为T的大小
        // 2. [`new`]函数已保证T的对齐需求为1，任意地址都满足这个约束
        // 3. 调用方已保证当前数据不会产生无效值
        unsafe { &*(self.bytes.as_ptr() as *const T) }
    }

    /// Safety: 调用方需确保当前字节数据不会产生T的无效值
    pub unsafe fn as_struct_mut(&mut self) -> &mut T {
        // Safety: 由于我们已经满足了下面的约束，所以产生&mut T是安全的
        // 1. [`new`]函数已保证数组大小至少为T的大小
        // 2. [`new`]函数已保证T的对齐需求为1，任意地址都满足这个约束
        // 3. 调用方已保证当前数据不会产生无效值
        unsafe { &mut *(self.bytes.as_mut_ptr() as *mut T) }
    }

    /// Safety: 调用方需确保当前字节数据不会产生T的无效值
    pub unsafe fn into_inner(self) -> T {
        // Safety: 由于我们已经满足了下面的约束，所以产生T是安全的
        // 1. [`new`]函数已保证数组大小至少为T的大小
        // 2. [`new`]函数已保证T的对齐需求为1，任意地址都满足这个约束
        // 3. 调用方已保证当前数据不会产生无效值
        // 我们这里可以使用[read]而非[read_unaligned]，因为T的对齐要求为1
        unsafe { read(self.bytes.as_ptr() as *const T) }
    }
}
