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

//! The node header.

use codec::{Decode, Encode, Input, Output};

use crate::trie_constants;

/// A node header
#[derive(Copy, Clone, PartialEq, Eq, sp_core::RuntimeDebug)]
pub(crate) enum NodeHeader {
    Null,
    // contains wether there is a value and nibble count
    Branch(bool, usize),
    // contains nibble count
    Leaf(usize),
    // contains nibble count.
    HashedValueBranch(usize),
    // contains nibble count.
    HashedValueLeaf(usize),
}

impl NodeHeader {
    pub(crate) fn contains_hash_of_value(&self) -> bool {
        matches!(
            self,
            NodeHeader::HashedValueBranch(_) | NodeHeader::HashedValueLeaf(_)
        )
    }
}

/// NodeHeader without content
pub(crate) enum NodeKind {
    Leaf,
    BranchNoValue,
    BranchWithValue,
    HashedValueLeaf,
    HashedValueBranch,
}
use trie_constants::EMPTY_TRIE;

impl Encode for NodeHeader {
    fn encode_to<T: Output + ?Sized>(&self, output: &mut T) {
        let value: u64 = match self {
            NodeHeader::Null => EMPTY_TRIE, // Type 0
            NodeHeader::Branch(true, nibble_count) => {
                (1u64 << 60) | (*nibble_count as u64) // Type 1
            }
            NodeHeader::Branch(false, nibble_count) => {
                (2u64 << 60) | (*nibble_count as u64) // Type 2
            }
            NodeHeader::Leaf(nibble_count) => {
                (3u64 << 60) | (*nibble_count as u64) // Type 3
            }
            NodeHeader::HashedValueBranch(nibble_count) => {
                (4u64 << 60) | (*nibble_count as u64) // Type 4
            }
            NodeHeader::HashedValueLeaf(nibble_count) => {
                (5u64 << 60) | (*nibble_count as u64) // Type 5
            }
        };
        output.write(&value.to_le_bytes());
    }
}

impl codec::EncodeLike for NodeHeader {}

impl Decode for NodeHeader {
    fn decode<I: Input>(input: &mut I) -> Result<Self, codec::Error> {
        let mut bytes = [0u8; 8];
        input.read(&mut bytes)?;
        let value = u64::from_le_bytes(bytes);
        let type_code = (value >> 60) & 0xF; // Extract bits 63–60
        let nibble_count = (value & 0xFFFFFFFF) as usize; // Extract bits 31–0
        match type_code {
            0 => Ok(NodeHeader::Null),
            1 => Ok(NodeHeader::Branch(true, nibble_count)),
            2 => Ok(NodeHeader::Branch(false, nibble_count)),
            3 => Ok(NodeHeader::Leaf(nibble_count)),
            4 => Ok(NodeHeader::HashedValueBranch(nibble_count)),
            5 => Ok(NodeHeader::HashedValueLeaf(nibble_count)),
            _ => Err(codec::Error::from("Invalid NodeHeader type")),
        }
    }
}
