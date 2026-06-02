// Copyright 2019 Parity Technologies (UK) Ltd.
// Copyright 2023 litep2p developers
// Copyright 2025 Quantus Network developers
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

//! Noise handshake and transport implementations using pqXX pattern with ML-KEM 768.
//!
//! This module implements the Noise protocol using Clatter with the pqXX handshake pattern
//! and ML-KEM 768 (FIPS 203) for post-quantum key encapsulation. This provides ~192-bit
//! security against quantum attacks.
//!
//! ## Handshake Flow (pqXX - 4 messages)
//!
//! 1. Initiator -> Responder: `e` (ephemeral KEM public key)
//! 2. Responder -> Initiator: `ekem, e, es` + identity payload
//! 3. Initiator -> Responder: `skem, s, se` + identity payload
//! 4. Responder -> Initiator: `sks` (final KEM, empty payload)

use crate::{
	config::Role,
	crypto::{dilithium::Keypair, PublicKey, RemotePublicKey},
	error::{NegotiationError, ParseError},
	PeerId,
};

use bytes::{Buf, Bytes, BytesMut};
use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use prost::Message;

use std::{
	fmt, io,
	pin::Pin,
	task::{Context, Poll},
};

mod protocol;

use protocol::{ClatterSession, ClatterTransport};

mod handshake_schema {
	include!(concat!(env!("OUT_DIR"), "/noise.rs"));
}

/// Prefix of static key signatures for domain separation.
pub(crate) const STATIC_KEY_DOMAIN: &str = "noise-libp2p-static-key:";

/// Maximum Noise message size.
const MAX_NOISE_MSG_LEN: usize = u16::MAX as usize;

/// Space given to the encryption buffer to hold key material.
const NOISE_EXTRA_ENCRYPT_SPACE: usize = 16;

/// Max read ahead factor for the noise socket.
///
/// Specifies how many multiples of `MAX_NOISE_MESSAGE_LEN` are read from the socket
/// using one call to `poll_read()`.
pub(crate) const MAX_READ_AHEAD_FACTOR: usize = 5;

/// Maximum write buffer size.
pub(crate) const MAX_WRITE_BUFFER_SIZE: usize = 2;

/// Max. length for Noise protocol message payloads.
pub const MAX_FRAME_LEN: usize = MAX_NOISE_MSG_LEN - NOISE_EXTRA_ENCRYPT_SPACE;

/// Logging target for the file.
const LOG_TARGET: &str = "litep2p::crypto::noise";

/// Buffer size for ML-KEM 768 handshake messages.
/// - ML-KEM 768 public key: 1184 bytes
/// - ML-KEM 768 ciphertext: 1088 bytes
/// - Dilithium identity payload: ~7230 bytes
/// - Noise overhead: ~64 bytes
const HANDSHAKE_BUFFER_SIZE: usize = 16384;

#[derive(Debug)]
enum NoiseState {
	Handshake(ClatterSession),
	Transport(ClatterTransport),
}

pub struct NoiseContext {
	/// ML-KEM 768 keypair for the Noise static key
	kem_keypair: protocol::Keypair,
	/// Clatter session/transport state
	noise: NoiseState,
	/// Role (dialer/listener)
	role: Role,
	/// Identity payload (Dilithium public key + signature over KEM public key)
	pub payload: Vec<u8>,
}

impl fmt::Debug for NoiseContext {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("NoiseContext")
			.field("noise", &self.noise)
			.field("payload", &self.payload)
			.field("role", &self.role)
			.finish()
	}
}

