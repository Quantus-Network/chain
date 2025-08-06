// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! `NodeCodec` implementation for Substrate's trie format.

use super::node_header::{NodeHeader, NodeKind};
use crate::{error::Error, trie_constants};
use alloc::{borrow::Borrow, vec::Vec};
use codec::{Decode, Encode, Input};
use core::{marker::PhantomData, ops::Range};
use hash_db::Hasher;
use trie_db::{
    nibble_ops,
    node::{NibbleSlicePlan, NodeHandlePlan, NodePlan, Value, ValuePlan},
    ChildReference, NodeCodec as NodeCodecT,
};

/// Helper struct for trie node decoder. This implements `codec::Input` on a byte slice, while
/// tracking the absolute position. This is similar to `std::io::Cursor` but does not implement
/// `Read` and `io` are not in `core` or `alloc`.
struct ByteSliceInput<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> ByteSliceInput<'a> {
    fn new(data: &'a [u8]) -> Self {
        ByteSliceInput { data, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<Range<usize>, codec::Error> {
        if self.offset + count > self.data.len() {
            return Err("out of data".into());
        }

        let range = self.offset..(self.offset + count);
        self.offset += count;
        Ok(range)
    }
}

impl<'a> Input for ByteSliceInput<'a> {
    fn remaining_len(&mut self) -> Result<Option<usize>, codec::Error> {
        Ok(Some(self.data.len().saturating_sub(self.offset)))
    }

    fn read(&mut self, into: &mut [u8]) -> Result<(), codec::Error> {
        let range = self.take(into.len())?;
        into.copy_from_slice(&self.data[range]);
        Ok(())
    }

    fn read_byte(&mut self) -> Result<u8, codec::Error> {
        if self.offset + 1 > self.data.len() {
            return Err("out of data".into());
        }

        let byte = self.data[self.offset];
        self.offset += 1;
        Ok(byte)
    }
}

/// Concrete implementation of a [`NodeCodecT`] with SCALE encoding.
///
/// It is generic over `H` the [`Hasher`].
#[derive(Default, Clone)]
pub struct NodeCodec<H>(PhantomData<H>);

impl<H> NodeCodecT for NodeCodec<H>
where
    H: Hasher,
{
    const ESCAPE_HEADER: Option<u8> = Some(trie_constants::ESCAPE_COMPACT_HEADER);
    type Error = Error<H::Out>;
    type HashOut = H::Out;

    fn hashed_null_node() -> <H as Hasher>::Out {
        let empty_node = <Self as NodeCodecT>::empty_node();
        let hash_result = H::hash(empty_node);
        log::debug!(target: "zk-trie", "NodeCodec::hashed_null_node: empty_node={:02x?}, hash={:02x?}", empty_node, hash_result.as_ref());
        hash_result
    }

    fn decode_plan(data: &[u8]) -> Result<NodePlan, Self::Error> {
        log::debug!(target: "zk-trie", "NodeCodec::decode_plan called with data: {:02x?}", data);

        // Handle empty data
        if data.is_empty() {
            log::debug!(target: "zk-trie", "NodeCodec::decode_plan: empty data, returning Empty node plan");
            return Ok(NodePlan::Empty);
        }

        let mut input = ByteSliceInput::new(data);
        let header = NodeHeader::decode(&mut input)?;
        let contains_hash = header.contains_hash_of_value();

        let branch_has_value = if let NodeHeader::Branch(has_value, _) = &header {
            *has_value
        } else {
            // hashed_value_branch
            true
        };

        match header {
            NodeHeader::Null => Ok(NodePlan::Empty),
            NodeHeader::HashedValueBranch(nibble_count) | NodeHeader::Branch(_, nibble_count) => {
                let padding = nibble_count % nibble_ops::NIBBLE_PER_BYTE != 0;
                // check that the padding is valid (if any)
                if padding && nibble_ops::pad_left(data[input.offset]) != 0 {
                    return Err(Error::BadFormat);
                }
                let nibble_bytes = (nibble_count + (nibble_ops::NIBBLE_PER_BYTE - 1))
                    / nibble_ops::NIBBLE_PER_BYTE;
                let felt_aligned_bytes = ((nibble_bytes + 7) / 8) * 8;
                let felt_aligned_range = input.take(felt_aligned_bytes)?;
                // Only pass the actual nibble data to NibbleSlicePlan, not the padding
                let partial = felt_aligned_range.start..(felt_aligned_range.start + nibble_bytes);
                let partial_padding = nibble_ops::number_padding(nibble_count);
                let bitmap_range = input.take(BITMAP_LENGTH)?;
                let bitmap = Bitmap::decode(&data[bitmap_range])?;
                let value = if branch_has_value {
                    Some(if contains_hash {
                        ValuePlan::Node(input.take(H::LENGTH)?)
                    } else {
                        // Read 8-byte little-endian length
                        let length_range = input.take(8)?;
                        let length_bytes = &data[length_range];
                        let mut length_array = [0u8; 8];
                        length_array.copy_from_slice(length_bytes);
                        let count = u64::from_le_bytes(length_array) as usize;
                        // Calculate felt-aligned length to consume padding
                        let value_aligned_len = ((count + 7) / 8) * 8;
                        let value_range = input.take(value_aligned_len)?;
                        // Only return the actual value data, not the padding
                        ValuePlan::Inline(value_range.start..(value_range.start + count))
                    })
                } else {
                    None
                };
                let mut children = [
                    None, None, None, None, None, None, None, None, None, None, None, None, None,
                    None, None, None,
                ];
                for i in 0..nibble_ops::NIBBLE_LENGTH {
                    if bitmap.value_at(i) {
                        // Read 8-byte little-endian length
                        let length_range = input.take(8)?;
                        let length_bytes = &data[length_range];
                        let mut length_array = [0u8; 8];
                        length_array.copy_from_slice(length_bytes);
                        let count = u64::from_le_bytes(length_array) as usize;

                        let range = input.take(count)?;
                        children[i] = Some(if count == H::LENGTH {
                            NodeHandlePlan::Hash(range)
                        } else {
                            NodeHandlePlan::Inline(range)
                        });
                    }
                }
                Ok(NodePlan::NibbledBranch {
                    partial: NibbleSlicePlan::new(partial, partial_padding),
                    value,
                    children,
                })
            }
            NodeHeader::HashedValueLeaf(nibble_count) | NodeHeader::Leaf(nibble_count) => {
                let nibble_bytes = (nibble_count + (nibble_ops::NIBBLE_PER_BYTE - 1))
                    / nibble_ops::NIBBLE_PER_BYTE;

                // Calculate prefix padding to ensure partial key data aligns to felt boundaries
                let misalignment = nibble_bytes % 8;
                let prefix_padding = if misalignment == 0 {
                    0
                } else {
                    8 - misalignment
                };
                let total_nibble_section = ((prefix_padding + nibble_bytes + 7) / 8) * 8;

                let felt_aligned_range = input.take(total_nibble_section)?;

                // Skip prefix padding and locate actual nibble data
                let nibble_start = felt_aligned_range.start + prefix_padding;
                let nibble_padding = nibble_count % nibble_ops::NIBBLE_PER_BYTE != 0;
                // check that the padding is valid (if any)
                if nibble_padding && nibble_ops::pad_left(data[nibble_start]) != 0 {
                    return Err(Error::BadFormat);
                }

                let partial = nibble_start..(nibble_start + nibble_bytes);
                let partial_padding = nibble_ops::number_padding(nibble_count);
                let value = if contains_hash {
                    ValuePlan::Node(input.take(H::LENGTH)?)
                } else {
                    // Read 8-byte little-endian length
                    let length_range = input.take(8)?;
                    let length_bytes = &data[length_range];
                    let mut length_array = [0u8; 8];
                    length_array.copy_from_slice(length_bytes);
                    let count = u64::from_le_bytes(length_array) as usize;
                    // Calculate felt-aligned length to consume padding
                    let value_aligned_len = ((count + 7) / 8) * 8;
                    let value_range = input.take(value_aligned_len)?;
                    // Only return the actual value data, not the padding
                    ValuePlan::Inline(value_range.start..(value_range.start + count))
                };

                Ok(NodePlan::Leaf {
                    partial: NibbleSlicePlan::new(partial, partial_padding),
                    value,
                })
            }
        }
    }

    fn is_empty_node(data: &[u8]) -> bool {
        data == <Self as NodeCodecT>::empty_node()
    }

    fn empty_node() -> &'static [u8] {
        // Return 8-byte encoding for Null header (type 0, nibble_count 0)
        // This matches NodeHeader::Null.encode() output
        let empty = &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        log::debug!(target: "zk-trie", "NodeCodec::empty_node returning: {:02x?}", empty);
        empty
    }

    fn leaf_node(partial: impl Iterator<Item = u8>, number_nibble: usize, value: Value) -> Vec<u8> {
        let contains_hash = matches!(&value, Value::Node(..));
        let mut output = if contains_hash {
            partial_from_iterator_encode(partial, number_nibble, NodeKind::HashedValueLeaf)
        } else {
            partial_from_iterator_encode(partial, number_nibble, NodeKind::Leaf)
        };
        match value {
            Value::Inline(value) => {
                // Encode length as 8-byte little-endian
                let length_bytes = (value.len() as u64).to_le_bytes();
                output.extend_from_slice(&length_bytes);

                // Add value data
                output.extend_from_slice(value);

                // Pad value data to 8-byte boundary
                let value_aligned_len = ((value.len() + 7) / 8) * 8;
                let padding_needed = value_aligned_len - value.len();
                for _ in 0..padding_needed {
                    output.push(0);
                }
            }
            Value::Node(hash) => {
                debug_assert!(hash.len() == H::LENGTH);
                output.extend_from_slice(hash);
            }
        }
        output
    }

    fn extension_node(
        _partial: impl Iterator<Item = u8>,
        _nbnibble: usize,
        _child: ChildReference<<H as Hasher>::Out>,
    ) -> Vec<u8> {
        unreachable!("No extension codec.")
    }

    fn branch_node(
        _children: impl Iterator<Item = impl Borrow<Option<ChildReference<<H as Hasher>::Out>>>>,
        _maybe_value: Option<Value>,
    ) -> Vec<u8> {
        unreachable!("No extension codec.")
    }

    fn branch_node_nibbled(
        partial: impl Iterator<Item = u8>,
        number_nibble: usize,
        children: impl Iterator<Item = impl Borrow<Option<ChildReference<<H as Hasher>::Out>>>>,
        value: Option<Value>,
    ) -> Vec<u8> {
        let contains_hash = matches!(&value, Some(Value::Node(..)));
        let mut output = match (&value, contains_hash) {
            (&None, _) => {
                partial_from_iterator_encode(partial, number_nibble, NodeKind::BranchNoValue)
            }
            (_, false) => {
                partial_from_iterator_encode(partial, number_nibble, NodeKind::BranchWithValue)
            }
            (_, true) => {
                partial_from_iterator_encode(partial, number_nibble, NodeKind::HashedValueBranch)
            }
        };

        let bitmap_index = output.len();
        let mut bitmap: [u8; BITMAP_LENGTH] = [0; BITMAP_LENGTH];
        (0..BITMAP_LENGTH).for_each(|_| output.push(0));
        match value {
            Some(Value::Inline(value)) => {
                // Encode length as 8-byte little-endian
                let length_bytes = (value.len() as u64).to_le_bytes();
                output.extend_from_slice(&length_bytes);
                output.extend_from_slice(value);

                // Pad value data to 8-byte boundary
                let value_aligned_len = ((value.len() + 7) / 8) * 8;
                let padding_needed = value_aligned_len - value.len();
                for _ in 0..padding_needed {
                    output.push(0);
                }
            }
            Some(Value::Node(hash)) => {
                debug_assert!(hash.len() == H::LENGTH);
                output.extend_from_slice(hash);
            }
            None => (),
        }
        Bitmap::encode(
            children.map(|maybe_child| match maybe_child.borrow() {
                Some(ChildReference::Hash(h)) => {
                    // Always encode hash references with 8-byte length prefix
                    let length_bytes = (h.as_ref().len() as u64).to_le_bytes();
                    output.extend_from_slice(&length_bytes);
                    output.extend_from_slice(h.as_ref());
                    true
                }
                &Some(ChildReference::Inline(inline_data, len)) => {
                    // Encode length as 8-byte little-endian
                    let length_bytes = (len as u64).to_le_bytes();
                    output.extend_from_slice(&length_bytes);
                    output.extend_from_slice(&inline_data.as_ref()[..len]);
                    true
                }
                None => false,
            }),
            bitmap.as_mut(),
        );
        output[bitmap_index..bitmap_index + BITMAP_LENGTH]
            .copy_from_slice(&bitmap[..BITMAP_LENGTH]);
        output
    }
}

