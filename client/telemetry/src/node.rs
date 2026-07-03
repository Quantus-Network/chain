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

use crate::{
	transport::{connect_to_endpoint, WsTransport},
	TelemetryPayload,
};
use futures::{channel::mpsc, prelude::*};
use rand::Rng as _;
use std::{
	fmt, mem,
	pin::Pin,
	task::{Context, Poll},
	time::Duration,
};
use tokio::time::Sleep;
use url::Url;

pub(crate) type ConnectionNotifierSender = mpsc::Sender<()>;
pub(crate) type ConnectionNotifierReceiver = mpsc::Receiver<()>;

pub(crate) fn connection_notifier_channel() -> (ConnectionNotifierSender, ConnectionNotifierReceiver)
{
	mpsc::channel(0)
}

/// A boxed future for dialing.
type DialFuture =
	Pin<Box<dyn Future<Output = Result<WsTransport, crate::transport::TransportError>> + Send>>;

/// Handler for a single telemetry node.
///
/// This is a wrapper `Sink` around a network `Sink` with 3 particularities:
///  - It is infallible: if the connection stops, it will reconnect automatically when the server
///    becomes available again.
///  - It holds a list of "connection messages" which are sent automatically when the connection is
///    (re-)established. This is used for the "system.connected" message that needs to be send for
///    every substrate node that connects.
///  - It doesn't stay in pending while waiting for connection. Instead, it moves data into the void
///    if the connection could not be established. This is important for the `Dispatcher` `Sink`
///    which we don't want to block if one connection is broken.
pub(crate) struct Node {
	/// Address of the node.
	addr: Url,
	/// State of the connection.
	socket: NodeSocket,
	/// Messages that are sent when the connection (re-)establishes.
	pub(crate) connection_messages: Vec<TelemetryPayload>,
	/// Notifier for when the connection (re-)establishes.
	pub(crate) telemetry_connection_notifier: Vec<ConnectionNotifierSender>,
}

impl fmt::Debug for Node {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Node")
			.field("addr", &self.addr.as_str())
			.field("socket", &self.socket)
			.field("connection_messages_count", &self.connection_messages.len())
			.field("notifiers_count", &self.telemetry_connection_notifier.len())
			.finish()
	}
}

enum NodeSocket {
	/// We're connected to the node. This is the normal state.
	Connected(NodeSocketConnected),
	/// We are currently dialing the node.
	Dialing(DialFuture),
	/// A new connection should be started as soon as possible.
	ReconnectNow,
	/// Waiting before attempting to dial again.
	WaitingReconnect(Pin<Box<Sleep>>),
	/// Temporary transition state.
	Poisoned,
}

impl NodeSocket {
	fn wait_reconnect() -> NodeSocket {
		let random_delay = rand::thread_rng().gen_range(10..20);
		let delay = tokio::time::sleep(Duration::from_secs(random_delay));
		log::trace!(target: "telemetry", "Pausing for {} secs before reconnecting", random_delay);
		NodeSocket::WaitingReconnect(Box::pin(delay))
	}
}

struct NodeSocketConnected {
	/// Where to send data.
	sink: WsTransport,
	/// Queue of packets to send before accepting new packets.
	buf: Vec<Vec<u8>>,
}

impl NodeSocketConnected {
	/// Drain the read half of the connection, discarding any inbound messages.
	///
	/// The telemetry protocol is write-only from the node's perspective, but the read half
	/// still needs to be polled: tungstenite only processes incoming control frames (queueing
	/// a Pong reply to a server Ping) when the socket is read. This also detects
	/// server-initiated Close frames or disconnects instead of waiting for a write to fail.
	fn poll_drain_read(&mut self, cx: &mut Context<'_>) -> Result<(), std::io::Error> {
		loop {
			match self.sink.poll_next_unpin(cx) {
				Poll::Ready(Some(Ok(data))) => {
					log::trace!(
						target: "telemetry",
						"Discarding {} bytes received from the server",
						data.len(),
					);
				},
				Poll::Ready(Some(Err(err))) => return Err(err),
				Poll::Ready(None) =>
					return Err(std::io::Error::new(
						std::io::ErrorKind::ConnectionAborted,
						"connection closed by the server",
					)),
				Poll::Pending => return Ok(()),
			}
		}
	}
}