impl NoiseContext {
	/// Assemble Noise payload and return [`NoiseContext`].
	fn assemble(
		session: ClatterSession,
		kem_keypair: protocol::Keypair,
		id_keys: &Keypair,
		role: Role,
	) -> Result<Self, NegotiationError> {
		// Sign the ML-KEM public key with the Dilithium identity key
		let signature = id_keys
			.sign(&[STATIC_KEY_DOMAIN.as_bytes(), kem_keypair.public().as_ref()].concat())
			.map_err(|e| NegotiationError::SigningFailed(e.to_string()))?;

		let noise_payload = handshake_schema::NoiseHandshakePayload {
			identity_key: Some(PublicKey::from(id_keys.public()).to_protobuf_encoding()),
			identity_sig: Some(signature),
			..Default::default()
		};

		let mut payload = Vec::with_capacity(noise_payload.encoded_len());
		noise_payload.encode(&mut payload).map_err(ParseError::from)?;

		Ok(Self { noise: NoiseState::Handshake(session), kem_keypair, payload, role })
	}

	/// Create a new NoiseContext for the pqXX handshake.
	pub fn new(keypair: &Keypair, role: Role) -> Result<Self, NegotiationError> {
		tracing::trace!(target: LOG_TARGET, ?role, "create new noise configuration (pqXX + ML-KEM 768)");

		// Generate ML-KEM 768 keypair for Noise static key
		let kem_keypair = protocol::Keypair::new();

		let is_initiator = matches!(role, Role::Dialer);
		let session = ClatterSession::new(&[], is_initiator, &kem_keypair)?;

		Self::assemble(session, kem_keypair, keypair, role)
	}

	/// Get first message (pqXX message 1: -> e).
	///
	/// For initiator: sends ephemeral KEM public key
	/// For listener: sends message 2 (identity payload)
	pub fn first_message(&mut self, role: Role) -> Result<Vec<u8>, NegotiationError> {
		match role {
			Role::Dialer => {
				tracing::trace!(target: LOG_TARGET, "get noise dialer first message (-> e)");

				let NoiseState::Handshake(ref mut session) = self.noise else {
					tracing::error!(target: LOG_TARGET, "invalid state to write the first handshake message");
					debug_assert!(false);
					return Err(NegotiationError::StateMismatch);
				};

				// pqXX message 1: -> e (ephemeral KEM public key, ~1184 bytes)
				let mut buffer = vec![0u8; HANDSHAKE_BUFFER_SIZE];
				let nwritten = session.write_message(&[], &mut buffer)?;
				buffer.truncate(nwritten);

				let size = nwritten as u16;
				let mut size = size.to_be_bytes().to_vec();
				size.append(&mut buffer);

				Ok(size)
			},
			Role::Listener => self.second_message(),
		}
	}

	/// Get second message (pqXX message 2 or 3 depending on role).
	///
	/// Contains the identity payload (Dilithium public key + signature).
	pub fn second_message(&mut self) -> Result<Vec<u8>, NegotiationError> {
		tracing::trace!(target: LOG_TARGET, role = ?self.role, "get noise payload message");

		let NoiseState::Handshake(ref mut session) = self.noise else {
			tracing::error!(target: LOG_TARGET, "invalid state to write handshake message");
			debug_assert!(false);
			return Err(NegotiationError::StateMismatch);
		};

		// pqXX message 2 or 3 with identity payload
		// Buffer needs space for:
		// - ML-KEM ciphertext: 1088 bytes
		// - ML-KEM public key: 1184 bytes
		// - Dilithium identity: ~7230 bytes
		// - Encryption overhead
		let mut buffer = vec![0u8; HANDSHAKE_BUFFER_SIZE];
		let nwritten = session.write_message(&self.payload, &mut buffer)?;
		buffer.truncate(nwritten);

		let size = nwritten as u16;
		let mut size = size.to_be_bytes().to_vec();
		size.append(&mut buffer);

		Ok(size)
	}

