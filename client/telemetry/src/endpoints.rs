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

use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

/// Error type for telemetry endpoint parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EndpointError {
	/// Invalid URL format.
	#[error("Invalid URL: {0}")]
	InvalidUrl(String),
	/// URL scheme must be ws or wss.
	#[error("URL scheme must be ws:// or wss://, got: {0}")]
	InvalidScheme(String),
}

/// List of telemetry servers we want to talk to. Contains the URL of the server, and the
/// maximum verbosity level.
///
/// The URL string should be a WebSocket URL (ws:// or wss://).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TelemetryEndpoints(#[serde(deserialize_with = "url_deser")] pub(crate) Vec<(Url, u8)>);

/// Custom deserializer for TelemetryEndpoints.
fn url_deser<'de, D>(deserializer: D) -> Result<Vec<(Url, u8)>, D::Error>
where
	D: Deserializer<'de>,
{
	Vec::<(String, u8)>::deserialize(deserializer)?
		.iter()
		.map(|e| parse_telemetry_url(&e.0).map_err(serde::de::Error::custom).map(|u| (u, e.1)))
		.collect()
}

impl TelemetryEndpoints {
	/// Create a `TelemetryEndpoints` based on a list of `(String, u8)`.
	pub fn new(endpoints: Vec<(String, u8)>) -> Result<Self, EndpointError> {
		let endpoints: Result<Vec<(Url, u8)>, EndpointError> =
			endpoints.iter().map(|e| Ok((parse_telemetry_url(&e.0)?, e.1))).collect();
		endpoints.map(Self)
	}
}

impl TelemetryEndpoints {
	/// Return `true` if there are no telemetry endpoints, `false` otherwise.
	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}
}

/// Parses a WebSocket URL string into a `Url`.
/// Accepts ws://, wss://, or multiaddr-style addresses (converted to URLs).
fn parse_telemetry_url(url_str: &str) -> Result<Url, EndpointError> {
	// First try to parse as a regular URL
	if let Ok(url) = Url::parse(url_str) {
		match url.scheme() {
			"ws" | "wss" => return Ok(url),
			scheme => return Err(EndpointError::InvalidScheme(scheme.to_string())),
		}
	}

	// Try to parse as a multiaddr-style string (e.g., /dns/example.com/tcp/443/wss)
	// This provides backwards compatibility with existing configs
	if url_str.starts_with('/') {
		if let Some(url) = multiaddr_to_url(url_str) {
			return Ok(url);
		}
	}

	Err(EndpointError::InvalidUrl(url_str.to_string()))
}

/// Attempts to convert a multiaddr-style string to a WebSocket URL.
/// Supports formats like:
/// - /dns/example.com/tcp/443/wss
/// - /dns4/example.com/tcp/443/wss/p2p/...
/// - /ip4/127.0.0.1/tcp/8080/ws
fn multiaddr_to_url(addr: &str) -> Option<Url> {
	let parts: Vec<&str> = addr.split('/').filter(|s| !s.is_empty()).collect();

	let mut host = None;
	let mut port: Option<u16> = None;
	let mut secure = false;
	let mut path = String::new();

	let mut i = 0;
	while i < parts.len() {
		match parts[i] {
			"dns" | "dns4" | "dns6" =>
				if i + 1 < parts.len() {
					host = Some(parts[i + 1].to_string());
					i += 2;
				} else {
					return None;
				},
			"ip4" | "ip6" =>
				if i + 1 < parts.len() {
					host = Some(parts[i + 1].to_string());
					i += 2;
				} else {
					return None;
				},
			"tcp" =>
				if i + 1 < parts.len() {
					port = parts[i + 1].parse().ok();
					i += 2;
				} else {
					return None;
				},
			"wss" | "x-parity-wss" => {
				secure = true;
				i += 1;
			},
			"ws" | "x-parity-ws" => {
				secure = false;
				i += 1;
			},
			"p2p" => {
				// Skip p2p peer ID - not needed for telemetry
				i += 2;
			},
			other => {
				// Might be a path component after ws/wss
				if host.is_some() && (secure || port.is_some()) {
					path.push('/');
					path.push_str(other);
				}
				i += 1;
			},
		}
	}

	let host = host?;
	let scheme = if secure { "wss" } else { "ws" };

	// IPv6 addresses need brackets in URLs
	let host_for_url = if host.contains(':') { format!("[{}]", host) } else { host };

	let url_str = if let Some(p) = port {
		if path.is_empty() {
			format!("{}://{}:{}/", scheme, host_for_url, p)
		} else {
			format!("{}://{}:{}{}", scheme, host_for_url, p, path)
		}
	} else {
		let default_port = if secure { 443 } else { 80 };
		if path.is_empty() {
			format!("{}://{}:{}/", scheme, host_for_url, default_port)
		} else {
			format!("{}://{}:{}{}", scheme, host_for_url, default_port, path)
		}
	};

	Url::parse(&url_str).ok()
}

#[cfg(test)]
mod tests {
	use super::{parse_telemetry_url, TelemetryEndpoints, Url};

	#[test]
	fn valid_wss_url() {
		let url = parse_telemetry_url("wss://telemetry.polkadot.io/submit/")
			.expect("Should parse valid wss URL");
		assert_eq!(url.scheme(), "wss");
		assert_eq!(url.host_str(), Some("telemetry.polkadot.io"));
	}

	#[test]
	fn valid_ws_url() {
		let url =
			parse_telemetry_url("ws://localhost:8080/submit").expect("Should parse valid ws URL");
		assert_eq!(url.scheme(), "ws");
		assert_eq!(url.host_str(), Some("localhost"));
		assert_eq!(url.port(), Some(8080));
	}

	#[test]
	fn multiaddr_dns_wss() {
		let url = parse_telemetry_url("/dns/telemetry.polkadot.io/tcp/443/wss")
			.expect("Should parse multiaddr");
		assert_eq!(url.scheme(), "wss");
		assert_eq!(url.host_str(), Some("telemetry.polkadot.io"));
	}

	#[test]
	fn multiaddr_ip4_ws() {
		let url =
			parse_telemetry_url("/ip4/127.0.0.1/tcp/8080/ws").expect("Should parse multiaddr");
		assert_eq!(url.scheme(), "ws");
		assert_eq!(url.host_str(), Some("127.0.0.1"));
		assert_eq!(url.port(), Some(8080));
	}

	#[test]
	fn multiaddr_ip6_ws() {
		let url = parse_telemetry_url("/ip6/::1/tcp/8080/ws").expect("Should parse IPv6 multiaddr");
		assert_eq!(url.scheme(), "ws");
		assert_eq!(url.host_str(), Some("[::1]"));
		assert_eq!(url.port(), Some(8080));
	}

	#[test]
	fn invalid_scheme() {
		let result = parse_telemetry_url("http://example.com");
		assert!(result.is_err());
	}

	#[test]
	fn invalid_url() {
		let result = parse_telemetry_url("not a valid url");
		assert!(result.is_err());
	}

	#[test]
	fn valid_endpoints() {
		let endp = vec![
			("wss://telemetry.polkadot.io/submit/".into(), 3),
			("ws://localhost:8080".into(), 4),
		];
		let telem =
			TelemetryEndpoints::new(endp.clone()).expect("Telemetry endpoint should be valid");
		assert_eq!(telem.0.len(), 2);
	}

	#[test]
	fn invalid_endpoints() {
		let endp = vec![("http://example.com".into(), 3)];
		let telem = TelemetryEndpoints::new(endp);
		assert!(telem.is_err());
	}
}