impl Node {
	/// Builds a new node handler.
	pub(crate) fn new(
		addr: Url,
		connection_messages: Vec<serde_json::Map<String, serde_json::Value>>,
		telemetry_connection_notifier: Vec<ConnectionNotifierSender>,
	) -> Self {
		Node {
			addr,
			socket: NodeSocket::ReconnectNow,
			connection_messages,
			telemetry_connection_notifier,
		}
	}

	// NOTE: this code has been inspired from `Buffer` (`futures_util::sink::Buffer`).
	//       https://docs.rs/futures-util/0.3.8/src/futures_util/sink/buffer.rs.html#32
	fn try_send_connection_messages(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		conn: &mut NodeSocketConnected,
	) -> Poll<Result<(), std::io::Error>> {
		while let Some(item) = conn.buf.pop() {
			if let Err(e) = conn.sink.start_send_unpin(item) {
				return Poll::Ready(Err(e));
			}
			futures::ready!(conn.sink.poll_ready_unpin(cx))?;
		}
		Poll::Ready(Ok(()))
	}
}

pub(crate) enum Infallible {}

impl Sink<TelemetryPayload> for Node {
	type Error = Infallible;

	fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
		let mut socket = mem::replace(&mut self.socket, NodeSocket::Poisoned);
		self.socket = loop {
			match socket {
				NodeSocket::Connected(mut conn) => {
					if let Err(err) = conn.poll_drain_read(cx) {
						log::warn!(target: "telemetry", "⚠️  Disconnected from {}: {:?}", self.addr, err);
						socket = NodeSocket::wait_reconnect();
						continue;
					}

					match conn.sink.poll_ready_unpin(cx) {
						Poll::Ready(Ok(())) => {
							match self.as_mut().try_send_connection_messages(cx, &mut conn) {
								Poll::Ready(Err(err)) => {
									log::warn!(target: "telemetry", "⚠️  Disconnected from {}: {:?}", self.addr, err);
									socket = NodeSocket::wait_reconnect();
								},
								Poll::Ready(Ok(())) => {
									self.socket = NodeSocket::Connected(conn);
									return Poll::Ready(Ok(()));
								},
								Poll::Pending => {
									self.socket = NodeSocket::Connected(conn);
									return Poll::Pending;
								},
							}
						},
						Poll::Ready(Err(err)) => {
							log::warn!(target: "telemetry", "⚠️  Disconnected from {}: {:?}", self.addr, err);
							socket = NodeSocket::wait_reconnect();
						},
						Poll::Pending => {
							self.socket = NodeSocket::Connected(conn);
							return Poll::Pending;
						},
					}
				},
				NodeSocket::Dialing(mut fut) => match fut.as_mut().poll(cx) {
					Poll::Ready(Ok(ws_transport)) => {
						log::debug!(target: "telemetry", "✅ Connected to {}", self.addr);

						{
							let mut index = 0;
							while index < self.telemetry_connection_notifier.len() {
								let sender = &mut self.telemetry_connection_notifier[index];
								if let Err(error) = sender.try_send(()) {
									if !error.is_disconnected() {
										log::debug!(target: "telemetry", "Failed to send a telemetry connection notification: {}", error);
									} else {
										self.telemetry_connection_notifier.swap_remove(index);
										continue;
									}
								}
								index += 1;
							}
						}

						let buf = self
							.connection_messages
							.iter()
							.map(|json| {
								let mut json = json.clone();
								json.insert(
									"ts".to_string(),
									chrono::Local::now().to_rfc3339().into(),
								);
								json
							})
							.filter_map(|json| match serde_json::to_vec(&json) {
								Ok(message) => Some(message),
								Err(err) => {
									log::error!(
										target: "telemetry",
										"An error occurred while generating new connection \
										messages: {}",
										err,
									);
									None
								},
							})
							.collect();

						socket =
							NodeSocket::Connected(NodeSocketConnected { sink: ws_transport, buf });
					},
					Poll::Pending => break NodeSocket::Dialing(fut),
					Poll::Ready(Err(err)) => {
						log::warn!(target: "telemetry", "❌ Error while dialing {}: {:?}", self.addr, err);
						socket = NodeSocket::wait_reconnect();
					},
				},
				NodeSocket::ReconnectNow => {
					let addr = self.addr.clone();
					log::trace!(target: "telemetry", "Re-dialing {}", self.addr);
					let dial_future: DialFuture =
						Box::pin(async move { connect_to_endpoint(&addr).await });
					socket = NodeSocket::Dialing(dial_future);
				},
				NodeSocket::WaitingReconnect(mut s) =>
					if s.as_mut().poll(cx).is_ready() {
						socket = NodeSocket::ReconnectNow;
					} else {
						break NodeSocket::WaitingReconnect(s);
					},
				NodeSocket::Poisoned => {
					log::error!(target: "telemetry", "‼️ Poisoned connection with {}", self.addr);
					break NodeSocket::Poisoned;
				},
			}
		};

