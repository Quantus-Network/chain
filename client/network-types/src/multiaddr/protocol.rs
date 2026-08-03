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

use crate::multihash::Multihash;
use litep2p::types::multiaddr::Protocol as LiteP2pProtocol;
use std::{
	borrow::Cow,
	fmt::{self, Debug, Display},
	net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

const LOG_TARGET: &str = "sub-libp2p";

/// Error converting a local [`Protocol`] into a litep2p/multiaddr protocol.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolConversionError {
	/// [`Protocol::P2p`] carried a multihash that is not a valid peer id
	/// (for example sha2-512, or an oversized identity digest).
	#[error("p2p protocol multihash is not a valid peer id")]
	InvalidPeerId,
}

/// [`Protocol`] describes all possible multiaddress protocols.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum Protocol<'a> {
	Dccp(u16),
	Dns(Cow<'a, str>),
	Dns4(Cow<'a, str>),
	Dns6(Cow<'a, str>),
	Dnsaddr(Cow<'a, str>),
	Http,
	Https,
	Ip4(Ipv4Addr),
	Ip6(Ipv6Addr),
	P2pWebRtcDirect,
	P2pWebRtcStar,
	WebRTC,
	WebRTCDirect,
	Certhash(Multihash),
	P2pWebSocketStar,
	/// Contains the "port" to contact. Similar to TCP or UDP, 0 means "assign me a port".
	Memory(u64),
	Onion(Cow<'a, [u8; 10]>, u16),
	Onion3(Cow<'a, [u8; 35]>, u16),
	P2p(Multihash),
	P2pCircuit,
	Quic,
	QuicV1,
	Sctp(u16),
	Tcp(u16),
	Tls,
	Noise,
	Udp(u16),
	Udt,
	Unix(Cow<'a, str>),
	Utp,
	WebTransport,
	Ws(Cow<'a, str>),
	Wss(Cow<'a, str>),
	Ip6zone(Cow<'a, str>),
	Ipcidr(u8),
	Garlic64(Cow<'a, [u8]>),
	Garlic32(Cow<'a, [u8]>),
	Sni(Cow<'a, str>),
	P2pStardust,
}

impl Display for Protocol<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match LiteP2pProtocol::try_from(self.clone()) {
			Ok(protocol) => Display::fmt(&protocol, f),
			Err(ProtocolConversionError::InvalidPeerId) => f.write_str("/p2p/<invalid-peer-id>"),
		}
	}
}

impl From<IpAddr> for Protocol<'_> {
	#[inline]
	fn from(addr: IpAddr) -> Self {
		match addr {
			IpAddr::V4(addr) => Protocol::Ip4(addr),
			IpAddr::V6(addr) => Protocol::Ip6(addr),
		}
	}
}

impl From<Ipv4Addr> for Protocol<'_> {
	#[inline]
	fn from(addr: Ipv4Addr) -> Self {
		Protocol::Ip4(addr)
	}
}

impl From<Ipv6Addr> for Protocol<'_> {
	#[inline]
	fn from(addr: Ipv6Addr) -> Self {
		Protocol::Ip6(addr)
	}
}

/// Fallible conversion from the litep2p/multiaddr protocol enum.
///
/// Used at parsing boundaries so addresses containing protocols we do not model are rejected
/// instead of being stored and later panicking on iteration. `multiaddr::Protocol` is
/// `#[non_exhaustive]`, so unknown future variants surface here as `Err`.
pub(super) fn try_from_litep2p_protocol<'a>(
	protocol: LiteP2pProtocol<'a>,
) -> Result<Protocol<'a>, &'static str> {
	Ok(match protocol {
		LiteP2pProtocol::Dccp(port) => Protocol::Dccp(port),
		LiteP2pProtocol::Dns(str) => Protocol::Dns(str),
		LiteP2pProtocol::Dns4(str) => Protocol::Dns4(str),
		LiteP2pProtocol::Dns6(str) => Protocol::Dns6(str),
		LiteP2pProtocol::Dnsaddr(str) => Protocol::Dnsaddr(str),
		LiteP2pProtocol::Http => Protocol::Http,
		LiteP2pProtocol::Https => Protocol::Https,
		LiteP2pProtocol::Ip4(ipv4_addr) => Protocol::Ip4(ipv4_addr),
		LiteP2pProtocol::Ip6(ipv6_addr) => Protocol::Ip6(ipv6_addr),
		LiteP2pProtocol::P2pWebRtcDirect => Protocol::P2pWebRtcDirect,
		LiteP2pProtocol::P2pWebRtcStar => Protocol::P2pWebRtcStar,
		LiteP2pProtocol::WebRTC => Protocol::WebRTC,
		LiteP2pProtocol::WebRTCDirect => Protocol::WebRTCDirect,
		LiteP2pProtocol::Certhash(multihash) => Protocol::Certhash(multihash.into()),
		LiteP2pProtocol::P2pWebSocketStar => Protocol::P2pWebSocketStar,
		LiteP2pProtocol::Memory(port) => Protocol::Memory(port),
		LiteP2pProtocol::Onion(str, port) => Protocol::Onion(str, port),
		LiteP2pProtocol::Onion3(addr) => Protocol::Onion3(Cow::Owned(*addr.hash()), addr.port()),
		LiteP2pProtocol::P2p(peer_id) => Protocol::P2p(
			Multihash::from_bytes(&peer_id.to_bytes()).expect("valid peer id multihash"),
		),
		LiteP2pProtocol::P2pCircuit => Protocol::P2pCircuit,
		LiteP2pProtocol::Quic => Protocol::Quic,
		LiteP2pProtocol::QuicV1 => Protocol::QuicV1,
		LiteP2pProtocol::Sctp(port) => Protocol::Sctp(port),
		LiteP2pProtocol::Tcp(port) => Protocol::Tcp(port),
		LiteP2pProtocol::Tls => Protocol::Tls,
		LiteP2pProtocol::Noise => Protocol::Noise,
		LiteP2pProtocol::Udp(port) => Protocol::Udp(port),
		LiteP2pProtocol::Udt => Protocol::Udt,
		LiteP2pProtocol::Unix(str) => Protocol::Unix(str),
		LiteP2pProtocol::Utp => Protocol::Utp,
		LiteP2pProtocol::WebTransport => Protocol::WebTransport,
		LiteP2pProtocol::Ws(str) => Protocol::Ws(str),
		LiteP2pProtocol::Wss(str) => Protocol::Wss(str),
		LiteP2pProtocol::Ip6zone(str) => Protocol::Ip6zone(str),
		LiteP2pProtocol::Ipcidr(mask) => Protocol::Ipcidr(mask),
		LiteP2pProtocol::Garlic64(addr) => Protocol::Garlic64(addr),
		LiteP2pProtocol::Garlic32(addr) => Protocol::Garlic32(addr),
		LiteP2pProtocol::Sni(str) => Protocol::Sni(str),
		LiteP2pProtocol::P2pStardust => Protocol::P2pStardust,
		other => return Err(other.tag()),
	})
}

impl<'a> From<LiteP2pProtocol<'a>> for Protocol<'a> {
	fn from(protocol: LiteP2pProtocol<'a>) -> Self {
		try_from_litep2p_protocol(protocol).unwrap_or_else(|tag| {
			// Never panic while iterating addresses that bypassed the fallible parse path
			// (e.g. wrapped litep2p multiaddrs). Lossy, but safe.
			log::error!(
				target: LOG_TARGET,
				"Got unsupported multiaddr protocol '{}'",
				tag,
			);
			Protocol::Dccp(0)
		})
	}
}

impl<'a> TryFrom<Protocol<'a>> for LiteP2pProtocol<'a> {
	type Error = ProtocolConversionError;

	fn try_from(protocol: Protocol<'a>) -> Result<Self, Self::Error> {
		Ok(match protocol {
			Protocol::Dccp(port) => LiteP2pProtocol::Dccp(port),
			Protocol::Dns(str) => LiteP2pProtocol::Dns(str),
			Protocol::Dns4(str) => LiteP2pProtocol::Dns4(str),
			Protocol::Dns6(str) => LiteP2pProtocol::Dns6(str),
			Protocol::Dnsaddr(str) => LiteP2pProtocol::Dnsaddr(str),
			Protocol::Http => LiteP2pProtocol::Http,
			Protocol::Https => LiteP2pProtocol::Https,
			Protocol::Ip4(ipv4_addr) => LiteP2pProtocol::Ip4(ipv4_addr),
			Protocol::Ip6(ipv6_addr) => LiteP2pProtocol::Ip6(ipv6_addr),
			Protocol::P2pWebRtcDirect => LiteP2pProtocol::P2pWebRtcDirect,
			Protocol::P2pWebRtcStar => LiteP2pProtocol::P2pWebRtcStar,
			Protocol::WebRTC => LiteP2pProtocol::WebRTC,
			Protocol::WebRTCDirect => LiteP2pProtocol::WebRTCDirect,
			Protocol::Certhash(multihash) => LiteP2pProtocol::Certhash(multihash.into()),
			Protocol::P2pWebSocketStar => LiteP2pProtocol::P2pWebSocketStar,
			Protocol::Memory(port) => LiteP2pProtocol::Memory(port),
			Protocol::Onion(str, port) => LiteP2pProtocol::Onion(str, port),
			Protocol::Onion3(str, port) => LiteP2pProtocol::Onion3((str.into_owned(), port).into()),
			Protocol::P2p(multihash) => {
				let litep2p_multihash: litep2p::types::multihash::Multihash = multihash.into();
				LiteP2pProtocol::P2p(
					multiaddr::PeerId::try_from(litep2p_multihash)
						.map_err(|_| ProtocolConversionError::InvalidPeerId)?,
				)
			},
			Protocol::P2pCircuit => LiteP2pProtocol::P2pCircuit,
			Protocol::Quic => LiteP2pProtocol::Quic,
			Protocol::QuicV1 => LiteP2pProtocol::QuicV1,
			Protocol::Sctp(port) => LiteP2pProtocol::Sctp(port),
			Protocol::Tcp(port) => LiteP2pProtocol::Tcp(port),
			Protocol::Tls => LiteP2pProtocol::Tls,
			Protocol::Noise => LiteP2pProtocol::Noise,
			Protocol::Udp(port) => LiteP2pProtocol::Udp(port),
			Protocol::Udt => LiteP2pProtocol::Udt,
			Protocol::Unix(str) => LiteP2pProtocol::Unix(str),
			Protocol::Utp => LiteP2pProtocol::Utp,
			Protocol::WebTransport => LiteP2pProtocol::WebTransport,
			Protocol::Ws(str) => LiteP2pProtocol::Ws(str),
			Protocol::Wss(str) => LiteP2pProtocol::Wss(str),
			Protocol::Ip6zone(str) => LiteP2pProtocol::Ip6zone(str),
			Protocol::Ipcidr(mask) => LiteP2pProtocol::Ipcidr(mask),
			Protocol::Garlic64(addr) => LiteP2pProtocol::Garlic64(addr),
			Protocol::Garlic32(addr) => LiteP2pProtocol::Garlic32(addr),
			Protocol::Sni(str) => LiteP2pProtocol::Sni(str),
			Protocol::P2pStardust => LiteP2pProtocol::P2pStardust,
		})
	}
}

/// Lossy conversion for legacy infallible call sites (`push` / `with`).
///
/// Prefer [`TryFrom`] — that is the real contract for [`Protocol::P2p`]. This helper never
/// panics: invalid peer-id multihashes are replaced with a placeholder and logged.
pub(super) fn into_litep2p_lossy<'a>(protocol: Protocol<'a>) -> LiteP2pProtocol<'a> {
	LiteP2pProtocol::try_from(protocol).unwrap_or_else(|err| {
		log::error!(
			target: LOG_TARGET,
			"Failed to convert Protocol to litep2p protocol: {err}; \
			 replacing invalid /p2p component with a placeholder peer id",
		);
		LiteP2pProtocol::P2p(placeholder_peer_id())
	})
}

/// Valid peer id used only as a non-panicking stand-in for a bad `/p2p` multihash.
fn placeholder_peer_id() -> multiaddr::PeerId {
	let multihash = litep2p::types::multihash::Multihash::wrap(0, &[0u8; 32])
		.expect("32-byte identity digest fits Multihash<64>");
	multiaddr::PeerId::try_from(multihash).expect("identity digests <= 42 bytes are valid peer ids")
}
