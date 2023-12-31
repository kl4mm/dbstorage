use std::{collections::BTreeSet, ops::Range};

use bytes::BytesMut;

use crate::{
    btree::slot::Either,
    get_ptr,
    page::{PageBuf, PageId, PAGE_SIZE},
    storable::Storable,
};

use super::slot::{Increment, Slot};

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum NodeType {
    Internal,
    Leaf,
}

impl From<u8> for NodeType {
    fn from(value: u8) -> Self {
        match value {
            1 => NodeType::Internal,
            2 => NodeType::Leaf,
            _ => unreachable!("unexpected NodeType: {value}"),
        }
    }
}

impl From<NodeType> for u8 {
    fn from(value: NodeType) -> Self {
        match value {
            NodeType::Internal => 1,
            NodeType::Leaf => 2,
        }
    }
}

const NODE_TYPE: usize = 0;
const NODE_IS_ROOT: usize = 1;
const NODE_LEN: Range<usize> = 2..6;
const NODE_MAX: Range<usize> = 6..10;
const NODE_NEXT: Range<usize> = 10..14;
const NODE_ID: Range<usize> = 14..18;
const NODE_VALUES_START: usize = 18;

// | NodeType (1) | Root (1) | Len(4) | Max (4) | Next (4) | PageId (4) | Values
#[derive(PartialEq, Clone, Debug)]
pub struct Node<K, V> {
    pub t: NodeType,
    pub is_root: bool,
    len: u32,
    max: u32,
    pub next: PageId,
    pub id: PageId,
    pub values: BTreeSet<Slot<K, V>>,
}

impl<K, V> From<&PageBuf> for Node<K, V>
where
    K: Storable + Ord,
    V: Storable + Eq,
{
    fn from(buf: &PageBuf) -> Self {
        let t = NodeType::from(buf[NODE_TYPE]);
        let is_root = buf[NODE_IS_ROOT] > 0;
        let len = u32::from_be_bytes(buf[NODE_LEN].try_into().unwrap());
        let max = u32::from_be_bytes(buf[NODE_MAX].try_into().unwrap());
        let next = PageId::from_be_bytes(buf[NODE_NEXT].try_into().unwrap());
        let id = PageId::from_be_bytes(buf[NODE_ID].try_into().unwrap());

        let mut values = BTreeSet::new();
        let size = Slot::<K, V>::SIZE;

        let left = &buf[NODE_VALUES_START..];
        let mut from = 0;
        let mut to = size;
        let mut rem = len;
        while rem > 0 {
            let bytes = &left[from..to];
            let slot = Slot::from(bytes);
            values.insert(slot);
            from += size;
            to += size;
            rem -= 1;
        }

        Self {
            t,
            is_root,
            len,
            max,
            next,
            id,
            values,
        }
    }
}

impl<K, V> From<&Node<K, V>> for PageBuf
where
    K: Copy + Storable,
    V: Copy + Storable,
{
    fn from(node: &Node<K, V>) -> Self {
        let mut ret: PageBuf = [0; PAGE_SIZE];

        ret[NODE_TYPE] = u8::from(node.t);
        ret[NODE_IS_ROOT] = node.is_root as u8;
        ret[NODE_LEN].copy_from_slice(&(node.values.len() as u32).to_be_bytes());
        ret[NODE_MAX].copy_from_slice(&node.max.to_be_bytes());
        ret[NODE_NEXT].copy_from_slice(&node.next.to_be_bytes());
        ret[NODE_ID].copy_from_slice(&node.id.to_be_bytes());

        let size = Slot::<K, V>::SIZE;
        let mut from = NODE_VALUES_START;
        let mut to = from + size;
        for value in &node.values {
            let slot = BytesMut::from(*value);
            ret[from..to].copy_from_slice(&slot);
            from += size;
            to += size;
        }

        if ret == [0; 4096] {
            panic!("PageBuf::from(Node) produced an empty buffer");
        }

        ret
    }
}

impl<K, V> From<Node<K, V>> for PageBuf
where
    K: Copy + Storable,
    V: Copy + Storable,
{
    fn from(node: Node<K, V>) -> Self {
        PageBuf::from(&node)
    }
}

