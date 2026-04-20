use sc_cli::RunCmd;

#[derive(Debug, clap::Parser)]
#[command(arg_required_else_help = true)]
pub struct Cli {
	#[command(subcommand)]
	pub subcommand: Option<Subcommand>,

	#[clap(flatten)]
	pub run: RunCmd,

	/// Inner hash for mining rewards (0x-prefixed, 32-byte hex from wormhole key generation)
	#[arg(long, value_name = "INNER_HASH")]
	pub rewards_inner_hash: Option<String>,

	/// Port to listen for external miner connections (e.g., 9833).
	/// When set, the node will wait for miners to connect instead of mining locally.
	#[arg(long, value_name = "PORT")]
	pub miner_listen_port: Option<u16>,

	/// Enable peer sharing via RPC endpoint
	#[arg(long)]
	pub enable_peer_sharing: bool,

	/// Sync: maximum timeouts before dropping a peer during major sync.
	#[arg(long, default_value_t = 20)]
	pub sync_max_timeouts_before_drop: u32,

	/// Sync: disable gating peer drops during major sync (fast-ban even in major sync).
	#[arg(long, default_value_t = false)]
	pub sync_disable_major_sync_gating: bool,

	/// Sync: block request timeout in seconds (default: 30).
	#[arg(long, default_value_t = 30)]
	pub sync_block_request_timeout: u64,
}

#[derive(Debug, clap::Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Subcommand {
	/// Key management cli utilities
	#[command(subcommand)]
	Key(QuantusKeySubcommand),

	/// Build a chain specification.
	BuildSpec(sc_cli::BuildSpecCmd),

	/// Validate blocks.
	CheckBlock(sc_cli::CheckBlockCmd),

	/// Export blocks.
	ExportBlocks(sc_cli::ExportBlocksCmd),

	/// Export the state of a given block into a chain spec.
	ExportState(sc_cli::ExportStateCmd),

	/// Import blocks.
	ImportBlocks(sc_cli::ImportBlocksCmd),

	/// Remove the whole chain.
	PurgeChain(sc_cli::PurgeChainCmd),

	/// Revert the chain to a previous state.
	Revert(sc_cli::RevertCmd),

	/// Sub-commands concerned with benchmarking.
	#[cfg(feature = "runtime-benchmarks")]
	#[command(subcommand)]
	Benchmark(frame_benchmarking_cli::BenchmarkCmd),

	/// Db meta columns information.
	ChainInfo(sc_cli::ChainInfoCmd),
}
#[derive(Debug, clap::Subcommand)]
pub enum QuantusKeySubcommand {
	/// Standard key commands from sc_cli
	#[command(flatten)]
	Sc(sc_cli::KeySubcommand),
	/// Generate a quantus address
	Quantus {
		/// Type of the key
		#[arg(long, value_name = "SCHEME", value_enum, default_value_t = QuantusAddressType::Standard, ignore_case = true)]
		scheme: QuantusAddressType,

		/// Optional: Provide a 64-character hex string to be used as a 32-byte seed.
		/// This is mutually exclusive with --words.
		#[arg(long, value_name = "SEED", conflicts_with = "words")]
		seed: Option<String>,

		/// Optional: Provide a BIP39 phrase (e.g., "word1 word2 ... word24").
		/// This is mutually exclusive with --seed.
		#[arg(long, value_name = "WORDS_PHRASE", conflicts_with = "seed")]
		words: Option<String>,

		/// Optional: HD wallet derivation index (default 0). Ignored if --no-derivation is set.
		#[arg(long, value_name = "INDEX", default_value_t = 0u32)]
		wallet_index: u32,

		/// Disable HD derivation. Generates the same result as current behavior.
		#[arg(long, default_value_t = false)]
		no_derivation: bool,

		/// Print sensitive key material (seed, public key, secret key). Useful for debugging.
		#[arg(long, short = 'v', default_value_t = false)]
		verbose: bool,
	},
}

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum QuantusAddressType {
	Wormhole,
	Standard,
}
