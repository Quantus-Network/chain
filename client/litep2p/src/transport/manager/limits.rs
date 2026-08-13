// Copyright 2024 litep2p developers
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! Limits for the transport manager.

use crate::types::ConnectionId;

use std::collections::HashSet;

/// Configuration for the connection limits.
#[derive(Debug, Clone, Default)]
pub struct ConnectionLimitsConfig {
	/// Maximum number of incoming connections that can be established.
	max_incoming_connections: Option<usize>,
	/// Maximum number of outgoing connections that can be established.
	max_outgoing_connections: Option<usize>,
	/// Maximum number of incoming connections that have been accepted but not
	/// yet established (in handshake / negotiation).
	///
	/// This must be set independently of [`Self::max_incoming_connections`]:
	/// established-only limits do not bound pre-handshake sockets, which hold a
	/// file descriptor until negotiation times out.
	max_pending_incoming_connections: Option<usize>,
}

impl ConnectionLimitsConfig {
	/// Configures the maximum number of incoming connections that can be established.
	pub fn max_incoming_connections(mut self, limit: Option<usize>) -> Self {
		self.max_incoming_connections = limit;
		self
	}

	/// Configures the maximum number of outgoing connections that can be established.
	pub fn max_outgoing_connections(mut self, limit: Option<usize>) -> Self {
		self.max_outgoing_connections = limit;
		self
	}

	/// Configures the maximum number of incoming connections in handshake.
	pub fn max_pending_incoming_connections(mut self, limit: Option<usize>) -> Self {
		self.max_pending_incoming_connections = limit;
		self
	}
}

/// Error type for connection limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionLimitsError {
	/// Maximum number of incoming connections exceeded.
	MaxIncomingConnectionsExceeded,
	/// Maximum number of outgoing connections exceeded.
	MaxOutgoingConnectionsExceeded,
}

/// Connection limits.
#[derive(Debug, Clone)]
pub struct ConnectionLimits {
	/// Configuration for the connection limits.
	config: ConnectionLimitsConfig,

	/// Incoming connections that have been accepted but not yet established.
	pending_incoming: HashSet<ConnectionId>,
	/// Established incoming connections.
	incoming_connections: HashSet<ConnectionId>,
	/// Established outgoing connections.
	outgoing_connections: HashSet<ConnectionId>,
}

impl ConnectionLimits {
	/// Creates a new connection limits instance.
	pub fn new(config: ConnectionLimitsConfig) -> Self {
		let max_incoming_connections = config.max_incoming_connections.unwrap_or(0);
		let max_outgoing_connections = config.max_outgoing_connections.unwrap_or(0);
		let max_pending_incoming = config.max_pending_incoming_connections.unwrap_or(0);

		Self {
			config,
			pending_incoming: HashSet::with_capacity(max_pending_incoming),
			incoming_connections: HashSet::with_capacity(max_incoming_connections),
			outgoing_connections: HashSet::with_capacity(max_outgoing_connections),
		}
	}

	/// Called when dialing an address.
	///
	/// Returns the number of outgoing connections permitted to be established.
	/// It is guaranteed that at least one connection can be established if the method returns `Ok`.
	/// The number of available outgoing connections can influence the maximum parallel dials to a
	/// single address.
	///
	/// If the maximum number of outgoing connections is not set, `Ok(usize::MAX)` is returned.
	pub fn on_dial_address(&mut self) -> Result<usize, ConnectionLimitsError> {
		if let Some(max_outgoing_connections) = self.config.max_outgoing_connections {
			if self.outgoing_connections.len() >= max_outgoing_connections {
				return Err(ConnectionLimitsError::MaxOutgoingConnectionsExceeded);
			}

			return Ok(max_outgoing_connections - self.outgoing_connections.len());
		}

		Ok(usize::MAX)
	}

	/// Called before accepting a new incoming connection, including pre-handshake.
	///
	/// Counts the connection against pending (and combined incoming) limits so
	/// unauthenticated peers cannot exhaust file descriptors.
	pub fn on_incoming(
		&mut self,
		connection_id: ConnectionId,
	) -> Result<(), ConnectionLimitsError> {
		if let Some(max_pending) = self.config.max_pending_incoming_connections {
			if self.pending_incoming.len() >= max_pending {
				return Err(ConnectionLimitsError::MaxIncomingConnectionsExceeded);
			}
		}

		if let Some(max_incoming_connections) = self.config.max_incoming_connections {
			if self.pending_incoming.len() + self.incoming_connections.len() >=
				max_incoming_connections
			{
				return Err(ConnectionLimitsError::MaxIncomingConnectionsExceeded);
			}
		}

		if self.config.max_pending_incoming_connections.is_some() ||
			self.config.max_incoming_connections.is_some()
		{
			self.pending_incoming.insert(connection_id);
		}

		Ok(())
	}

	/// Called when a pending incoming connection fails or is rejected before
	/// it is established.
	pub fn on_pending_incoming_failed(&mut self, connection_id: ConnectionId) {
		self.pending_incoming.remove(&connection_id);
	}

	/// Called when a new connection is established.
	///
	/// Returns an error if the connection cannot be accepted due to connection limits.
	pub fn can_accept_connection(
		&mut self,
		is_listener: bool,
	) -> Result<(), ConnectionLimitsError> {
		// Check connection limits.
		if is_listener {
			if let Some(max_incoming_connections) = self.config.max_incoming_connections {
				if self.incoming_connections.len() >= max_incoming_connections {
					return Err(ConnectionLimitsError::MaxIncomingConnectionsExceeded);
				}
			}
		} else if let Some(max_outgoing_connections) = self.config.max_outgoing_connections {
			if self.outgoing_connections.len() >= max_outgoing_connections {
				return Err(ConnectionLimitsError::MaxOutgoingConnectionsExceeded);
			}
		}

		Ok(())
	}