impl<K, V> Node<K, V>
where
    K: Storable + Copy + Ord,
    V: Storable + Copy + Eq,
{
    pub fn new(id: PageId, max: u32, t: NodeType, is_root: bool) -> Self {
        Self {
            t,
            is_root,
            len: 0,
            max,
            next: -1,
            id,
            values: BTreeSet::new(),
        }
    }

    /// Split out half of self's values into a new node.
    pub fn split(&mut self, id: PageId) -> Node<K, V> {
        let mid = *self
            .values
            .iter()
            .nth(self.values.len() / 2)
            .expect("there should be a mid node");

        // All values in the greater half end up in `rest`
        let rest = self.values.split_off(&mid);
        self.len = self.values.len() as u32;
        self.is_root = false;

        let mut new = Node {
            t: self.t,
            is_root: false,
            len: rest.len() as u32,
            max: self.max,
            next: -1,
            id,
            values: rest,
        };

        if self.t == NodeType::Leaf {
            new.next = self.next;
            self.next = new.id;
        }

        new
    }

    /// Using last values for separators
    pub fn get_separators(self, other: Option<Node<K, V>>) -> Option<(Slot<K, V>, Slot<K, V>)>
    where
        K: Increment,
    {
        other.map(|other| (self.get_separator(), other.get_separator()))
    }

    /// Using last values for separators
    fn get_separator(self) -> Slot<K, V>
    where
        K: Increment,
    {
        let ls = self.values.last().expect("there should be a last slot");
        let k = if self.t == NodeType::Leaf { ls.0.next() } else { ls.0 };
        Slot(k, Either::Pointer(self.id))
    }

    /// Returns `None` if node is a leaf or if no keys were matched and the next key is invalid
    pub fn find_child(&self, key: K) -> Option<PageId> {
        if self.t == NodeType::Leaf {
            return None;
        }

        match self.values.iter().find(|s| key < s.0).map(|s| get_ptr!(s)) {
            None => match self.next {
                -1 => None,
                ptr => Some(ptr),
            },
            ptr => ptr,
        }
    }

    #[inline]
    pub fn first_ptr(&self) -> Option<PageId> {
        self.values.first().map(|s| match s.1 {
            Either::Value(_) => unreachable!(),
            Either::Pointer(ptr) => ptr,
        })
    }

    #[inline]
    pub fn last_key(&self) -> Option<K> {
        self.values.last().map(|s| s.0)
    }

    #[inline]
    pub fn almost_full(&self) -> bool {
        self.values.len() >= self.max as usize / 2
    }
}

#[cfg(test)]
mod test {
    use crate::btree::slot::Either;

    use super::*;

    #[test]
    fn test_from() {
        let node = Node {
            t: NodeType::Leaf,
            is_root: true,
            len: 10,
            max: 20,
            next: -1,
            id: 0,
            values: BTreeSet::from([
                Slot(10, Either::Value(20)),
                Slot(0, Either::Pointer(1)),
                Slot(20, Either::Value(30)),
                Slot(1, Either::Pointer(2)),
                Slot(30, Either::Value(40)),
                Slot(2, Either::Pointer(3)),
                Slot(40, Either::Value(50)),
                Slot(3, Either::Pointer(4)),
                Slot(50, Either::Value(60)),
                Slot(4, Either::Pointer(5)),
            ]),
        };

        let bytes = PageBuf::from(node.clone());

        let node2: Node<i32, i32> = Node::from(&bytes);

        assert!(node == node2, "Node: {:?}\n Node2: {:?}", node, node2);
    }

