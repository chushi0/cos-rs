use core::ptr::NonNull;

use alloc::boxed::Box;

use crate::{boxed::TryBox, error::AllocError};

/// RBNode<T> 是红黑树的一个节点
///
/// # 节点类型
///
/// 它可能是
///
///     - 红黑树中的数据节点
///     - 红黑树中的NIL节点
///     - 红黑树外游离的节点
///
/// ## 数据节点
/// parent/left/right 分别表示树中父节点、左子树和右子树，
/// color表示颜色，value为当前节点持有的数据
///
/// ## NIL节点
/// parent/left/right 均指向自身
/// color始终为黑色，value为None
///
/// ## 游离节点
/// parent/left/right 均为 dangling
/// color未定义，value为当前节点持有的数据
///
/// # 树操作
/// 当游离节点进入树时，parent/left/right/color均需要重新赋值，且旧值直接忽略
#[derive(Debug)]
pub struct RBNode<T> {
    pub(super) parent: NonNull<RBNode<T>>,
    pub(super) left: NonNull<RBNode<T>>,
    pub(super) right: NonNull<RBNode<T>>,
    pub(super) color: NodeColor,
    pub(super) value: Option<T>, // None for NIL
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeColor {
    Black,
    Red,
}

impl<T> RBNode<T> {
    pub fn new_nil() -> Result<Box<Self>, AllocError> {
        let mut node = <Box<Self> as TryBox<Self>>::try_new(Self {
            parent: NonNull::dangling(),
            left: NonNull::dangling(),
            right: NonNull::dangling(),
            color: NodeColor::Black,
            value: None,
        })?;

        node.parent = NonNull::from_ref(node.as_ref());
        node.left = NonNull::from_ref(node.as_ref());
        node.right = NonNull::from_ref(node.as_ref());

        Ok(node)
    }

    pub fn new_detach(value: T) -> Result<Box<Self>, AllocError> {
        <Box<Self> as TryBox<Self>>::try_new(Self {
            parent: NonNull::dangling(),
            left: NonNull::dangling(),
            right: NonNull::dangling(),
            color: NodeColor::Black,
            value: Some(value),
        })
    }

    pub(super) fn is_nil(&self) -> bool {
        self.value.is_none()
    }
}

macro_rules! node_path {
    ($node:ident . $f:ident $($t:tt)*) => {
        node_path!(@ ( (*$node.as_ptr()).$f ) $($t)* )
    };
    (ref $($t:tt)*) => {
        *node_path!($($t)*).as_ptr()
    };
    (@ ($node:expr) . $f:ident $($t:tt)*) => {
        node_path!(@ ( (*$node.as_ptr()).$f ) $($t)* )
    };
    (@ ($node:expr)) => {
        $node
    };
}

pub(super) use node_path;