	/// Accept an established connection.
	///
	/// # Note
	///
	/// This method should be called after the `Self::can_accept_connection` method
	/// to ensure that the connection can be accepted.
	pub fn accept_established_connection(
		&mut self,
		connection_id: ConnectionId,
		is_listener: bool,
	) {
		if is_listener {
			self.pending_incoming.remove(&connection_id);
			if self.config.max_incoming_connections.is_some() {
				self.incoming_connections.insert(connection_id);
			}
		} else if self.config.max_outgoing_connections.is_some() {
			self.outgoing_connections.insert(connection_id);
		}
	}

	/// Called when a connection is closed.
	pub fn on_connection_closed(&mut self, connection_id: ConnectionId) {
		self.pending_incoming.remove(&connection_id);
		self.incoming_connections.remove(&connection_id);
		self.outgoing_connections.remove(&connection_id);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::ConnectionId;

	#[test]
	fn connection_limits() {
		let config = ConnectionLimitsConfig::default()
			.max_incoming_connections(Some(3))
			.max_outgoing_connections(Some(2));
		let mut limits = ConnectionLimits::new(config);

		let connection_id_in_1 = ConnectionId::random();
		let connection_id_in_2 = ConnectionId::random();
		let connection_id_out_1 = ConnectionId::random();
		let connection_id_out_2 = ConnectionId::random();
		let connection_id_in_3 = ConnectionId::random();

		// Establish incoming connection.
		assert!(limits.can_accept_connection(true).is_ok());
		limits.accept_established_connection(connection_id_in_1, true);
		assert_eq!(limits.incoming_connections.len(), 1);

		assert!(limits.can_accept_connection(true).is_ok());
		limits.accept_established_connection(connection_id_in_2, true);
		assert_eq!(limits.incoming_connections.len(), 2);

		assert!(limits.can_accept_connection(true).is_ok());
		limits.accept_established_connection(connection_id_in_3, true);
		assert_eq!(limits.incoming_connections.len(), 3);

		assert_eq!(
			limits.can_accept_connection(true).unwrap_err(),
			ConnectionLimitsError::MaxIncomingConnectionsExceeded
		);
		assert_eq!(limits.incoming_connections.len(), 3);

		// Establish outgoing connection.
		assert!(limits.can_accept_connection(false).is_ok());
		limits.accept_established_connection(connection_id_out_1, false);
		assert_eq!(limits.incoming_connections.len(), 3);
		assert_eq!(limits.outgoing_connections.len(), 1);

		assert!(limits.can_accept_connection(false).is_ok());
		limits.accept_established_connection(connection_id_out_2, false);
		assert_eq!(limits.incoming_connections.len(), 3);
		assert_eq!(limits.outgoing_connections.len(), 2);

		assert_eq!(
			limits.can_accept_connection(false).unwrap_err(),
			ConnectionLimitsError::MaxOutgoingConnectionsExceeded
		);

		// Close connections with peer a.
		limits.on_connection_closed(connection_id_in_1);
		assert_eq!(limits.incoming_connections.len(), 2);
		assert_eq!(limits.outgoing_connections.len(), 2);

		limits.on_connection_closed(connection_id_out_1);
		assert_eq!(limits.incoming_connections.len(), 2);
		assert_eq!(limits.outgoing_connections.len(), 1);
	}

	#[test]
	fn pending_incoming_counts_against_limits() {
		let config = ConnectionLimitsConfig::default()
			.max_incoming_connections(Some(3))
			.max_pending_incoming_connections(Some(2));
		let mut limits = ConnectionLimits::new(config);

		let pending_1 = ConnectionId::random();
		let pending_2 = ConnectionId::random();
		let pending_3 = ConnectionId::random();

		assert!(limits.on_incoming(pending_1).is_ok());
		assert_eq!(limits.pending_incoming.len(), 1);
		assert!(limits.on_incoming(pending_2).is_ok());
		assert_eq!(limits.pending_incoming.len(), 2);
		assert_eq!(
			limits.on_incoming(pending_3).unwrap_err(),
			ConnectionLimitsError::MaxIncomingConnectionsExceeded
		);
		assert_eq!(limits.pending_incoming.len(), 2);

		limits.accept_established_connection(pending_1, true);
		assert_eq!(limits.pending_incoming.len(), 1);
		assert_eq!(limits.incoming_connections.len(), 1);

		assert!(limits.on_incoming(pending_3).is_ok());
		assert_eq!(limits.pending_incoming.len(), 2);

		limits.on_pending_incoming_failed(pending_2);
		assert_eq!(limits.pending_incoming.len(), 1);
		assert!(limits.on_incoming(pending_2).is_ok());
	}

	#[test]
	fn pending_plus_established_cannot_exceed_incoming_max() {
		let config = ConnectionLimitsConfig::default().max_incoming_connections(Some(2));
		let mut limits = ConnectionLimits::new(config);

		let a = ConnectionId::random();
		let b = ConnectionId::random();
		let c = ConnectionId::random();

		assert!(limits.on_incoming(a).is_ok());
		limits.accept_established_connection(a, true);
		assert!(limits.on_incoming(b).is_ok());
		assert_eq!(
			limits.on_incoming(c).unwrap_err(),
			ConnectionLimitsError::MaxIncomingConnectionsExceeded
		);

		limits.on_pending_incoming_failed(b);
		assert!(limits.on_incoming(c).is_ok());
	}
}