    #[test]
    fn test_split() {
        let mut node = Node {
            t: NodeType::Leaf,
            is_root: true,
            len: 11,
            max: 20,
            next: -1,
            id: 0,
            values: BTreeSet::from([
                Slot(10, Either::Value(1)),
                Slot(20, Either::Value(2)),
                Slot(30, Either::Value(3)),
                Slot(40, Either::Value(4)),
                Slot(50, Either::Value(5)),
                Slot(60, Either::Value(6)),
                Slot(70, Either::Value(7)),
                Slot(80, Either::Value(8)),
                Slot(90, Either::Value(9)),
                Slot(100, Either::Value(10)),
                Slot(110, Either::Value(11)),
            ]),
        };

        let new = node.split(1);

        let expected = Node {
            t: NodeType::Leaf,
            is_root: false,
            len: 5,
            max: 20,
            next: 1,
            id: 0,
            values: BTreeSet::from([
                Slot(10, Either::Value(1)),
                Slot(20, Either::Value(2)),
                Slot(30, Either::Value(3)),
                Slot(40, Either::Value(4)),
                Slot(50, Either::Value(5)),
            ]),
        };

        assert!(node == expected, "\nExpected: {:?}\n    Node: {:?}\n", expected, node);

        let expected_new = Node {
            t: NodeType::Leaf,
            is_root: false,
            len: 6,
            max: 20,
            next: -1,
            id: 1,
            values: BTreeSet::from([
                Slot(60, Either::Value(6)),
                Slot(70, Either::Value(7)),
                Slot(80, Either::Value(8)),
                Slot(90, Either::Value(9)),
                Slot(100, Either::Value(10)),
                Slot(110, Either::Value(11)),
            ]),
        };

        assert!(new == expected_new, "\nExpected: {:?}\n    Node: {:?}\n", expected_new, new);
    }

    #[test]
    fn test_get_separators_leaf() {
        let node = Node {
            t: NodeType::Leaf,
            is_root: false,
            len: 5,
            max: 20,
            next: 1,
            id: 0,
            values: BTreeSet::from([
                Slot(10, Either::Value(1)),
                Slot(20, Either::Value(2)),
                Slot(30, Either::Value(3)),
                Slot(40, Either::Value(4)),
                Slot(50, Either::Value(5)),
            ]),
        };

        let other = Node {
            t: NodeType::Leaf,
            is_root: false,
            len: 6,
            max: 20,
            next: -1,
            id: 1,
            values: BTreeSet::from([
                Slot(60, Either::Value(6)),
                Slot(70, Either::Value(7)),
                Slot(80, Either::Value(8)),
                Slot(90, Either::Value(9)),
                Slot(100, Either::Value(10)),
                Slot(110, Either::Value(11)),
            ]),
        };

        let Some(slots) = node.get_separators(Some(other)) else { panic!() };
        let expected = (Slot(51, Either::Pointer(0)), Slot(111, Either::Pointer(1)));
        assert!(slots == expected);
    }

    #[test]
    fn test_get_separators_internal() {
        let node: Node<u16, i32> = Node {
            t: NodeType::Internal,
            is_root: false,
            len: 5,
            max: 20,
            next: 1,
            id: 0,
            values: BTreeSet::from([
                Slot(10, Either::Pointer(1)),
                Slot(20, Either::Pointer(2)),
                Slot(30, Either::Pointer(3)),
                Slot(40, Either::Pointer(4)),
                Slot(50, Either::Pointer(5)),
            ]),
        };

        let other = Node {
            t: NodeType::Internal,
            is_root: false,
            len: 6,
            max: 20,
            next: -1,
            id: 1,
            values: BTreeSet::from([
                Slot(60, Either::Pointer(6)),
                Slot(70, Either::Pointer(7)),
                Slot(80, Either::Pointer(8)),
                Slot(90, Either::Pointer(9)),
                Slot(100, Either::Pointer(10)),
                Slot(110, Either::Pointer(11)),
            ]),
        };

        let Some(slots) = node.get_separators(Some(other)) else { panic!() };
        let expected = (Slot(50, Either::Pointer(0)), Slot(110, Either::Pointer(1)));
        assert!(slots == expected);
    }

    #[test]
    fn test_find_child() {
        let node: Node<i32, i32> = Node {
            t: NodeType::Internal,
            is_root: false,
            len: 5,
            max: 20,
            next: 1,
            id: 0,
            values: BTreeSet::from([
                Slot(10, Either::Pointer(1)),
                Slot(20, Either::Pointer(2)),
                Slot(30, Either::Pointer(3)),
                Slot(40, Either::Pointer(4)),
                Slot(50, Either::Pointer(5)),
            ]),
        };

        let a = node.find_child(25);
        let b = node.find_child(30);
        let c = node.find_child(60);

        assert!(a == Some(3));
        assert!(b == Some(4));
        assert!(c == Some(1));
    }
}
