use core::ptr::NonNull;

use crate::{
    collection::linked::node::Node,
    ptr::{dangling, is_dangling},
};

/// 侵入式链表
///
/// IntrusiveLinkedList是一个侵入式双向链表实现。链表不“拥有”节点，
/// 而是指向外部数据的一部分。
///
/// 对于此链表指向的数据，它们可能在栈上，可能在堆上，可能在静态存储区。
/// 但需要注意：链表节点必须在一个稳定、不可变的内存区域。
///
/// 当链表被释放后，不会对链表中的节点进行任何操作。
pub struct IntrusiveLinkedList<T> {
    head: NonNull<Node<T>>,
    tail: NonNull<Node<T>>,
    len: usize,
}

impl<T> IntrusiveLinkedList<T> {
    /// 构造一个空的侵入式链表
    pub const fn new() -> Self {
        Self {
            head: dangling(),
            tail: dangling(),
            len: 0,
        }
    }

    /// 获取链表大小
    pub fn len(&self) -> usize {
        self.len
    }

    /// 从链表头插入链表节点
    ///
    /// Safety:
    /// 节点必须有效，且未被加入到其他链表，节点地址在链表中必须稳定
    pub unsafe fn push_front_raw(&mut self, node: NonNull<Node<T>>) {
        unsafe {
            assert!(!is_dangling(node.as_ptr()));
            assert!(is_dangling((*node.as_ptr()).prev.as_ptr()));
            assert!(is_dangling((*node.as_ptr()).next.as_ptr()));
        }
        if is_dangling(self.head.as_ptr()) {
            unsafe {
                (*node.as_ptr()).prev = dangling();
                (*node.as_ptr()).next = dangling();
            }
            self.head = node;
            self.tail = node;
        } else {
            unsafe {
                (*node.as_ptr()).prev = dangling();
                (*node.as_ptr()).next = self.head;
                (*self.head.as_ptr()).prev = node;
            }
            self.head = node;
        }
        self.len += 1;
    }

    /// 从链表头插入链表节点
    ///
    /// Safety:
    /// 节点必须有效，且未被加入到其他链表，节点地址在链表中必须稳定
    pub unsafe fn push_back_raw(&mut self, node: NonNull<Node<T>>) {
        unsafe {
            assert!(!is_dangling(node.as_ptr()));
            assert!(is_dangling((*node.as_ptr()).prev.as_ptr()));
            assert!(is_dangling((*node.as_ptr()).next.as_ptr()));
        }
        if is_dangling(self.head.as_ptr()) {
            unsafe {
                (*node.as_ptr()).prev = dangling();
                (*node.as_ptr()).next = dangling();
            }
            self.head = node;
            self.tail = node;
        } else {
            unsafe {
                (*node.as_ptr()).prev = self.tail;
                (*node.as_ptr()).next = dangling();
                (*self.tail.as_ptr()).next = node;
            }
            self.tail = node;
        }
        self.len += 1;
    }

    /// 从链表头移出节点
    pub fn pop_front_raw(&mut self) -> Option<NonNull<Node<T>>> {
        if is_dangling(self.head.as_ptr()) {
            return None;
        }
        let node = self.head;
        unsafe {
            self.head = (*self.head.as_ptr()).next;
            if is_dangling(self.head.as_ptr()) {
                self.tail = dangling();
            } else {
                (*self.head.as_ptr()).prev = dangling();
            }
            (*node.as_ptr()).prev = dangling();
            (*node.as_ptr()).next = dangling();
        }
        self.len -= 1;
        Some(node)
    }

    /// 从链表尾移出节点
    pub fn pop_back_raw(&mut self) -> Option<NonNull<Node<T>>> {
        if is_dangling(self.head.as_ptr()) {
            return None;
        }
        let node = self.tail;
        unsafe {
            self.tail = (*self.tail.as_ptr()).prev;
            if is_dangling(self.tail.as_ptr()) {
                self.head = dangling();
            } else {
                (*self.tail.as_ptr()).next = dangling();
            }
            (*node.as_ptr()).prev = dangling();
            (*node.as_ptr()).next = dangling();
        }
        self.len -= 1;
        Some(node)
    }

    /// 从链表中移除节点
    ///
    /// 检查指定节点是否在链表中。如果存在，移除并返回true；如果不存在，返回false。
    /// “节点是否存在”是地址比较，不会解引用传入指针，不会调用T::eq
    ///
    /// 对链表中数据搜索需要O(n)时间复杂度。
    ///
    /// 如果传入 [dangling]，始终返回false
    ///
    /// Safety:
    /// 尽管node作为指针传入，但我们没有解引用或读取其值，因此此函数无需标注unsafe
    pub fn remove_raw(&mut self, node: NonNull<Node<T>>) -> bool {
        let mut current = self.head;
        while !is_dangling(current.as_ptr()) {
            if current != node {
                current = unsafe { (*current.as_ptr()).next };
                continue;
            }

            unsafe {
                if self.head == current {
                    self.head = (*current.as_ptr()).next;
                }
                if self.tail == current {
                    self.tail = (*current.as_ptr()).prev;
                }
                let prev = (*current.as_ptr()).prev;
                let next = (*current.as_ptr()).next;
                if !is_dangling(prev.as_ptr()) {
                    (*prev.as_ptr()).next = next;
                }
                if !is_dangling(next.as_ptr()) {
                    (*next.as_ptr()).prev = prev;
                }
                (*current.as_ptr()).prev = dangling();
                (*current.as_ptr()).next = dangling();
            }
            self.len -= 1;
            return true;
        }
        return false;
    }
}
