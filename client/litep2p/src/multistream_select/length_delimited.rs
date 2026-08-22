// Copyright 2017 Parity Technologies (UK) Ltd.
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

use bytes::{Buf as _, BufMut as _, Bytes, BytesMut};
use futures::{io::IoSlice, prelude::*};
use std::{
	convert::TryFrom as _,
	io,
	pin::Pin,
	task::{Context, Poll},
};

const MAX_LEN_BYTES: u16 = 2;
const MAX_FRAME_SIZE: u16 = (1 << (MAX_LEN_BYTES * 8 - MAX_LEN_BYTES)) - 1;
/// Inbound negotiation frames are protocol names and short control lines
/// (`/multistream/1.0.0`, `na`, `ls`). 1 KiB is far above any name this
/// node speaks; the 16 KiB varint width is not a useful read budget.
const MAX_NEGOTIATION_FRAME: u16 = 1024;
const DEFAULT_BUFFER_SIZE: usize = 64;
const LOG_TARGET: &str = "litep2p::multistream-select";

/// A `Stream` and `Sink` for unsigned-varint length-delimited frames,
/// wrapping an underlying `AsyncRead + AsyncWrite` I/O resource.
///
/// The unsigned-varint length prefix is at most two bytes (16 KiB), but
/// inbound frames are rejected above 1 KiB. The read buffer grows with
/// bytes actually received, so a peer that declares a length and sends
/// nothing commits no payload allocation. Outbound frames still use the
/// varint width so an `ls` reply listing local protocols is not truncated.
#[pin_project::pin_project]
#[derive(Debug)]
pub struct LengthDelimited<R> {
	/// The inner I/O resource.
	#[pin]
	inner: R,
	/// Read buffer for a single incoming unsigned-varint length-delimited frame.
	read_buffer: BytesMut,
	/// Write buffer for outgoing unsigned-varint length-delimited frames.
	write_buffer: BytesMut,
	/// The current read state, alternating between reading a frame
	/// length and reading a frame payload.
	read_state: ReadState,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ReadState {
	/// We are currently reading the length of the next frame of data.
	ReadLength { buf: [u8; MAX_LEN_BYTES as usize], pos: usize },
	/// We are currently reading the frame of data itself.
	ReadData { len: u16 },
}

impl Default for ReadState {
	fn default() -> Self {
		ReadState::ReadLength { buf: [0; MAX_LEN_BYTES as usize], pos: 0 }
	}
}

impl<R> LengthDelimited<R> {
	/// Creates a new I/O resource for reading and writing unsigned-varint
	/// length delimited frames.
	pub fn new(inner: R) -> LengthDelimited<R> {
		LengthDelimited {
			inner,
			read_state: ReadState::default(),
			read_buffer: BytesMut::with_capacity(DEFAULT_BUFFER_SIZE),
			write_buffer: BytesMut::with_capacity(DEFAULT_BUFFER_SIZE + MAX_LEN_BYTES as usize),
		}
	}

	/// Drops the [`LengthDelimited`] resource, yielding the underlying I/O stream.
	///
	/// # Panic
	///
	/// Will panic if called while there is data in the read or write buffer.
	/// The read buffer is guaranteed to be empty whenever `Stream::poll` yields
	/// a new `Bytes` frame. The write buffer is guaranteed to be empty after
	/// flushing.
	pub fn into_inner(self) -> R {
		assert!(self.read_buffer.is_empty());
		assert!(self.write_buffer.is_empty());
		self.inner
	}

	/// Converts the [`LengthDelimited`] into a [`LengthDelimitedReader`], dropping the
	/// uvi-framed `Sink` in favour of direct `AsyncWrite` access to the underlying
	/// I/O stream.
	///
	/// This is typically done if further uvi-framed messages are expected to be
	/// received but no more such messages are written, allowing the writing of
	/// follow-up protocol data to commence.
	pub fn into_reader(self) -> LengthDelimitedReader<R> {
		LengthDelimitedReader { inner: self }
	}

