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

use futures::{
	prelude::*,
	task::{Context, Poll},
};
use std::{io, pin::Pin, time::Duration};
use tokio::time::timeout;
use tokio_tungstenite::{
	connect_async,
	tungstenite::{Error as WsError, Message},
	MaybeTlsStream, WebSocketStream,
};
use url::Url;

/// Timeout after which a connection attempt is considered failed. Includes the WebSocket HTTP
/// upgrading.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Error type for WebSocket transport operations.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
	/// WebSocket error.
	#[error("WebSocket error: {0}")]
	WebSocket(#[from] WsError),
	/// Connection timeout.
	#[error("Connection timeout")]
	Timeout,
	/// IO error.
	#[error("IO error: {0}")]
	Io(#[from] io::Error),
}

/// The WebSocket connection type using tokio-tungstenite.
pub(crate) type WsConnection = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// A wrapper around a WebSocket connection that implements Stream and Sink for Vec<u8>.
#[pin_project::pin_project]
pub(crate) struct WsTransport {
	#[pin]
	inner: WsConnection,
}

impl WsTransport {
	/// Create a new WsTransport from a WebSocket connection.
	pub fn new(inner: WsConnection) -> Self {
		Self { inner }
	}
}

impl Stream for WsTransport {
	type Item = Result<Vec<u8>, io::Error>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let mut this = self.project();
		loop {
			return match futures::ready!(this.inner.as_mut().poll_next(cx)) {
				Some(Ok(msg)) => match msg {
					Message::Binary(data) => Poll::Ready(Some(Ok(data.to_vec()))),
					Message::Text(text) => Poll::Ready(Some(Ok(text.as_bytes().to_vec()))),
					Message::Close(_) => Poll::Ready(None),
					// Ping/Pong are handled automatically by tungstenite (reading a Ping
					// queues a Pong reply); skip them and poll for the next frame.
					Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
				},
				Some(Err(e)) => Poll::Ready(Some(Err(io::Error::new(io::ErrorKind::Other, e)))),
				None => Poll::Ready(None),
			}
		}
	}
}

impl Sink<Vec<u8>> for WsTransport {
	type Error = io::Error;

	fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		let this = self.project();
		this.inner.poll_ready(cx).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
	}

	fn start_send(self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
		let this = self.project();
		this.inner
			.start_send(Message::Binary(item.into()))
			.map_err(|e| io::Error::new(io::ErrorKind::Other, e))
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		let this = self.project();
		this.inner.poll_flush(cx).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		let this = self.project();
		this.inner.poll_close(cx).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
	}
}

/// Connect to a WebSocket endpoint with timeout.
pub(crate) async fn connect_to_endpoint(url: &Url) -> Result<WsTransport, TransportError> {
	let result = timeout(CONNECT_TIMEOUT, connect_async(url.as_str())).await;

	match result {
		Ok(Ok((ws_stream, _response))) => Ok(WsTransport::new(ws_stream)),
		Ok(Err(e)) => Err(TransportError::WebSocket(e)),
		Err(_) => Err(TransportError::Timeout),
	}
}
