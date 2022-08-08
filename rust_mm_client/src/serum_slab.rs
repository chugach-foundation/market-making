#![allow(dead_code, clippy::upper_case_acronyms)]
use {
    arrayref::array_refs,
    bytemuck::{cast_mut, cast_ref, cast_slice, Pod, Zeroable},
    num_enum::{IntoPrimitive, TryFromPrimitive},
    static_assertions::const_assert_eq,
    std::{
        convert::TryFrom,
        mem::{align_of, size_of},
        num::NonZeroU64,
    },
};

#[derive(Copy, Clone, IntoPrimitive, TryFromPrimitive, Debug)]
#[repr(u8)]
pub enum FeeTier {
    Base,
    SRM2,
    SRM3,
    SRM4,
    SRM5,
    SRM6,
    MSRM,
    Stable,
}

#[derive(Copy, Clone)]
#[repr(packed)]
pub struct OrderBookStateHeader {
    account_flags: u64, // Initialized, (Bids or Asks)
}

unsafe impl Zeroable for OrderBookStateHeader {}
unsafe impl Pod for OrderBookStateHeader {}

pub type NodeHandle = u32;

#[derive(IntoPrimitive, TryFromPrimitive)]
#[repr(u32)]
enum NodeTag {
    Uninitialized = 0,
    InnerNode = 1,
    LeafNode = 2,
    FreeNode = 3,
    LastFreeNode = 4,
}

#[derive(Debug, Copy, Clone)]
#[repr(packed)]
#[allow(dead_code)]
struct InnerNode {
    tag: u32,           // 4
    prefix_len: u32,    // 8
    key: u128,          // 24
    children: [u32; 2], // 32
    _padding: [u64; 5], // 72
}

unsafe impl Zeroable for InnerNode {}
unsafe impl Pod for InnerNode {}

impl InnerNode {
    fn walk_down(&self, search_key: u128) -> (NodeHandle, bool) {
        let crit_bit_mask = (1u128 << 127) >> self.prefix_len;
        let crit_bit = (search_key & crit_bit_mask) != 0;
        (self.children[crit_bit as usize], crit_bit)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(packed)]
pub struct LeafNode {
    tag: u32,             // 4
    owner_slot: u8,       // 5
    fee_tier: u8,         // 6
    padding: [u8; 2],     // 8
    key: u128,            // 24
    owner: [u64; 4],      // 56
    quantity: u64,        // 64
    client_order_id: u64, // 72
}

unsafe impl Zeroable for LeafNode {}
unsafe impl Pod for LeafNode {}

impl LeafNode {
    #[inline]
    pub fn new(
        owner_slot: u8,
        key: u128,
        owner: [u64; 4],
        quantity: u64,
        fee_tier: FeeTier,
        client_order_id: u64,
    ) -> Self {
        LeafNode {
            tag: NodeTag::LeafNode.into(),
            owner_slot,
            fee_tier: fee_tier.into(),
            padding: [0; 2],
            key,
            owner,
            quantity,
            client_order_id,
        }
    }

    #[inline]
    pub fn price(&self) -> NonZeroU64 {
        NonZeroU64::new((self.key >> 64) as u64).unwrap()
    }

    #[inline]
    pub fn order_id(&self) -> u128 {
        self.key
    }

    #[inline]
    pub fn quantity(&self) -> u64 {
        self.quantity
    }

    #[inline]
    pub fn set_quantity(&mut self, quantity: u64) {
        self.quantity = quantity;
    }

    #[inline]
    pub fn owner(&self) -> [u64; 4] {
        self.owner
    }

    #[inline]
    pub fn owner_slot(&self) -> u8 {
        self.owner_slot
    }