	/// Writes all buffered frame data to the underlying I/O stream,
	/// _without flushing it_.
	///
	/// After this method returns `Poll::Ready`, the write buffer of frames
	/// submitted to the `Sink` is guaranteed to be empty.
	pub fn poll_write_buffer(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
	) -> Poll<Result<(), io::Error>>
	where
		R: AsyncWrite,
	{
		let mut this = self.project();

		while !this.write_buffer.is_empty() {
			match this.inner.as_mut().poll_write(cx, this.write_buffer) {
				Poll::Pending => return Poll::Pending,
				Poll::Ready(Ok(0)) =>
					return Poll::Ready(Err(io::Error::new(
						io::ErrorKind::WriteZero,
						"Failed to write buffered frame.",
					))),
				Poll::Ready(Ok(n)) => this.write_buffer.advance(n),
				Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
			}
		}

		Poll::Ready(Ok(()))
	}
}

impl<R> Stream for LengthDelimited<R>
where
	R: AsyncRead,
{
	type Item = Result<Bytes, io::Error>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let mut this = self.project();

		loop {
			match this.read_state {
				ReadState::ReadLength { buf, pos } => {
					match this.inner.as_mut().poll_read(cx, &mut buf[*pos..*pos + 1]) {
						Poll::Ready(Ok(0)) =>
							if *pos == 0 {
								return Poll::Ready(None);
							} else {
								return Poll::Ready(Some(Err(io::ErrorKind::UnexpectedEof.into())));
							},
						Poll::Ready(Ok(n)) => {
							debug_assert_eq!(n, 1);
							*pos += n;
						},
						Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
						Poll::Pending => return Poll::Pending,
					};

					if (buf[*pos - 1] & 0x80) == 0 {
						// MSB is not set, indicating the end of the length prefix.
						let (len, _) = unsigned_varint::decode::u16(buf).map_err(|e| {
							tracing::debug!(target: LOG_TARGET, "invalid length prefix: {}", e);
							io::Error::new(io::ErrorKind::InvalidData, "invalid length prefix")
						})?;

						if len > MAX_NEGOTIATION_FRAME {
							tracing::debug!(
								target: LOG_TARGET,
								len,
								max = MAX_NEGOTIATION_FRAME,
								"rejecting negotiation frame outside protocol maximum"
							);
							return Poll::Ready(Some(Err(io::Error::new(
								io::ErrorKind::InvalidData,
								"Maximum frame length exceeded",
							))));
						}

						if len >= 1 {
							*this.read_state = ReadState::ReadData { len };
							this.read_buffer.clear();
						} else {
							debug_assert_eq!(len, 0);
							*this.read_state = ReadState::default();
							return Poll::Ready(Some(Ok(Bytes::new())));
						}
					} else if *pos == MAX_LEN_BYTES as usize {
						// MSB signals more length bytes but we have already read the maximum.
						// See the module documentation about the max frame len.
						return Poll::Ready(Some(Err(io::Error::new(
							io::ErrorKind::InvalidData,
							"Maximum frame length exceeded",
						))));
					}
				},
				ReadState::ReadData { len } => {
					let remaining = *len as usize - this.read_buffer.len();
					debug_assert!(remaining > 0);
					let mut tmp = [0u8; MAX_NEGOTIATION_FRAME as usize];
					let to_read = remaining.min(tmp.len());
					match this.inner.as_mut().poll_read(cx, &mut tmp[..to_read]) {
						Poll::Ready(Ok(0)) =>
							return Poll::Ready(Some(Err(io::ErrorKind::UnexpectedEof.into()))),
						Poll::Ready(Ok(n)) => this.read_buffer.extend_from_slice(&tmp[..n]),
						Poll::Pending => return Poll::Pending,
						Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
					};

					if this.read_buffer.len() == *len as usize {
						let frame = this.read_buffer.split_off(0).freeze();
						*this.read_state = ReadState::default();
						return Poll::Ready(Some(Ok(frame)));
					}
				},
			}
		}
	}
}