		// The Dispatcher blocks when the Node syncs blocks. This is why it is important that the
		// Node sinks don't go into "Pending" state while waiting for reconnection but rather
		// discard the excess of telemetry messages.
		Poll::Ready(Ok(()))
	}

	fn start_send(mut self: Pin<&mut Self>, item: TelemetryPayload) -> Result<(), Self::Error> {
		// Any buffered outgoing telemetry messages are discarded while (re-)connecting.
		match &mut self.socket {
			NodeSocket::Connected(conn) => match serde_json::to_vec(&item) {
				Ok(data) => {
					log::trace!(target: "telemetry", "Sending {} bytes", data.len());
					let _ = conn.sink.start_send_unpin(data);
				},
				Err(err) => log::debug!(
					target: "telemetry",
					"Could not serialize payload: {}",
					err,
				),
			},
			// We are currently dialing the node.
			NodeSocket::Dialing(_) => log::trace!(target: "telemetry", "Dialing"),
			// A new connection should be started as soon as possible.
			NodeSocket::ReconnectNow => log::trace!(target: "telemetry", "Reconnecting"),
			// Waiting before attempting to dial again.
			NodeSocket::WaitingReconnect(_) => {},
			// Temporary transition state.
			NodeSocket::Poisoned => log::trace!(target: "telemetry", "Poisoned"),
		}
		Ok(())
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		match &mut self.socket {
			NodeSocket::Connected(conn) => match conn.sink.poll_flush_unpin(cx) {
				Poll::Ready(Err(e)) => {
					log::trace!(target: "telemetry", "[poll_flush] Error: {:?}", e);
					self.socket = NodeSocket::wait_reconnect();
					Poll::Ready(Ok(()))
				},
				Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
				Poll::Pending => Poll::Pending,
			},
			_ => Poll::Ready(Ok(())),
		}
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		match &mut self.socket {
			NodeSocket::Connected(conn) => conn.sink.poll_close_unpin(cx).map(|_| Ok(())),
			_ => Poll::Ready(Ok(())),
		}
	}
}

impl fmt::Debug for NodeSocket {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		use NodeSocket::*;
		f.write_str(match self {
			Connected(_) => "Connected",
			Dialing(_) => "Dialing",
			ReconnectNow => "ReconnectNow",
			WaitingReconnect(_) => "WaitingReconnect",
			Poisoned => "Poisoned",
		})
	}
}
