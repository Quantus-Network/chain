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
	/// When set, the node waits for miners instead of mining locally.
	/// Requires `--validator`; startup fails otherwise.
	#[arg(long, value_name = "PORT")]
	pub miner_listen_port: Option<u16>,

	/// Path to the miner auth token file.
	///
	/// Requires `--miner-listen-port` (startup fails otherwise). Defaults to
	/// `<base-path>/chains/<chain>/miner-auth-token`. If the file does not exist,
	/// the node generates a random token and writes it there (mode 0600 on Unix).
	/// The token itself is never logged — read the file to configure miners.
	/// Startup fails if this path is empty/unreadable or cannot be created.
	#[arg(long, value_name = "PATH")]
	pub miner_auth_token_file: Option<std::path::PathBuf>,

	/// Enable peer sharing via RPC endpoint (`peer_getNetworkInfo`).
	///
	/// The endpoint is an unsafe RPC: it is served only to local connections, or to
	/// remote ones when the node also runs with `--rpc-methods unsafe`.
	#[arg(long)]
	pub enable_peer_sharing: bool,

	/// Sync: maximum request failures before dropping a peer that is ahead.
	#[arg(long, default_value_t = 20)]
	pub sync_max_timeouts_before_drop: u32,

	/// Sync: disable request-failure tolerance for peers that are ahead.
	#[arg(long, default_value_t = false)]
	pub sync_disable_major_sync_gating: bool,

	/// Maximum tip age in seconds before authoring pauses (default: 24 hours).
	///
	/// Until the node has observed its best block to be at most this old, it
	/// refuses to mine (initial-sync guard, like Bitcoin's -maxtipage).
	/// Bypassed entirely by --force-authoring.
	#[arg(long, value_name = "SECONDS", default_value_t = crate::service::DEFAULT_MAX_TIP_AGE_SECS)]
	pub max_tip_age: u64,

	/// Sync: block request timeout in seconds (default: 30).
	#[arg(long, default_value_t = 30)]
	pub sync_block_request_timeout: u64,

	/// Opt in to public-batch aggregation of wormhole exits.
	///
	/// Private-batch proofs offered to this node's transaction pool (gossip and
	/// RPC) are verified, pooled, and folded into public-batch proofs that this
	/// node submits instead; the on-chain aggregator rebate is paid to
	/// `--public-batch-aggregator-address`. Building the public-batch circuit
	/// takes tens of seconds and several GB of memory at startup, and each
	/// batch is tens of seconds of proving. Requires
	/// `--public-batch-aggregator-address`.
	#[arg(long)]
	pub public_batch_aggregator: bool,

	/// SS58 account credited with the public-batch aggregator rebate.
	#[arg(long, value_name = "SS58", requires = "public_batch_aggregator")]
	pub public_batch_aggregator_address: Option<String>,

	/// Prove a partial public batch once it holds at least this many private
	/// batches and the oldest has waited `--public-batch-max-wait`.
	#[arg(long, value_name = "N", default_value_t = 1, requires = "public_batch_aggregator")]
	pub public_batch_min_proofs: usize,

	/// Seconds the oldest proof of a partial bucket waits before it is proved.
	/// Full buckets are proved immediately.
	#[arg(
		long,
		value_name = "SECONDS",
		default_value_t = 60,
		requires = "public_batch_aggregator"
	)]
	pub public_batch_max_wait: u64,

	/// Seconds to leave a bucket alone after proving it, so the submitted batch
	/// can settle before the bucket is considered again.
	#[arg(
		long,
		value_name = "SECONDS",
		default_value_t = 120,
		requires = "public_batch_aggregator"
	)]
	pub public_batch_reprove_after: u64,

	/// Seconds after which a pooled private batch is dropped even if it never
	/// settled. Defaults to the chain's proof acceptance window
	/// (`BlockHashCount` blocks).
	#[arg(long, value_name = "SECONDS", requires = "public_batch_aggregator")]
	pub public_batch_proof_expiry: Option<u64>,

	/// Maximum private-batch proofs held by the aggregator pool.
	#[arg(long, value_name = "N", default_value_t = 1024, requires = "public_batch_aggregator")]
	pub public_batch_pool_max_proofs: usize,
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
	Sc(Box<sc_cli::KeySubcommand>),
	/// Generate a quantus address
	Quantus {
		/// Type of the key
		#[arg(long, value_name = "SCHEME", value_enum, default_value_t = QuantusAddressType::Standard, ignore_case = true)]
		scheme: QuantusAddressType,

		/// Optional: Read a 128-character hex master seed (64 bytes) from stdin.
		/// The value is intentionally not accepted as a command-line argument
		/// (argv is world-readable and recorded in shell history / audit logs):
		/// on a terminal you are prompted without echo, otherwise pipe it in
		/// (e.g. `... --seed < seed.txt`). Mutually exclusive with --words.
		#[arg(long, conflicts_with = "words")]
		seed: bool,

		/// Optional: Read a BIP39 phrase ("word1 word2 ... word24") from stdin.
		/// The value is intentionally not accepted as a command-line argument
		/// (argv is world-readable and recorded in shell history / audit logs):
		/// on a terminal you are prompted without echo, otherwise pipe it in
		/// (e.g. `... --words < mnemonic.txt`). Mutually exclusive with --seed.
		#[arg(long, conflicts_with = "seed")]
		words: bool,

		/// Optional: HD wallet derivation index (default 0). Ignored if --no-derivation is set.
		#[arg(long, value_name = "INDEX", default_value_t = 0u32)]
		wallet_index: u32,

		/// Disable HD derivation. Generates the same result as current behavior.
		#[arg(long, default_value_t = false)]
		no_derivation: bool,

		/// Additionally print the public key / address hex. Secret material (seed,
		/// secret key) is never printed; everything is re-derivable from the mnemonic.
		#[arg(long, short = 'v', default_value_t = false)]
		verbose: bool,
	},
}

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum QuantusAddressType {
	Wormhole,
	Standard,
}