	/// Get final KEM message (pqXX message 4: <- sks).
	///
	/// Only sent by responder to complete the handshake.
	pub fn final_kem_message(&mut self) -> Result<Vec<u8>, NegotiationError> {
		tracing::trace!(target: LOG_TARGET, "get noise final KEM message (<- sks)");

		let NoiseState::Handshake(ref mut session) = self.noise else {
			tracing::error!(target: LOG_TARGET, "invalid state to write final KEM message");
			debug_assert!(false);
			return Err(NegotiationError::StateMismatch);
		};

		// pqXX message 4: <- sks (KEM ciphertext, empty payload)
		let mut buffer = vec![0u8; HANDSHAKE_BUFFER_SIZE];
		let nwritten = session.write_message(&[], &mut buffer)?;
		buffer.truncate(nwritten);

		let size = nwritten as u16;
		let mut size = size.to_be_bytes().to_vec();
		size.append(&mut buffer);

		Ok(size)
	}

	/// Read handshake message from the wire.
	async fn read_handshake_message<T: AsyncRead + AsyncWrite + Unpin>(
		&mut self,
		io: &mut T,
	) -> Result<Bytes, NegotiationError> {
		let mut size = BytesMut::zeroed(2);
		io.read_exact(&mut size).await?;
		let size = size.get_u16();

		let mut message = BytesMut::zeroed(size as usize);
		io.read_exact(&mut message).await?;

		let mut out = BytesMut::new();
		out.resize(message.len() + HANDSHAKE_BUFFER_SIZE, 0u8);

		let NoiseState::Handshake(ref mut session) = self.noise else {
			tracing::error!(target: LOG_TARGET, "invalid state to read handshake message");
			debug_assert!(false);
			return Err(NegotiationError::StateMismatch);
		};

		let nread = session.read_message(&message, &mut out)?;
		out.truncate(nread);

		Ok(out.freeze())
	}

	/// Read a message (works in both handshake and transport mode).
	fn read_message(&mut self, message: &[u8], out: &mut [u8]) -> Result<usize, NegotiationError> {
		match &mut self.noise {
			NoiseState::Handshake(session) => session.read_message(message, out),
			NoiseState::Transport(transport) => transport.read_message(message, out),
		}
	}

	/// Write a message (works in both handshake and transport mode).
	fn write_message(&mut self, message: &[u8], out: &mut [u8]) -> Result<usize, NegotiationError> {
		match &mut self.noise {
			NoiseState::Handshake(session) => session.write_message(message, out),
			NoiseState::Transport(transport) => transport.write_message(message, out),
		}
	}

	/// Get the remote's static KEM public key.
	fn get_remote_static(&self) -> Result<Vec<u8>, NegotiationError> {
		let NoiseState::Handshake(ref session) = self.noise else {
			tracing::error!(target: LOG_TARGET, "invalid state to get remote public key");
			return Err(NegotiationError::StateMismatch);
		};

		session.get_remote_static().ok_or_else(|| {
			tracing::error!(target: LOG_TARGET, "expected remote public key at the end of pqXX session");
			NegotiationError::IoError(std::io::ErrorKind::InvalidData)
		})
	}

	/// Convert Noise into transport mode.
	fn into_transport(self) -> Result<NoiseContext, NegotiationError> {
		let transport = match self.noise {
			NoiseState::Handshake(session) => session.into_transport_mode()?,
			NoiseState::Transport(_) => return Err(NegotiationError::StateMismatch),
		};

		Ok(NoiseContext {
			kem_keypair: self.kem_keypair,
			payload: self.payload,
			role: self.role,
			noise: NoiseState::Transport(transport),
		})
	}
}

enum ReadState {
	ReadData {
		max_read: usize,
	},
	ReadFrameLen,
	ProcessNextFrame {
		pending: Option<Vec<u8>>,
		offset: usize,
		size: usize,
		frame_size: usize,
		decrypted: bool,
	},
}

enum WriteState {
	/// No pending encrypted data, ready to accept new writes
	Idle,
	/// Writing encrypted data to socket
	Writing {
		/// Offset into encrypt_buffer that's been written to socket
		offset: usize,
		/// Total length of encrypted data in encrypt_buffer
		encrypted_len: usize,
	},
}

