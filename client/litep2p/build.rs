fn main() {
	let mut config = prost_build::Config::new();
	// Configure Prost to add #[derive(Serialize, Deserialize)] to all generated structs
	config.type_attribute(
		".",
		"#[cfg_attr(feature = \"fuzz\", derive(serde::Serialize, serde::Deserialize))]",
	);
	if let Err(e) = config.compile_protos(
		&[
			"src/schema/keys.proto",
			"src/schema/noise.proto",
			"src/schema/webrtc.proto",
			"src/protocol/libp2p/schema/identify.proto",
			"src/protocol/libp2p/schema/kademlia.proto",
			"src/protocol/libp2p/schema/bitswap.proto",
		],
		&["src"],
	) {
		println!("cargo::error=Failed to compile protobuf schemas: {e}");
		std::process::exit(1);
	}
}
