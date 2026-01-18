use core::ptr::NonNull;

use crate::ptr::dangling;

/// 双链表节点
pub struct Node<T> {
    pub val: T,
    pub(super) prev: NonNull<Node<T>>,
    pub(super) next: NonNull<Node<T>>,
}

impl<T> Node<T> {
    pub const fn new(val: T) -> Self {
        Self {
            val,
            prev: dangling(),
            next: dangling(),
        }
    }
}