pub struct NoiseSocket<S: AsyncRead + AsyncWrite + Unpin> {
	io: S,
	noise: NoiseContext,
	current_frame_size: Option<usize>,
	write_state: WriteState,
	encrypt_buffer: Vec<u8>,
	offset: usize,
	nread: usize,
	read_state: ReadState,
	read_buffer: Vec<u8>,
	canonical_max_read: usize,
	decrypt_buffer: Option<Vec<u8>>,
	peer: PeerId,
	ty: HandshakeTransport,
}

impl<S: AsyncRead + AsyncWrite + Unpin> NoiseSocket<S> {
	fn new(
		io: S,
		noise: NoiseContext,
		max_read_ahead_factor: usize,
		max_write_buffer_size: usize,
		peer: PeerId,
		ty: HandshakeTransport,
	) -> Self {
		Self {
			io,
			noise,
			read_buffer: vec![
				0u8;
				max_read_ahead_factor * MAX_NOISE_MSG_LEN + (2 + MAX_NOISE_MSG_LEN)
			],
			nread: 0usize,
			offset: 0usize,
			current_frame_size: None,
			write_state: WriteState::Idle,
			encrypt_buffer: vec![0u8; max_write_buffer_size * (MAX_NOISE_MSG_LEN + 2)],
			decrypt_buffer: Some(vec![0u8; MAX_FRAME_LEN]),
			read_state: ReadState::ReadData { max_read: max_read_ahead_factor * MAX_NOISE_MSG_LEN },
			canonical_max_read: max_read_ahead_factor * MAX_NOISE_MSG_LEN,
			peer,
			ty,
		}
	}

	fn compact_read_buffer(&mut self, remaining: usize) {
		if remaining > 0 && self.offset != 0 {
			self.read_buffer.copy_within(self.offset..self.nread, 0);
		}

		self.nread = remaining;
		self.offset = 0;
	}

	fn read_more(&mut self) {
		self.read_state = ReadState::ReadData {
			max_read: std::cmp::min(self.read_buffer.len(), self.nread + self.canonical_max_read),
		};
	}