impl<R> Sink<Bytes> for LengthDelimited<R>
where
	R: AsyncWrite,
{
	type Error = io::Error;

	fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		// Use the maximum frame length also as a (soft) upper limit
		// for the entire write buffer. The actual (hard) limit is thus
		// implied to be roughly 2 * MAX_FRAME_SIZE.
		if self.as_mut().project().write_buffer.len() >= MAX_FRAME_SIZE as usize {
			match self.as_mut().poll_write_buffer(cx) {
				Poll::Ready(Ok(())) => {},
				Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
				Poll::Pending => return Poll::Pending,
			}

			debug_assert!(self.as_mut().project().write_buffer.is_empty());
		}

		Poll::Ready(Ok(()))
	}

	fn start_send(self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
		let this = self.project();

		let len = match u16::try_from(item.len()) {
			Ok(len) if len <= MAX_FRAME_SIZE => len,
			_ =>
				return Err(io::Error::new(
					io::ErrorKind::InvalidData,
					"Maximum frame size exceeded.",
				)),
		};

		let mut uvi_buf = unsigned_varint::encode::u16_buffer();
		let uvi_len = unsigned_varint::encode::u16(len, &mut uvi_buf);
		this.write_buffer.reserve(len as usize + uvi_len.len());
		this.write_buffer.put(uvi_len);
		this.write_buffer.put(item);

		Ok(())
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		// Write all buffered frame data to the underlying I/O stream.
		match LengthDelimited::poll_write_buffer(self.as_mut(), cx) {
			Poll::Ready(Ok(())) => {},
			Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
			Poll::Pending => return Poll::Pending,
		}

		let this = self.project();
		debug_assert!(this.write_buffer.is_empty());

		// Flush the underlying I/O stream.
		this.inner.poll_flush(cx)
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		// Write all buffered frame data to the underlying I/O stream.
		match LengthDelimited::poll_write_buffer(self.as_mut(), cx) {
			Poll::Ready(Ok(())) => {},
			Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
			Poll::Pending => return Poll::Pending,
		}

		let this = self.project();
		debug_assert!(this.write_buffer.is_empty());

		// Close the underlying I/O stream.
		this.inner.poll_close(cx)
	}
}

/// A `LengthDelimitedReader` implements a `Stream` of uvi-length-delimited
/// frames on an underlying I/O resource combined with direct `AsyncWrite` access.
#[pin_project::pin_project]
#[derive(Debug)]
pub struct LengthDelimitedReader<R> {
	#[pin]
	inner: LengthDelimited<R>,
}

impl<R> LengthDelimitedReader<R> {
	/// Destroys the `LengthDelimitedReader` and returns the underlying I/O stream.
	///
	/// This method is guaranteed not to drop any data read from or not yet
	/// submitted to the underlying I/O stream.
	///
	/// # Panic
	///
	/// Will panic if called while there is data in the read or write buffer.
	/// The read buffer is guaranteed to be empty whenever [`Stream::poll_next`]
	/// yield a new `Message`. The write buffer is guaranteed to be empty whenever
	/// [`LengthDelimited::poll_write_buffer`] yields [`Poll::Ready`] or after
	/// the [`Sink`] has been completely flushed via [`Sink::poll_flush`].
	pub fn into_inner(self) -> R {
		self.inner.into_inner()
	}
}

impl<R> Stream for LengthDelimitedReader<R>
where
	R: AsyncRead,
{
	type Item = Result<Bytes, io::Error>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.project().inner.poll_next(cx)
	}
}

