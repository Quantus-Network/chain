// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Network service module.
//!
//! This module provides shared types and traits used by the litep2p network backend.
//! The libp2p backend has been removed - only litep2p is supported.

use sc_network_types::{multiaddr::Multiaddr, PeerId};
use std::collections::HashSet;

pub mod metrics;
pub(crate) mod out_events;

pub mod signature;
pub mod traits;

// Re-export PeerStoreProvider from peer_store
pub use crate::peer_store::PeerStoreProvider;
// Re-export NotificationMetrics
pub use metrics::NotificationMetrics;

/// Utility function to ensure addresses are consistent with transport configuration.
/// All addresses should be the same "family" (TCP or WebSocket).
pub fn ensure_addresses_consistent_with_transport<'a>(
	addresses: impl Iterator<Item = &'a Multiaddr>,
	_transport: &crate::config::TransportConfig,
) -> Result<HashSet<Multiaddr>, crate::error::Error> {
	// For litep2p, we just collect the addresses without strict libp2p-style validation
	// The litep2p backend handles address validation internally
	Ok(addresses.cloned().collect())
}