    #[inline]
    pub fn client_order_id(&self) -> u64 {
        self.client_order_id
    }
}

#[derive(Copy, Clone)]
#[repr(packed)]
#[allow(dead_code)]
struct FreeNode {
    tag: u32,
    next: u32,
    _padding: [u64; 8],
}
unsafe impl Zeroable for FreeNode {}
unsafe impl Pod for FreeNode {}

const fn _const_max(a: usize, b: usize) -> usize {
    let gt = (a > b) as usize;
    gt * a + (1 - gt) * b
}

const _INNER_NODE_SIZE: usize = size_of::<InnerNode>();
const _LEAF_NODE_SIZE: usize = size_of::<LeafNode>();
const _FREE_NODE_SIZE: usize = size_of::<FreeNode>();
const _NODE_SIZE: usize = 72;

const _INNER_NODE_ALIGN: usize = align_of::<InnerNode>();
const _LEAF_NODE_ALIGN: usize = align_of::<LeafNode>();
const _FREE_NODE_ALIGN: usize = align_of::<FreeNode>();
const _NODE_ALIGN: usize = 1;

const_assert_eq!(_NODE_SIZE, _INNER_NODE_SIZE);
const_assert_eq!(_NODE_SIZE, _LEAF_NODE_SIZE);
const_assert_eq!(_NODE_SIZE, _FREE_NODE_SIZE);

const_assert_eq!(_NODE_ALIGN, _INNER_NODE_ALIGN);
const_assert_eq!(_NODE_ALIGN, _LEAF_NODE_ALIGN);
const_assert_eq!(_NODE_ALIGN, _FREE_NODE_ALIGN);

#[derive(Copy, Clone)]
#[repr(packed)]
#[allow(dead_code)]
pub struct AnyNode {
    tag: u32,
    data: [u32; 17],
}
unsafe impl Zeroable for AnyNode {}
unsafe impl Pod for AnyNode {}

enum NodeRef<'a> {
    Inner(&'a InnerNode),
    Leaf(&'a LeafNode),
}

enum NodeRefMut<'a> {
    Inner(&'a mut InnerNode),
    Leaf(&'a mut LeafNode),
}

impl AnyNode {
    fn key(&self) -> Option<u128> {
        match self.case()? {
            NodeRef::Inner(inner) => Some(inner.key),
            NodeRef::Leaf(leaf) => Some(leaf.key),
        }
    }

    fn children(&self) -> Option<[u32; 2]> {
        match self.case().unwrap() {
            NodeRef::Inner(&InnerNode { children, .. }) => Some(children),
            NodeRef::Leaf(_) => None,
        }
    }

    fn case(&self) -> Option<NodeRef> {
        match NodeTag::try_from(self.tag) {
            Ok(NodeTag::InnerNode) => Some(NodeRef::Inner(cast_ref(self))),
            Ok(NodeTag::LeafNode) => Some(NodeRef::Leaf(cast_ref(self))),
            _ => None,
        }
    }

    fn case_mut(&mut self) -> Option<NodeRefMut> {
        match NodeTag::try_from(self.tag) {
            Ok(NodeTag::InnerNode) => Some(NodeRefMut::Inner(cast_mut(self))),
            Ok(NodeTag::LeafNode) => Some(NodeRefMut::Leaf(cast_mut(self))),
            _ => None,
        }
    }

    #[inline]
    pub fn as_leaf(&self) -> Option<&LeafNode> {
        match self.case() {
            Some(NodeRef::Leaf(leaf_ref)) => Some(leaf_ref),
            _ => None,
        }
    }

    #[inline]
    pub fn as_leaf_mut(&mut self) -> Option<&mut LeafNode> {
        match self.case_mut() {
            Some(NodeRefMut::Leaf(leaf_ref)) => Some(leaf_ref),
            _ => None,
        }
    }
}

impl AsRef<AnyNode> for InnerNode {
    fn as_ref(&self) -> &AnyNode {
        cast_ref(self)
    }
}

impl AsRef<AnyNode> for LeafNode {
    #[inline]
    fn as_ref(&self) -> &AnyNode {
        cast_ref(self)
    }
}

const_assert_eq!(_NODE_SIZE, size_of::<AnyNode>());
const_assert_eq!(_NODE_ALIGN, align_of::<AnyNode>());

#[derive(Debug, Copy, Clone)]
#[repr(packed)]
struct SlabHeader {
    bump_index: u64,
    free_list_len: u64,
    free_list_head: u32,

    root_node: u32,
    leaf_count: u64,
}
unsafe impl Zeroable for SlabHeader {}
unsafe impl Pod for SlabHeader {}

const SLAB_HEADER_LEN: usize = size_of::<SlabHeader>();

#[cfg(debug_assertions)]
unsafe fn invariant(check: bool) {
    if check {
        unreachable!();
    }
}

#[cfg(not(debug_assertions))]
#[inline(always)]
unsafe fn invariant(check: bool) {
    if check {
        std::hint::unreachable_unchecked();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OrderBookOrder {
    pub price: u64,
    pub quantity: u64,
    pub order_id: u128,
    pub client_order_id: u64,
}

#[repr(transparent)]
pub struct Slab([u8]);

impl Slab {
    /// Creates a slab that holds and references the bytes
    ///
    /// ```compile_fail
    /// let slab = {
    ///     let mut bytes = [10; 100];
    ///     serum_dex::critbit::Slab::new(&mut bytes)
    /// };
    /// ```
    #[inline]
    pub fn new(bytes: &mut [u8]) -> &mut Self {
        let len_without_header = bytes.len().checked_sub(SLAB_HEADER_LEN).unwrap();
        let slop = len_without_header % size_of::<AnyNode>();
        let truncated_len = bytes.len() - slop;
        let bytes = &mut bytes[..truncated_len];
        let slab: &mut Self = unsafe { &mut *(bytes as *mut [u8] as *mut Slab) };
        slab.check_size_align(); // check alignment
        slab
    }

    //Each one of these does a preorder traversal
    pub fn get_depth(
        &self,
        depth: u64,
        pc_lot_size: u64,
        coin_lot_size: u64,
        is_asks: bool,
    ) -> Vec<OrderBookOrder> {
        let (header, _nodes) = self.parts();
        let depth_to_get: usize = std::cmp::min(depth, header.leaf_count) as usize;
        let mut res: Vec<OrderBookOrder> = Vec::with_capacity(depth_to_get);
        let maybe_leafs = self.get_leaf_depth(depth_to_get, is_asks);

        let leafs = match maybe_leafs {
            Some(l) => l,
            _ => {
                return res;
            }
        };
        for leaf in leafs {
            let leaf_price = u64::from(leaf.price());
            let token_price =
                u128::from(leaf_price) * u128::from(pc_lot_size) / u128::from(coin_lot_size);
            let token_quantity = leaf.quantity() * coin_lot_size;
            let line = OrderBookOrder {
                price: u64::try_from(token_price).unwrap(),
                quantity: token_quantity,
                order_id: leaf.order_id(),
                client_order_id: leaf.client_order_id,
            };
            res.push(line);
        }

        res
    }

    #[allow(clippy::ptr_offset_with_cast)]
    fn check_size_align(&self) {
        let (header_bytes, nodes_bytes) = array_refs![&self.0, SLAB_HEADER_LEN; .. ;];
        let _header: &SlabHeader = cast_ref(header_bytes);
        let _nodes: &[AnyNode] = cast_slice(nodes_bytes);
    }

    #[allow(clippy::ptr_offset_with_cast)]
    fn parts(&self) -> (&SlabHeader, &[AnyNode]) {
        unsafe {
            invariant(self.0.len() < size_of::<SlabHeader>());
            invariant((self.0.as_ptr() as usize) % align_of::<SlabHeader>() != 0);
            invariant(
                ((self.0.as_ptr() as usize) + size_of::<SlabHeader>()) % align_of::<AnyNode>() != 0,
            );
        }

        let (header_bytes, nodes_bytes) = array_refs![&self.0, SLAB_HEADER_LEN; .. ;];
        let header = cast_ref(header_bytes);
        let nodes = cast_slice(nodes_bytes);
        (header, nodes)
    }

    fn header(&self) -> &SlabHeader {
        self.parts().0
    }

    fn nodes(&self) -> &[AnyNode] {
        self.parts().1
    }
}

pub trait SlabView<T> {
    fn get(&self, h: NodeHandle) -> Option<&T>;
}

impl SlabView<AnyNode> for Slab {
    fn get(&self, key: u32) -> Option<&AnyNode> {
        let node = self.nodes().get(key as usize)?;
        let tag = NodeTag::try_from(node.tag);
        match tag {
            Ok(NodeTag::InnerNode) | Ok(NodeTag::LeafNode) => Some(node),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum SlabTreeError {
    OutOfSpace,
}

impl Slab {
    fn root(&self) -> Option<NodeHandle> {
        if self.header().leaf_count == 0 {
            return None;
        }

        Some(self.header().root_node)
    }

    fn get_leaf_depth(&self, depth: usize, asc: bool) -> Option<Vec<&LeafNode>> {
        let root: NodeHandle = self.root()?;
        //println!("root {}", root);
        let mut stack: Vec<NodeHandle> = Vec::with_capacity(self.header().leaf_count as usize);
        let mut res: Vec<&LeafNode> = Vec::with_capacity(depth);
        stack.push(root);
        loop {
            if stack.is_empty() {
                break;
            }
            let node_contents = self.get(stack.pop().unwrap()).unwrap();
            match node_contents.case().unwrap() {
                NodeRef::Inner(&InnerNode { children, .. }) => {
                    if asc {
                        stack.push(children[1]);
                        stack.push(children[0]);
                    } else {
                        stack.push(children[0]);
                        stack.push(children[1]);
                    }
                    continue;
                }
                NodeRef::Leaf(leaf) => {
                    res.push(leaf);
                }
            }
            if res.len() == depth {
                break;
            }
        }
        Some(res)
    }
}
