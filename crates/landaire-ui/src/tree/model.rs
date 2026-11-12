use std::hash::Hash;
use std::rc::Rc;

/// The nodes a [`Tree`](crate::Tree) shows, supplied by the application.
///
/// The tree reads only the levels it shows: a collapsed node's children are never asked for, so
/// a model may hold a large hierarchy without the tree walking it.
pub trait TreeModel: 'static {
    /// What a node is known by. A node keeps its key while the model changes around it, so the
    /// expanded set and the selection still name it.
    type Key: Clone + Eq + Hash + 'static;

    /// The top-level nodes, in order.
    fn roots(&self) -> &[Self::Key];

    /// The nodes directly below `key`, in order.
    fn children(&self, key: &Self::Key) -> &[Self::Key];

    /// The node directly above `key`, or `None` for a top-level node.
    fn parent(&self, key: &Self::Key) -> Option<Self::Key>;

    /// Whether `key` can be expanded.
    fn is_branch(&self, key: &Self::Key) -> bool {
        !self.children(key).is_empty()
    }

    /// The text typing a prefix finds `key` by, or `None` to leave it out of the search.
    fn text(&self, _key: &Self::Key) -> Option<&str> {
        None
    }
}

impl<M> TreeModel for Rc<M>
where
    M: TreeModel,
{
    type Key = M::Key;

    fn roots(&self) -> &[Self::Key] {
        (**self).roots()
    }

    fn children(&self, key: &Self::Key) -> &[Self::Key] {
        (**self).children(key)
    }

    fn parent(&self, key: &Self::Key) -> Option<Self::Key> {
        (**self).parent(key)
    }

    fn is_branch(&self, key: &Self::Key) -> bool {
        (**self).is_branch(key)
    }

    fn text(&self, key: &Self::Key) -> Option<&str> {
        (**self).text(key)
    }
}
