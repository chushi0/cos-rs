use core::{
    borrow::Borrow,
    cmp::Ordering,
    fmt::Debug,
    ptr::{self, NonNull},
};

use alloc::boxed::Box;

use crate::{
    boxed::dealloc_box,
    collection::rbtree::node::{NodeColor, RBNode, node_path},
    error::AllocError,
    ptr::{dangling, is_dangling},
};

/// 基于红黑树的Map
///
/// 红黑树是一种2-3-4树在二叉树上的投影，红黑树的节点分为红色和黑色，一个黑色节点及其最多两个红色子节点
/// 共同构成了2-3-4树上的一个节点。在2-3-4树上，每个子树的高度相同。由此推导出红黑树的以下性质：
///
/// 1. 每个节点的左子节点小于当前节点小于右子节点
/// 2. 节点分为红色和黑色
/// 3. 根节点和叶子节点（NIL节点）是黑色
/// 4. 不存在两个相邻的红色节点
/// 5. 从任意节点到叶子节点每个路径上的黑色节点数相同
///
/// 红黑树的插入、查询、删除的复杂度均为`O(log n)`，且除数据节点所需要的内存外，不需要额外内存。
pub struct RBTreeMap<K, V> {
    root: NonNull<RBNode<Entry<K, V>>>,
    nil: NonNull<RBNode<Entry<K, V>>>,
}

#[derive(Debug)]
pub struct Entry<K, V> {
    pub key: K,
    pub value: V,
}

impl<K: Ord, V> RBTreeMap<K, V> {
    /// 新建一个空的红黑树。在首次使用前，必须进行初始化
    pub const fn const_new() -> Self {
        Self {
            root: dangling(),
            nil: dangling(),
        }
    }

    /// 新建一个初始化后的红黑树
    pub fn new() -> Result<Self, AllocError> {
        let nil = RBNode::new_nil()?;
        let nil = NonNull::from_ref(Box::leak(nil));
        Ok(Self { root: nil, nil })
    }

    pub fn init(&mut self) -> Result<(), AllocError> {
        if is_dangling(self.nil.as_ptr()) {
            let nil = RBNode::new_nil()?;
            let nil = NonNull::from_ref(Box::leak(nil));
            self.root = nil;
            self.nil = nil;
        }
        Ok(())
    }

    /// 查询数据，返回对应value的借用。如果不存在，返回None
    pub fn get<Q>(&self, key: Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        self.find(&key)
            .map(|entry| unsafe { &(*entry.as_ptr()).value.as_ref().unwrap().value })
    }

    /// 查询数据，返回对应value的借用。如果不存在，返回None
    pub fn get_mut<Q>(&mut self, key: Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        self.find(&key)
            .map(|entry| unsafe { &mut (*entry.as_ptr()).value.as_mut().unwrap().value })
    }

    pub unsafe fn insert_raw(
        &mut self,
        node: NonNull<RBNode<Entry<K, V>>>,
    ) -> Option<NonNull<RBNode<Entry<K, V>>>> {
        assert!(!is_dangling(self.root.as_ptr()));
        assert!(!is_dangling(self.nil.as_ptr()));

        unsafe { self.insert(node) }
    }

    pub fn try_insert(&mut self, key: K, value: V) -> Result<Option<V>, AllocError> {
        let node = RBNode::new_detach(Entry { key, value })?;
        let node = NonNull::from_mut(Box::leak(node));

        let prev = unsafe { self.insert_raw(node) };
        Ok(prev.map(|entry| unsafe { Self::unbox_value(entry) }))
    }