	fn reset_read_state(&mut self, remaining: usize) {
		self.compact_read_buffer(remaining);

		self.current_frame_size = None;
		self.read_more();
	}
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for NoiseSocket<S> {
	fn poll_read(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &mut [u8],
	) -> Poll<io::Result<usize>> {
		let this = Pin::into_inner(self);

		if buf.is_empty() {
			return Poll::Ready(Ok(0));
		}

		loop {
			match this.read_state {
				ReadState::ReadData { max_read } => {
					let nread = match Pin::new(&mut this.io)
						.poll_read(cx, &mut this.read_buffer[this.nread..max_read])
					{
						Poll::Pending => return Poll::Pending,
						Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
						Poll::Ready(Ok(nread)) => match nread == 0 {
							true => return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into())),
							false => nread,
						},
					};

					tracing::trace!(
						target: LOG_TARGET,
						?nread,
						peer = ?this.peer,
						transport = ?this.ty,
						"read encrypted bytes",
					);

					this.nread += nread;
					// Check if we were waiting for more data for an existing frame
					if let Some(frame_size) = this.current_frame_size {
						// Check if we have enough data now
						let remaining = this.nread - this.offset;
						if remaining >= frame_size {
							this.read_state = ReadState::ProcessNextFrame {
								pending: this.decrypt_buffer.take(),
								offset: 0usize,
								size: 0usize,
								frame_size,
								decrypted: false,
							};
						}
						// else stay in ReadData to get more
					} else {
						this.read_state = ReadState::ReadFrameLen;
					}
				},
				ReadState::ReadFrameLen => {
					// try to read the frame length
					let remaining = this.nread - this.offset;

					if remaining < 2 {
						this.reset_read_state(remaining);
						continue;
					}

					let frame_len = u16::from_be_bytes([
						this.read_buffer[this.offset],
						this.read_buffer[this.offset + 1],
					]) as usize;

					// consume the frame length
					this.offset += 2;

					// set the frame size and switch to processing state
					this.current_frame_size = Some(frame_len);
					this.read_state = ReadState::ProcessNextFrame {
						pending: this.decrypt_buffer.take(),
						offset: 0usize,
						size: 0usize,
						frame_size: frame_len,
						decrypted: false,
					};
				},
				ReadState::ProcessNextFrame {
					ref mut pending,
					ref mut offset,
					ref mut size,
					frame_size,
					ref mut decrypted,
				} => {
					// Decrypt only once. If the caller did not consume all plaintext in the
					// previous poll, serve the pending plaintext before reading more ciphertext.
					if !*decrypted {
						let remaining = this.nread - this.offset;

						// need to read more bytes to complete the frame
						if remaining < frame_size {
							// Put pending buffer back before switching states
							if let Some(buf) = pending.take() {
								this.decrypt_buffer = Some(buf);
							}
							this.compact_read_buffer(remaining);
							this.current_frame_size = Some(frame_size);
							this.read_more();
							continue;
						}

						let read_end = this.offset + frame_size;
						let pending = pending.as_mut().expect("to have a buffer");

						let ciphertext = &this.read_buffer[this.offset..read_end];
						tracing::trace!(
							target: LOG_TARGET,
							frame_size = ?frame_size,
							ciphertext_len = ciphertext.len(),
							first_bytes = ?&ciphertext[..std::cmp::min(32, ciphertext.len())],
							peer = ?this.peer,
							transport = ?this.ty,
							"attempting to decrypt frame"
						);

						match this.noise.read_message(ciphertext, pending) {
							Ok(nread) => {
								tracing::trace!(
									target: LOG_TARGET,
									?nread,
									?frame_size,
									peer = ?this.peer,
									transport = ?this.ty,
									"decrypted bytes"
								);

								this.offset += frame_size;
								*size = nread;
								*decrypted = true;
							},
							Err(error) => {
								tracing::error!(
									target: LOG_TARGET,
									?error,
									?frame_size,
									ciphertext_len = ciphertext.len(),
									first_bytes = ?&ciphertext[..std::cmp::min(32, ciphertext.len())],
									peer = ?this.peer,
									transport = ?this.ty,
									"failed to decrypt"
								);
								return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
							},
						}
					}

					// pending buffer already decrypted,
					// copy as much as possible to user's buffer
					let pending_ref = pending.as_ref().expect("to have a buffer");
					let to_copy = std::cmp::min(*size - *offset, buf.len());
					buf[..to_copy].copy_from_slice(&pending_ref[*offset..*offset + to_copy]);
					*offset += to_copy;

					// if pending buffer was exhausted,
					// process next frame if there is one
					if *offset == *size {
						// Clear current frame size since we're done with this frame
						this.current_frame_size = None;

						// Put the decrypt buffer back before transitioning
						// Note: pending is &mut Option<Vec<u8>> from the match
						this.decrypt_buffer = pending.take();

						let remaining = this.nread - this.offset;

						match remaining {
							// all read bytes have been consumed, need to read more data
							0 | 1 => {
								this.reset_read_state(remaining);
							},
							// at least two bytes have been read,
							// check if there's another full frame ready to be parsed
							_ => this.read_state = ReadState::ReadFrameLen,
						}

						if to_copy == 0 {
							continue;
						}
					}

					return Poll::Ready(Ok(to_copy));
				},
			}
		}
	}
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for NoiseSocket<S> {
	fn poll_write(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &[u8],
	) -> Poll<io::Result<usize>> {
		let this = Pin::into_inner(self);

		// Step 1: Try to drain any pending encrypted data first
		let mut buffer_offset = 0usize;
		if let WriteState::Writing { offset, encrypted_len } = &mut this.write_state {
			loop {
				match futures::ready!(Pin::new(&mut this.io)
					.poll_write(cx, &this.encrypt_buffer[*offset..*encrypted_len]))
				{
					Ok(0) => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
					Ok(n) => {
						*offset += n;
						if offset == encrypted_len {
							// All pending data sent, reset to idle
							this.write_state = WriteState::Idle;
							break;
						}
					},
					Err(e) => return Poll::Ready(Err(e)),
				}
			}
		}

		// Step 2: Buffer has been drained (or was empty).
		// Encrypt new data into the buffer.
		if buf.is_empty() {
			return Poll::Ready(Ok(0));
		}

		let mut total_plaintext = 0usize;
		// Encrypt as many chunks as fit in the remaining space
		for chunk in buf.chunks(MAX_FRAME_LEN) {
			// Check space for this specific chunk + overhead
			// Note: overhead is 2 bytes length + 16 bytes auth tag
			let overhead = 2 + NOISE_EXTRA_ENCRYPT_SPACE;
			if buffer_offset + chunk.len() + overhead > this.encrypt_buffer.len() {
				// Buffer is full, stop packing
				break;
			}

			match this.noise.write_message(chunk, &mut this.encrypt_buffer[buffer_offset + 2..]) {
				Ok(nwritten) => {
					// Write frame length prefix
					this.encrypt_buffer[buffer_offset] = (nwritten >> 8) as u8;
					this.encrypt_buffer[buffer_offset + 1] = (nwritten & 0xff) as u8;

					tracing::trace!(
						target: LOG_TARGET,
						plaintext_len = chunk.len(),
						ciphertext_len = nwritten,
						frame_len = nwritten,
						first_plaintext_bytes = ?&chunk[..std::cmp::min(32, chunk.len())],
						peer = ?this.peer,
						transport = ?this.ty,
						"encrypted frame"
					);

					buffer_offset += nwritten + 2;
					total_plaintext += chunk.len();
				},
				Err(error) => {
					tracing::error!(target: LOG_TARGET, ?error, "failed to encrypt");
					return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
				},
			}
		}
		if total_plaintext == 0 {
			// No data could be buffered because the buffer is full.
			return Poll::Pending;
		}

		// Step 3. Adjust state to writing and return number of bytes accepted.
		match this.write_state {
			WriteState::Idle => {
				this.write_state = WriteState::Writing { offset: 0, encrypted_len: buffer_offset };
			},
			WriteState::Writing { ref mut encrypted_len, .. } => {
				*encrypted_len = buffer_offset;
			},
		}

		Poll::Ready(Ok(total_plaintext))
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		let this = Pin::into_inner(self);

		// Flush internal buffer of encrypted messages
		if let WriteState::Writing { offset, encrypted_len } = &mut this.write_state {
			loop {
				match futures::ready!(Pin::new(&mut this.io)
					.poll_write(cx, &this.encrypt_buffer[*offset..*encrypted_len]))
				{
					Ok(0) => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
					Ok(n) => {
						*offset += n;
						if offset == encrypted_len {
							this.write_state = WriteState::Idle;
							break;
						}
					},
					Err(e) => return Poll::Ready(Err(e)),
				}
			}
		}

		// Flush underlying socket
		Pin::new(&mut this.io).poll_flush(cx)
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		// Ensure buffer is flushed before closing
		futures::ready!(self.as_mut().poll_flush(cx))?;

		Pin::new(&mut self.io).poll_close(cx)
	}
}