impl<R> AsyncWrite for LengthDelimitedReader<R>
where
	R: AsyncWrite,
{
	fn poll_write(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &[u8],
	) -> Poll<Result<usize, io::Error>> {
		// `this` here designates the `LengthDelimited`.
		let mut this = self.project().inner;

		// We need to flush any data previously written with the `LengthDelimited`.
		match LengthDelimited::poll_write_buffer(this.as_mut(), cx) {
			Poll::Ready(Ok(())) => {},
			Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
			Poll::Pending => return Poll::Pending,
		}
		debug_assert!(this.write_buffer.is_empty());

		this.project().inner.poll_write(cx, buf)
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
		self.project().inner.poll_flush(cx)
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
		self.project().inner.poll_close(cx)
	}

	fn poll_write_vectored(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		bufs: &[IoSlice<'_>],
	) -> Poll<Result<usize, io::Error>> {
		// `this` here designates the `LengthDelimited`.
		let mut this = self.project().inner;

		// We need to flush any data previously written with the `LengthDelimited`.
		match LengthDelimited::poll_write_buffer(this.as_mut(), cx) {
			Poll::Ready(Ok(())) => {},
			Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
			Poll::Pending => return Poll::Pending,
		}
		debug_assert!(this.write_buffer.is_empty());

		this.project().inner.poll_write_vectored(cx, bufs)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use futures::{AsyncRead, Stream, StreamExt};
	use std::{
		pin::Pin,
		task::{Context, Poll},
	};

	fn encode_len(len: u16) -> Vec<u8> {
		let mut buf = unsigned_varint::encode::u16_buffer();
		unsigned_varint::encode::u16(len, &mut buf).to_vec()
	}

	/// Yields `data`, then stalls. Models a peer that sends a length prefix
	/// and never the frame body.
	struct PrefixThenPending {
		data: Vec<u8>,
		pos: usize,
	}

	impl AsyncRead for PrefixThenPending {
		fn poll_read(
			mut self: Pin<&mut Self>,
			_cx: &mut Context<'_>,
			buf: &mut [u8],
		) -> Poll<io::Result<usize>> {
			if self.pos >= self.data.len() {
				return Poll::Pending;
			}
			let n = buf.len().min(self.data.len() - self.pos);
			buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
			self.pos += n;
			Poll::Ready(Ok(n))
		}
	}

	async fn read_one(data: Vec<u8>) -> io::Result<Bytes> {
		let mut framed = LengthDelimited::new(futures::io::Cursor::new(data));
		framed
			.next()
			.await
			.transpose()?
			.ok_or_else(|| io::ErrorKind::UnexpectedEof.into())
	}

	#[tokio::test]
	async fn accepts_protocol_name_frame() {
		let payload = b"/noise\n";
		let mut data = encode_len(payload.len() as u16);
		data.extend_from_slice(payload);
		let frame = read_one(data).await.expect("protocol name frame");
		assert_eq!(&frame[..], payload);
	}

	#[tokio::test]
	async fn accepts_frame_at_negotiation_cap() {
		let payload = vec![b'x'; MAX_NEGOTIATION_FRAME as usize];
		let mut data = encode_len(MAX_NEGOTIATION_FRAME);
		data.extend_from_slice(&payload);
		let frame = read_one(data).await.expect("capped frame");
		assert_eq!(frame.len(), MAX_NEGOTIATION_FRAME as usize);
	}

	#[tokio::test]
	async fn rejects_declared_length_above_negotiation_cap() {
		let err = read_one(encode_len(MAX_NEGOTIATION_FRAME + 1))
			.await
			.expect_err("length above cap");
		assert_eq!(err.kind(), io::ErrorKind::InvalidData);
	}

	#[tokio::test]
	async fn rejects_legacy_max_frame_declaration() {
		// `\xff\x7f` is the unsigned-varint for 16383, the previous read budget.
		let err = read_one(vec![0xff, 0x7f]).await.expect_err("16 KiB declaration");
		assert_eq!(err.kind(), io::ErrorKind::InvalidData);
	}

	#[tokio::test]
	async fn declared_length_without_payload_does_not_precommit_read_buffer() {
		// A peer that declares a frame and sends none of it must not force a
		// resident buffer of that size. `BytesMut::resize` writes the fill
		// byte, so `len()` is the residency signal.
		let declared = 512u16;
		let framed = LengthDelimited::new(PrefixThenPending { data: encode_len(declared), pos: 0 });
		futures::pin_mut!(framed);

		let waker = futures::task::noop_waker();
		let mut cx = Context::from_waker(&waker);
		match framed.as_mut().poll_next(&mut cx) {
			Poll::Pending => {},
			other => panic!("prefix-only read should wait for payload, got {other:?}"),
		}

		assert_eq!(
			framed.as_ref().get_ref().read_buffer.len(),
			0,
			"declared length must not be committed until payload bytes arrive"
		);
	}

	#[tokio::test]
	async fn read_buffer_grows_only_with_received_payload() {
		let declared = 512u16;
		let payload = vec![b'y'; 17];
		let mut data = encode_len(declared);
		data.extend_from_slice(&payload);
		let framed = LengthDelimited::new(PrefixThenPending { data, pos: 0 });
		futures::pin_mut!(framed);

		let waker = futures::task::noop_waker();
		let mut cx = Context::from_waker(&waker);
		match framed.as_mut().poll_next(&mut cx) {
			Poll::Pending => {},
			other => panic!("partial payload should wait for the rest, got {other:?}"),
		}

		assert_eq!(framed.as_ref().get_ref().read_buffer.len(), payload.len());
	}
}