    pub fn remove_raw<Q>(&mut self, key: Q) -> Option<NonNull<RBNode<Entry<K, V>>>>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        self.find(&key).map(|entry| unsafe {
            self.delete(entry);
            entry
        })
    }

    pub fn remove<Q>(&mut self, key: Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        self.find(&key).map(|entry| unsafe {
            self.delete(entry);
            Self::unbox_value(entry)
        })
    }

    /// 以对应节点为中心进行左旋
    ///
    /// ```txt
    ///     X                 Y
    ///    / \               / \
    ///   A   Y     ===>    X   C
    ///      / \           / \
    ///     B   C         A   B
    /// ```
    ///
    /// Safety:
    /// 输入的x必须为当前树中的有效节点，且其右子节点不为NIL
    unsafe fn rotate_left(&mut self, x: NonNull<RBNode<Entry<K, V>>>) {
        unsafe {
            assert_ne!(x, self.nil);
            let y = node_path!(x.right);
            assert_ne!(y, self.nil);

            node_path!(x.right) = node_path!(y.left);
            if !node_path!(ref y.left).is_nil() {
                node_path!(y.left.parent) = x;
            }

            node_path!(y.parent) = node_path!(x.parent);
            if node_path!(ref x.parent).is_nil() {
                self.root = y;
            } else if x == node_path!(x.parent.left) {
                node_path!(x.parent.left) = y;
            } else {
                node_path!(x.parent.right) = y;
            }

            node_path!(y.left) = x;
            node_path!(x.parent) = y;
        }
    }

    /// 以对应节点为中心进行右旋
    ///
    /// ```txt
    ///      X                 Y
    ///     / \               / \
    ///    Y   C    ===>     A   X
    ///   / \                   / \
    ///  A   B                 B   C
    /// ```
    ///
    /// Safety:
    /// 输入的x必须为当前树中的有效节点，且其左子节点不为NIL
    fn rotate_right(&mut self, x: NonNull<RBNode<Entry<K, V>>>) {
        unsafe {
            assert_ne!(x, self.nil);
            let y = node_path!(x.left);
            assert_ne!(y, self.nil);

            node_path!(x.left) = node_path!(y.right);
            if !node_path!(ref y.right).is_nil() {
                node_path!(y.right.parent) = x;
            }

            node_path!(y.parent) = node_path!(x.parent);
            if node_path!(ref x.parent).is_nil() {
                self.root = y;
            } else if x == node_path!(x.parent.right) {
                node_path!(x.parent.right) = y;
            } else {
                node_path!(x.parent.left) = y;
            }

            node_path!(y.right) = x;
            node_path!(x.parent) = y;
        }
    }

    /// 查找指定节点，如果存在，返回此节点
    ///
    /// 搜索按照搜索树通用规则进行搜索，复杂度为`O(log n)`
    fn find<Q>(&self, key: &Q) -> Option<NonNull<RBNode<Entry<K, V>>>>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        if is_dangling(self.root.as_ptr()) || is_dangling(self.nil.as_ptr()) {
            return None;
        }

        unsafe {
            let mut x = self.root;
            while !(*x.as_ptr()).is_nil() {
                let node_key = (*x.as_ptr()).value.as_ref().unwrap().key.borrow();
                match key.cmp(node_key) {
                    Ordering::Equal => return Some(x),
                    Ordering::Less => {
                        x = node_path!(x.left);
                    }
                    Ordering::Greater => {
                        x = node_path!(x.right);
                    }
                }
            }
        }

        None
    }

    /// 用节点 v 整体替换节点 u，包含子树、颜色等。
    /// 替换后，节点u将脱离此树
    ///
    /// Safety:
    /// u必须在当前树中，v必须为游离节点，uv均不能为NIL
    unsafe fn replace(&mut self, u: NonNull<RBNode<Entry<K, V>>>, v: NonNull<RBNode<Entry<K, V>>>) {
        assert_ne!(u, self.nil);
        assert_ne!(v, self.nil);

        unsafe {
            // 颜色
            node_path!(v.color) = node_path!(u.color);

            // 左子节点
            node_path!(v.left) = node_path!(u.left);
            if !node_path!(ref v.left).is_nil() {
                node_path!(v.left.parent) = v;
            }

            // 右子节点
            node_path!(v.right) = node_path!(u.right);
            if !node_path!(ref v.right).is_nil() {
                node_path!(v.right.parent) = v;
            }

            // 父节点
            node_path!(v.parent) = node_path!(u.parent);
            if node_path!(ref v.parent).is_nil() {
                self.root = v;
            } else if node_path!(v.parent.left) == u {
                node_path!(v.parent.left) = v;
            } else {
                node_path!(v.parent.right) = v;
            }

            // 清空u
            node_path!(u.parent) = dangling();
            node_path!(u.left) = dangling();
            node_path!(u.right) = dangling();
        }
    }

    /// 用节点v代替节点u的位置，并使节点u成为单独一个树的根节点
    ///
    /// Safety:
    /// uv均为有效数据节点，且其NIL节点均为self.nil
    unsafe fn transplant(
        &mut self,
        u: NonNull<RBNode<Entry<K, V>>>,
        v: NonNull<RBNode<Entry<K, V>>>,
    ) {
        unsafe {
            assert_ne!(u, self.nil);
            assert_ne!(u, v);
            if node_path!(ref u.parent).is_nil() {
                self.root = v;
            } else if u == node_path!(u.parent.left) {
                node_path!(u.parent.left) = v;
            } else {
                node_path!(u.parent.right) = v;
            }

            if !(*v.as_ptr()).is_nil() {
                node_path!(v.parent) = node_path!(u.parent);
            }
            node_path!(u.parent) = self.nil;
        }
    }

    /// 将节点插入树中，如果Key相同，则进行替换。
    /// 如果为替换，返回被替换的节点。
    ///
    /// 注意：替换为对应内存的替换，而非值替换。
    ///
    /// Safety:
    /// z必须为有效的游离节点
    unsafe fn insert(
        &mut self,
        z: NonNull<RBNode<Entry<K, V>>>,
    ) -> Option<NonNull<RBNode<Entry<K, V>>>> {
        unsafe {
            let mut y = self.nil;
            let mut x = self.root;

            // 搜索插入位置
            let z_key = &(*z.as_ptr()).value.as_ref().unwrap().key;
            while !(*x.as_ptr()).is_nil() {
                y = x;
                let x_key = &(*x.as_ptr()).value.as_ref().unwrap().key;
                match z_key.cmp(x_key) {
                    Ordering::Less => x = node_path!(x.left),
                    Ordering::Greater => x = node_path!(x.right),
                    Ordering::Equal => {
                        self.replace(x, z);
                        return Some(x);
                    }
                }
            }

            // 作为红节点插入
            node_path!(z.parent) = y;
            if (*y.as_ptr()).is_nil() {
                self.root = z;
            } else {
                let parent_key = &node_path!(y.value).as_ref().unwrap().key;
                if z_key < parent_key {
                    node_path!(y.left) = z;
                } else {
                    node_path!(y.right) = z;
                }
            }

            node_path!(z.left) = self.nil;
            node_path!(z.right) = self.nil;
            node_path!(z.color) = NodeColor::Red;

            // 插入修正
            self.insert_fixup(z);

            None
        }
    }

    /// 插入修正，z为刚插入的节点
    ///
    /// 当节点插入后，需要对红黑树重新旋转平衡，以维持红黑树不变量。
    /// 节点插入后的情况总体可分为以下三类：
    ///
    /// 1. 当插入节点的父节点为黑节点时，无需处理
    /// 2. 当插入节点的父节点为红节点时
    ///     a. 当祖父节点仅有一个红子节点，则通过左旋或右旋调整平衡，然后重新染色
    ///     b. 当祖父节点有两个红子节点时，将父节点和叔节点染黑，祖父节点染红，然后以祖父节点为参数重新执行插入修正
    /// 3. 执行完成后将根节点重新染黑，因为上述迭代过程可能会将根节点染红
    ///
    /// 以下面的树为例：
    /// ```txt
    ///                         B(15)
    ///                   /               \
    ///               R(8)                 R(25)
    ///              /   \                 /   \
    ///          B(4)    B(10)          B(20)  B(36)
    ///          / \          \        /
    ///       R(2) R(6)      R(12)  R(18)
    /// ```
    /// 当插入`9、22、35、37`时，无需进行修正。插入后树结构如下：
    /// ```txt
    ///                         B(15)
    ///                   /               \
    ///               R(8)                 R(25)
    ///              /   \                 /   \
    ///          B(4)    B(10)          B(20)  B(36)
    ///          / \     /    \        /   \     /  \
    ///       R(2) R(6) R(9) R(12)  R(18) R(22) R(35) R(37)
    /// ```
    /// 当插入`11、13、16、19`时，满足2.a，进行左旋或右旋调整平衡。以11为例
    /// ```txt
    ///                         B(15)
    ///                   /               \
    ///               R(8)                 R(25)
    ///              /   \                 /   \
    ///          B(4)    B(10)          B(20)  B(36)
    ///          / \          \        /
    ///       R(2) R(6)      R(12)  R(18)
    ///                       /
    ///                     R(11)
    ///
    /// 以B(12)为中心右旋
    ///
    ///                         B(15)
    ///                   /               \
    ///               R(8)                 R(25)
    ///              /   \                 /   \
    ///          B(4)    B(10)          B(20)  B(36)
    ///          / \          \        /
    ///       R(2) R(6)      R(11)  R(18)
    ///                          \
    ///                         R(12)
    ///
    /// 以B(10)为中心左旋
    ///
    ///                         B(15)
    ///                   /               \
    ///               R(8)                 R(25)
    ///              /   \                 /   \
    ///          B(4)    R(11)          B(20)  B(36)
    ///          / \     /    \        /
    ///       R(2) R(6) B(10) R(12)  R(18)
    ///
    /// 重新染色
    ///
    ///                         B(15)
    ///                   /               \
    ///               R(8)                 R(25)
    ///              /   \                 /   \
    ///          B(4)    B(11)          B(20)  B(36)
    ///          / \     /    \        /
    ///       R(2) R(6) R(10) R(12)  R(18)
    /// ```
    /// 当插入`1、3、5、7`时，满足2.b，重新染色后循环执行。以插入1为例
    /// ```txt
    ///                         B(15)
    ///                   /               \
    ///               R(8)                 R(25)
    ///              /   \                 /   \
    ///          B(4)    B(10)          B(20)  B(36)
    ///          / \          \        /
    ///       R(2) R(6)      R(12)  R(18)
    ///       /
    ///     R(1)
    ///
    /// 重新染色
    ///
    ///                         B(15)
    ///                   /               \
    ///               R(8)                 R(25)
    ///              /   \                 /   \
    ///          R(4)    B(10)          B(20)  B(36)
    ///          / \          \        /
    ///       B(2) B(6)      R(12)  R(18)
    ///       /
    ///     R(1)
    ///
    /// 以R(4)为参数循环，再次满足2.b，重新染色
    ///
    ///                         R(15)
    ///                   /               \
    ///               B(8)                 B(25)
    ///              /   \                 /   \
    ///          R(4)    B(10)          B(20)  B(36)
    ///          / \          \        /
    ///       B(2) B(6)      R(12)  R(18)
    ///       /
    ///     R(1)
    ///
    /// 将根节点重新染黑
    ///
    ///                         B(15)
    ///                   /               \
    ///               B(8)                 B(25)
    ///              /   \                 /   \
    ///          R(4)    B(10)          B(20)  B(36)
    ///          / \          \        /
    ///       B(2) B(6)      R(12)  R(18)
    ///       /
    ///     R(1)
    /// ```
    ///
    /// Safety:
    /// z必须为当前树中的有效节点
    unsafe fn insert_fixup(&mut self, mut z: NonNull<RBNode<Entry<K, V>>>) {
        assert_ne!(z, self.nil);

        unsafe {
            // 仅在父节点颜色为红色时处理
            while node_path!(z.parent.color) == NodeColor::Red {
                // 父节点是祖父节点的左节点
                if node_path!(z.parent) == node_path!(z.parent.parent.left) {
                    let y = node_path!(z.parent.parent.right);
                    // 父节点为红色，叔节点也为红色，满足2.b
                    if node_path!(y.color) == NodeColor::Red {
                        node_path!(z.parent.color) = NodeColor::Black;
                        node_path!(y.color) = NodeColor::Black;
                        node_path!(z.parent.parent.color) = NodeColor::Red;
                        z = node_path!(z.parent.parent);
                    } else {
                        // 当前节点是父节点的右节点情况，先以父节点为中心左旋，转为当前节点为父节点的左节点
                        //（参考函数文档注释中插入16的情况）
                        if z == node_path!(z.parent.right) {
                            z = node_path!(z.parent);
                            self.rotate_left(z);
                        }

                        // 当前节点为父节点的左节点，以祖父节点进行右旋，并进行染色
                        node_path!(z.parent.color) = NodeColor::Black;
                        node_path!(z.parent.parent.color) = NodeColor::Red;
                        self.rotate_right(node_path!(z.parent.parent));
                    }
                } else {
                    // 对称操作
                    let y = node_path!(z.parent.parent.left);
                    // 2.b
                    if node_path!(y.color) == NodeColor::Red {
                        node_path!(z.parent.color) = NodeColor::Black;
                        node_path!(y.color) = NodeColor::Black;
                        node_path!(z.parent.parent.color) = NodeColor::Red;
                        z = node_path!(z.parent.parent);
                    } else {
                        // 父节点是祖父节点的右节点，当前节点是父节点的左节点
                        // 参考函数文档注释中插入11的情况
                        if z == node_path!(z.parent.left) {
                            z = node_path!(z.parent);
                            self.rotate_right(z);
                        }

                        // 当前节点为父节点的右节点，以祖父节点进行左旋，并进行染色
                        node_path!(z.parent.color) = NodeColor::Black;
                        node_path!(z.parent.parent.color) = NodeColor::Red;
                        self.rotate_left(node_path!(z.parent.parent));
                    }
                }
            }

            // 循环结束后将根节点重新染黑
            (*self.root.as_ptr()).color = NodeColor::Black;
        }
    }

    /// 将游离节点完全释放，并返回value
    ///
    /// Safety:
    /// 1. 节点必须有效，且必须已经完全脱离树结构
    /// 2. 内存必须来自全局分配器，且与Box兼容
    unsafe fn unbox_value(node: NonNull<RBNode<Entry<K, V>>>) -> V {
        unsafe {
            let value = (*node.as_ptr()).value.take().unwrap();
            ptr::drop_in_place(node.as_ptr());
            dealloc_box(node.as_ptr());
            value.value
        }
    }

    /// 获取指定子树的最小值
    unsafe fn tree_minimum(
        &self,
        mut node: NonNull<RBNode<Entry<K, V>>>,
    ) -> NonNull<RBNode<Entry<K, V>>> {
        unsafe {
            while !node_path!(ref node.left).is_nil() {
                node = node_path!(node.left);
            }
            node
        }
    }

    /// 将指定节点从树中移除
    ///
    /// 删除节点分为三步：
    /// 1. 如果待删除节点有两个子节点，将待删除节点与其前驱或后继节点互换
    /// 2. 如果删除的节点为红色，直接删除节点，无需进行平衡
    /// 3. 如果删除的节点为黑色，进行删除平衡操作
    ///
    /// Safety:
    /// z必须为树中有效的数据节点
    unsafe fn delete(&mut self, z: NonNull<RBNode<Entry<K, V>>>) {
        assert_ne!(z, self.nil);

        unsafe {
            let mut y = z;
            let mut y_original_color = node_path!(y.color);
            let x;

            if node_path!(ref z.left).is_nil() {
                x = node_path!(z.right);
                self.transplant(z, node_path!(z.right));
            } else if node_path!(ref z.right).is_nil() {
                x = node_path!(z.left);
                self.transplant(z, node_path!(z.left));
            } else {
                y = self.tree_minimum(node_path!(z.right));
                y_original_color = node_path!(y.color);
                x = node_path!(y.right);

                if node_path!(y.parent) == z {
                    node_path!(x.parent) = y;
                } else {
                    self.transplant(y, node_path!(y.right));
                    node_path!(y.right) = node_path!(z.right);
                    node_path!(y.right.parent) = y;
                }

                self.transplant(z, y);
                node_path!(y.left) = node_path!(z.left);
                node_path!(y.left.parent) = y;
                node_path!(y.color) = node_path!(z.color);
            }

            if y_original_color == NodeColor::Black {
                match node_path!(x.color) {
                    NodeColor::Black => self.delete_fixup(x),
                    NodeColor::Red => node_path!(x.color) = NodeColor::Black,
                }
            }
        }
    }

    /// 当删除黑色节点时，进行删除平衡调整
    ///
    /// x 为被删除节点
    ///
    /// 分为对称的四种情况
    /// 1. 如果父节点为黑色，兄弟节点为红色，通过将兄弟节点旋转到父节点位置，交换父节点和兄弟节点颜色，转换为其余三种情况之一
    /// 2. 如果兄弟节点和兄弟节点的两个子节点均为黑色，将兄弟节点染红，然后向父节点递归计算
    /// 3. 如果兄弟节点为黑色，兄弟节点的靠近待删除节点的子节点为红色，交换红色节点与兄弟节点的颜色，通过旋转将红色节点变为兄弟节点，转为情况四
    /// 4. 如果兄弟节点为黑色，兄弟节点的远离待删除节点的子节点为红色，交换父节点与兄弟节点的颜色，旋转使兄弟节点成为父节点，然后停止递归
    ///
    unsafe fn delete_fixup(&mut self, mut x: NonNull<RBNode<Entry<K, V>>>) {
        unsafe {
            while x != self.root {
                assert!(node_path!(x.color) == NodeColor::Black);

                if node_path!(x.parent.left) == x {
                    // 兄弟节点
                    let mut y = node_path!(x.parent.right);

                    // 1.
                    if node_path!(y.color) == NodeColor::Red {
                        node_path!(y.color) = node_path!(x.parent.color);
                        node_path!(x.parent.color) = NodeColor::Red;
                        self.rotate_left(node_path!(x.parent));
                        y = node_path!(x.parent.right);
                    }

                    // 2.
                    if node_path!(y.left.color) == NodeColor::Black
                        && node_path!(y.right.color) == NodeColor::Black
                    {
                        node_path!(y.color) = NodeColor::Red;
                        x = node_path!(x.parent);
                        if node_path!(x.color) == NodeColor::Red {
                            node_path!(x.color) = NodeColor::Black;
                            break;
                        }
                        continue;
                    }

                    // 3.
                    if node_path!(y.left.color) == NodeColor::Red
                        && node_path!(y.right.color) == NodeColor::Black
                    {
                        node_path!(y.color) = NodeColor::Red;
                        node_path!(y.left.color) = NodeColor::Black;
                        self.rotate_right(y);
                        y = node_path!(x.parent.right);
                    }

                    // 4.
                    if node_path!(y.right.color) == NodeColor::Red {
                        node_path!(y.color) = node_path!(x.parent.color);
                        node_path!(x.parent.color) = NodeColor::Black;
                        node_path!(y.right.color) = NodeColor::Black;
                        self.rotate_left(node_path!(x.parent));
                        break;
                    }
                } else {
                    // 兄弟节点
                    let mut y = node_path!(x.parent.left);

                    // 1.
                    if node_path!(y.color) == NodeColor::Red {
                        node_path!(y.color) = node_path!(x.parent.color);
                        node_path!(x.parent.color) = NodeColor::Red;
                        self.rotate_right(node_path!(x.parent));
                        y = node_path!(x.parent.left);
                    }

                    // 2.
                    if node_path!(y.left.color) == NodeColor::Black
                        && node_path!(y.right.color) == NodeColor::Black
                    {
                        node_path!(y.color) = NodeColor::Red;
                        x = node_path!(x.parent);
                        if node_path!(x.color) == NodeColor::Red {
                            node_path!(x.color) = NodeColor::Black;
                            break;
                        }
                        continue;
                    }

                    // 3.
                    if node_path!(y.right.color) == NodeColor::Red
                        && node_path!(y.left.color) == NodeColor::Black
                    {
                        node_path!(y.color) = NodeColor::Red;
                        node_path!(y.right.color) = NodeColor::Black;
                        self.rotate_left(y);
                        y = node_path!(x.parent.left);
                    }

                    // 4.
                    if node_path!(y.left.color) == NodeColor::Red {
                        node_path!(y.color) = node_path!(x.parent.color);
                        node_path!(x.parent.color) = NodeColor::Black;
                        node_path!(y.left.color) = NodeColor::Black;
                        self.rotate_right(node_path!(x.parent));
                        break;
                    }
                }
            }

            (*self.root.as_ptr()).color = NodeColor::Black;
        }
    }
}