/// Parse the `PeerId` from received `NoiseHandshakePayload` and verify the payload signature.
fn parse_and_verify_peer_id(
	payload: handshake_schema::NoiseHandshakePayload,
	kem_remote_pubkey: &[u8],
) -> Result<PeerId, NegotiationError> {
	let identity = payload.identity_key.ok_or(NegotiationError::PeerIdMissing)?;
	let remote_public_key = RemotePublicKey::from_protobuf_encoding(&identity)?;
	let remote_key_signature =
		payload.identity_sig.ok_or(NegotiationError::BadSignature).inspect_err(|_err| {
			tracing::debug!(target: LOG_TARGET, "payload without signature");
		})?;

	let peer_id = PeerId::from_public_key_protobuf(&identity);

	if !remote_public_key
		.verify(&[STATIC_KEY_DOMAIN.as_bytes(), kem_remote_pubkey].concat(), &remote_key_signature)
	{
		tracing::debug!(
			target: LOG_TARGET,
			?peer_id,
			"failed to verify remote public key signature"
		);

		return Err(NegotiationError::BadSignature);
	}

	Ok(peer_id)
}

/// The type of the transport used for the crypto/noise protocol.
///
/// This is used for logging purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeTransport {
	Tcp,
	#[cfg(feature = "websocket")]
	WebSocket,
}

