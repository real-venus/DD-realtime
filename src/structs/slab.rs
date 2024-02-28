use arrayref::array_refs;
use bytemuck::{cast_ref, cast_slice, Pod, Zeroable};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use num_traits::ToPrimitive;
use sqlx::types::Decimal;
use std::{
    convert::TryFrom,
    mem::{align_of, size_of},
};

use crate::utils::token_factor;

use super::{market::MarketOrder, openbook::ObMarketInfo};

pub type NodeHandle = u32;

#[derive(IntoPrimitive, TryFromPrimitive, Debug)]
#[repr(u32)]
enum NodeTag {
    Uninitialized = 0,
    InnerNode = 1,
    LeafNode = 2,
    FreeNode = 3,
    LastFreeNode = 4,
}

#[derive(Copy, Clone, IntoPrimitive, TryFromPrimitive, Debug)]
#[repr(u8)]
pub enum FeeTier {
    Base,
    _SRM2,
    _SRM3,
    _SRM4,
    _SRM5,
    _SRM6,
    _MSRM,
    Stable,
}

#[derive(Copy, Clone)]
#[repr(packed)]
#[allow(dead_code)]
struct InnerNode {
    tag: u32,
    prefix_len: u32,
    key: u128,
    children: [u32; 2],
    _padding: [u64; 5],
}
unsafe impl Zeroable for InnerNode {}
unsafe impl Pod for InnerNode {}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(packed)]
pub struct LeafNode {
    pub tag: u32,
    pub owner_slot: u8,
    pub fee_tier: u8,
    pub padding: [u8; 2],
    pub key: u128,
    pub owner: [u64; 4],
    pub quantity: u64,
    pub client_order_id: u64,
}
unsafe impl Zeroable for LeafNode {}
unsafe impl Pod for LeafNode {}

impl LeafNode {
    #[inline]
    pub fn price(&self) -> u64 {
        (self.key >> 64) as u64
    }

    #[inline]
    pub fn quantity(&self) -> u64 {
        self.quantity
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

#[derive(Copy, Clone, Debug)]
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

impl AnyNode {
    fn case(&self) -> Option<NodeRef> {
        match NodeTag::try_from(self.tag) {
            Ok(NodeTag::InnerNode) => Some(NodeRef::Inner(cast_ref(self))),
            Ok(NodeTag::LeafNode) => Some(NodeRef::Leaf(cast_ref(self))),
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

#[derive(Copy, Clone, Debug)]
#[repr(packed)]
struct SlabHeader {
    _bump_index: u64,
    _free_list_len: u64,
    _free_list_head: u32,

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

/// Mainly copied from the original code, slightly modified to make working with it easier.
#[repr(transparent)]
pub struct Slab([u8]);

impl Slab {
    /// Creates a slab that holds and references the bytes
    #[inline]
    #[allow(unsafe_code)]
    pub fn new(raw_bytes: &mut [u8]) -> &mut Self {
        let data_end = raw_bytes.len() - 7;
        let bytes = &mut raw_bytes[13..data_end];
        let len_without_header = bytes.len().checked_sub(SLAB_HEADER_LEN).unwrap();
        let slop = len_without_header % size_of::<AnyNode>();
        let truncated_len = bytes.len() - slop;
        let bytes = &mut bytes[..truncated_len];
        let slab: &mut Self = unsafe { &mut *(bytes as *mut [u8] as *mut Slab) };
        slab.check_size_align(); // check alignment
        slab
    }

    pub fn get(&self, key: u32) -> Option<&AnyNode> {
        let node = self.nodes().get(key as usize)?;
        let tag = NodeTag::try_from(node.tag);
        match tag {
            Ok(NodeTag::InnerNode) | Ok(NodeTag::LeafNode) => Some(node),
            _ => None,
        }
    }

    fn check_size_align(&self) {
        let (header_bytes, nodes_bytes) = array_refs![&self.0, SLAB_HEADER_LEN; .. ;];
        let _header: &SlabHeader = cast_ref(header_bytes);
        let _nodes: &[AnyNode] = cast_slice(nodes_bytes);
    }

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

    fn root(&self) -> Option<NodeHandle> {
        if self.header().leaf_count == 0 {
            return None;
        }

        Some(self.header().root_node)
    }

    pub fn traverse(&self, descending: bool) -> Vec<&LeafNode> {
        fn walk_rec<'a>(
            slab: &'a Slab,
            sub_root: NodeHandle,
            buf: &mut Vec<&'a LeafNode>,
            descending: bool,
        ) {
            match slab.get(sub_root).unwrap().case().unwrap() {
                NodeRef::Leaf(leaf) => {
                    buf.push(leaf);
                }
                NodeRef::Inner(inner) => {
                    if descending {
                        walk_rec(slab, inner.children[1], buf, descending);
                        walk_rec(slab, inner.children[0], buf, descending);
                    } else {
                        walk_rec(slab, inner.children[0], buf, descending);
                        walk_rec(slab, inner.children[1], buf, descending);
                    }
                }
            }
        }

        let mut buf = Vec::with_capacity(self.header().leaf_count as usize);
        if let Some(r) = self.root() {
            walk_rec(self, r, &mut buf, descending);
        }
        assert_eq!(buf.len(), buf.capacity());
        buf
    }
}

pub fn readable_price(price_lots: u64, market: &ObMarketInfo) -> f64 {
    let base_multiplier = token_factor(market.base_decimals);
    let quote_multiplier = token_factor(market.quote_decimals);
    let base_lot_size = Decimal::from(market.base_lot_size);
    let quote_lot_size = Decimal::from(market.quote_lot_size);
    Decimal::to_f64(
        &((Decimal::from(price_lots) * quote_lot_size * base_multiplier)
            / (base_lot_size * quote_multiplier)),
    )
    .unwrap_or_default()
}

pub fn readable_quantity(quantity: u64, market: &ObMarketInfo) -> f64 {
    let base_lot_size = Decimal::from(market.base_lot_size);
    let base_multiplier = token_factor(market.base_decimals);
    Decimal::to_f64(&(Decimal::from(quantity) * base_lot_size / base_multiplier))
        .unwrap_or_default()
}

pub fn construct_levels(
    leaves: Vec<&LeafNode>,
    market: &ObMarketInfo,
    depth: usize,
) -> Vec<MarketOrder> {
    let mut levels: Vec<(u64, u64)> = vec![];
    for x in leaves {
        let len = levels.len();
        if len > 0 && levels[len - 1].0 == x.price() {
            levels[len - 1].1 += x.quantity();
        } else if len == depth {
            break;
        } else {
            levels.push((x.price(), x.quantity()));
        }
    }
    levels
        .into_iter()
        .map(|x| MarketOrder {
            price: f64::from(readable_price(x.0, market)),
            amount: readable_quantity(x.1, market),
            price_lots: x.0,
            size_lots: x.1,
        })
        .collect()
}