impl<K, V> Debug for RBTreeMap<K, V>
where
    K: Debug,
    V: Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        struct NodeWrap<K, V>(NonNull<RBNode<Entry<K, V>>>);
        impl<K, V> Debug for NodeWrap<K, V>
        where
            K: Debug,
            V: Debug,
        {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                let val = unsafe { self.0.as_ref() };
                if val.value.is_some() {
                    f.debug_struct("RBNode")
                        .field("addr", &self.0.as_ptr())
                        .field("parent", &val.parent.as_ptr())
                        .field("color", &val.color)
                        .field("entry", &val.value)
                        .field("left", &NodeWrap(val.left))
                        .field("right", &NodeWrap(val.right))
                        .finish()
                } else {
                    f.debug_struct("RBNode")
                        .field("addr", &self.0.as_ptr())
                        .field("parent", &val.parent.as_ptr())
                        .field("entry", &"NIL")
                        .finish()
                }
            }
        }

        f.debug_struct("RBTreeMap")
            .field("root", &NodeWrap(self.root))
            .field("nil", &NodeWrap(self.nil))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let mut tree = RBTreeMap::new().unwrap();

        for i in 0..100 {
            assert!(tree.try_insert(i, i * 10).unwrap().is_none());
        }

        for i in 0..100 {
            assert_eq!(tree.get(i), Some(&(i * 10)));
        }
    }

    #[test]
    fn test_remove() {
        let mut tree = RBTreeMap::new().unwrap();

        for i in 0..50 {
            tree.try_insert(i, i).unwrap();
        }

        for i in (0..50).step_by(2) {
            assert_eq!(tree.remove(i), Some(i));
        }

        for i in 0..50 {
            if i % 2 == 0 {
                assert_eq!(tree.get(i), None);
            } else {
                assert_eq!(tree.get(i), Some(&i));
            }
        }
    }
}