/// Perform Noise handshake using pqXX pattern (4 messages).
pub async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(
	mut io: S,
	keypair: &Keypair,
	role: Role,
	max_read_ahead_factor: usize,
	max_write_buffer_size: usize,
	timeout: std::time::Duration,
	ty: HandshakeTransport,
) -> Result<(NoiseSocket<S>, PeerId), NegotiationError> {
	let handle_handshake = async move {
		tracing::debug!(target: LOG_TARGET, ?role, ?ty, "start noise handshake (pqXX + ML-KEM 768)");

		let mut noise = NoiseContext::new(keypair, role)?;
		let payload = match role {
			Role::Dialer => {
				// pqXX Message 1: -> e (ephemeral KEM public key)
				tracing::debug!(target: LOG_TARGET, "pqXX dialer: sending message 1 (-> e)");
				let first_message = noise.first_message(Role::Dialer)?;
				tracing::debug!(target: LOG_TARGET, len = first_message.len(), "pqXX dialer: message 1 size");
				io.write_all(&first_message).await?;
				io.flush().await?;
				tracing::debug!(target: LOG_TARGET, "pqXX dialer: message 1 sent, waiting for message 2");

				// pqXX Message 2: <- ekem, e, es + identity payload
				let message = noise.read_handshake_message(&mut io).await?;
				tracing::debug!(target: LOG_TARGET, len = message.len(), "pqXX dialer: received message 2");
				let payload = handshake_schema::NoiseHandshakePayload::decode(message)
					.map_err(ParseError::from)
					.map_err(|err| {
						tracing::error!(target: LOG_TARGET, ?err, ?ty, "failed to decode remote identity message");
						err
					})?;
				tracing::debug!(target: LOG_TARGET, "pqXX dialer: message 2 decoded successfully");

				// pqXX Message 3: -> skem, s, se + local identity payload
				tracing::debug!(target: LOG_TARGET, "pqXX dialer: sending message 3 (-> skem, s, se)");
				let third_message = noise.second_message()?;
				tracing::debug!(target: LOG_TARGET, len = third_message.len(), "pqXX dialer: message 3 size");
				io.write_all(&third_message).await?;
				io.flush().await?;
				tracing::debug!(target: LOG_TARGET, "pqXX dialer: message 3 sent, waiting for message 4");

				// pqXX Message 4: <- sks (final KEM, empty payload)
				let _final_message = noise.read_handshake_message(&mut io).await?;
				tracing::debug!(target: LOG_TARGET, "pqXX dialer: received message 4, handshake complete");
				// Message 4 should be empty (or contain no identity payload)

				payload
			},
			Role::Listener => {
				// pqXX Message 1: <- e (remote's ephemeral KEM public key)
				tracing::debug!(target: LOG_TARGET, "pqXX listener: waiting for message 1");
				let _ = noise.read_handshake_message(&mut io).await?;
				tracing::debug!(target: LOG_TARGET, "pqXX listener: received message 1");

				// pqXX Message 2: -> ekem, e, es + local identity payload
				tracing::debug!(target: LOG_TARGET, "pqXX listener: sending message 2");
				let second_message = noise.second_message()?;
				io.write_all(&second_message).await?;
				io.flush().await?;
				tracing::debug!(target: LOG_TARGET, "pqXX listener: message 2 sent, waiting for message 3");

				// pqXX Message 3: <- skem, s, se + remote identity payload
				let message = noise.read_handshake_message(&mut io).await?;
				tracing::debug!(target: LOG_TARGET, len = message.len(), "pqXX listener: received message 3");
				let payload = handshake_schema::NoiseHandshakePayload::decode(message)
					.map_err(ParseError::from)?;
				tracing::debug!(target: LOG_TARGET, "pqXX listener: message 3 decoded successfully");

				// pqXX Message 4: -> sks (final KEM, empty payload)
				tracing::debug!(target: LOG_TARGET, "pqXX listener: sending message 4 (-> sks)");
				let final_message = noise.final_kem_message()?;
				io.write_all(&final_message).await?;
				io.flush().await?;
				tracing::debug!(target: LOG_TARGET, "pqXX listener: handshake complete");

				payload
			},
		};

		let kem_remote_pubkey = noise.get_remote_static()?;
		let peer = parse_and_verify_peer_id(payload, &kem_remote_pubkey)?;

		Ok((
			NoiseSocket::new(
				io,
				noise.into_transport()?,
				max_read_ahead_factor,
				max_write_buffer_size,
				peer,
				ty,
			),
			peer,
		))
	};

	match tokio::time::timeout(timeout, handle_handshake).await {
		Err(_) => Err(NegotiationError::Timeout),
		Ok(result) => result,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::net::SocketAddr;
	use tokio::net::{TcpListener, TcpStream};
	use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

	#[tokio::test]
	async fn noise_handshake() {
		let _ = tracing_subscriber::fmt()
			.with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
			.try_init();

		let keypair1 = Keypair::generate();
		let keypair2 = Keypair::generate();

		let peer1_id = PeerId::from_public_key(&keypair1.public().into());
		let peer2_id = PeerId::from_public_key(&keypair2.public().into());

		let listener = TcpListener::bind("[::1]:0".parse::<SocketAddr>().unwrap()).await.unwrap();

		let (stream1, stream2) =
			tokio::join!(TcpStream::connect(listener.local_addr().unwrap()), listener.accept());
		let (io1, io2) = {
			let io1 = TokioAsyncReadCompatExt::compat(stream1.unwrap()).into_inner();
			let io1 = Box::new(TokioAsyncWriteCompatExt::compat_write(io1));
			let io2 = TokioAsyncReadCompatExt::compat(stream2.unwrap().0).into_inner();
			let io2 = Box::new(TokioAsyncWriteCompatExt::compat_write(io2));

			(io1, io2)
		};

		let (res1, res2) = tokio::join!(
			handshake(
				io1,
				&keypair1,
				Role::Dialer,
				MAX_READ_AHEAD_FACTOR,
				MAX_WRITE_BUFFER_SIZE,
				std::time::Duration::from_secs(10),
				HandshakeTransport::Tcp,
			),
			handshake(
				io2,
				&keypair2,
				Role::Listener,
				MAX_READ_AHEAD_FACTOR,
				MAX_WRITE_BUFFER_SIZE,
				std::time::Duration::from_secs(10),
				HandshakeTransport::Tcp,
			)
		);
		let (mut res1, mut res2) = (res1.unwrap(), res2.unwrap());

		assert_eq!(res1.1, peer2_id);
		assert_eq!(res2.1, peer1_id);

		// verify the connection works by reading a string
		let mut buf = vec![0u8; 512];

		let sent = res1.0.write(b"hello, world").await.unwrap();
		res1.0.flush().await.unwrap();

		let received = res2.0.read(&mut buf).await.unwrap();
		assert_eq!(sent, 12);
		assert_eq!(received, 12);
		assert_eq!(&buf[..received], b"hello, world");
	}
}