// utils

/// Encode and allocate node type header (type and size), and partial value.
/// It uses an iterator over encoded partial bytes as input.
fn partial_from_iterator_encode<I: Iterator<Item = u8>>(
    partial: I,
    nibble_count: usize,
    node_kind: NodeKind,
) -> Vec<u8> {
    let nibble_bytes =
        (nibble_count + (nibble_ops::NIBBLE_PER_BYTE - 1)) / nibble_ops::NIBBLE_PER_BYTE;

    // For leaf nodes, ensure partial key data aligns to felt boundaries
    let (prefix_padding, total_nibble_section) = match node_kind {
        NodeKind::Leaf | NodeKind::HashedValueLeaf => {
            // Calculate prefix padding to ensure proper felt boundary alignment
            // for ZK proof verification - align partial data to end at felt boundary
            let misalignment = nibble_bytes % 8;
            let prefix_pad = if misalignment == 0 {
                0
            } else {
                8 - misalignment
            };
            let total_section = ((prefix_pad + nibble_bytes + 7) / 8) * 8;
            (prefix_pad, total_section)
        }
        _ => {
            // Branch nodes use standard padding
            let felt_aligned_bytes = ((nibble_bytes + 7) / 8) * 8;
            (0, felt_aligned_bytes)
        }
    };

    let mut output = Vec::with_capacity(8 + total_nibble_section);
    match node_kind {
        NodeKind::Leaf => NodeHeader::Leaf(nibble_count).encode_to(&mut output),
        NodeKind::BranchWithValue => NodeHeader::Branch(true, nibble_count).encode_to(&mut output),
        NodeKind::BranchNoValue => NodeHeader::Branch(false, nibble_count).encode_to(&mut output),
        NodeKind::HashedValueLeaf => {
            NodeHeader::HashedValueLeaf(nibble_count).encode_to(&mut output)
        }
        NodeKind::HashedValueBranch => {
            NodeHeader::HashedValueBranch(nibble_count).encode_to(&mut output)
        }
    };

    // Add prefix padding for leaf nodes
    for _ in 0..prefix_padding {
        output.push(0);
    }

    // Collect partial bytes and add them
    let partial_bytes: Vec<u8> = partial.collect();
    output.extend_from_slice(&partial_bytes);

    // Pad with zeros to reach total section size
    while output.len() - 8 < total_nibble_section {
        output.push(0);
    }

    output
}

const BITMAP_LENGTH: usize = 8;

/// Radix 16 trie, bitmap encoding implementation,
/// it contains children mapping information for a branch
/// (children presence only), it encodes into
/// a compact bitmap encoding representation.
pub(crate) struct Bitmap(u64);

impl Bitmap {
    pub fn decode(data: &[u8]) -> Result<Self, codec::Error> {
        let value = u64::decode(&mut &data[..])?;
        if value == 0 {
            Err("Bitmap without a child.".into())
        } else {
            Ok(Bitmap(value))
        }
    }

    pub fn value_at(&self, i: usize) -> bool {
        self.0 & (1u64 << i) != 0
    }

    pub fn encode<I: Iterator<Item = bool>>(has_children: I, dest: &mut [u8]) {
        let mut bitmap: u64 = 0;
        let mut cursor: u64 = 1;
        for v in has_children {
            if v {
                bitmap |= cursor
            }
            cursor <<= 1;
        }
        // Store as little-endian 8-byte value
        let bytes = bitmap.to_le_bytes();
        dest[..8].copy_from_slice(&bytes);
    }
}
