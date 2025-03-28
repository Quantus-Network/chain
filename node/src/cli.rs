use sc_cli::RunCmd;

#[derive(Debug, clap::Parser)]
pub struct Cli {
	#[command(subcommand)]
	pub subcommand: Option<Subcommand>,

	#[clap(flatten)]
	pub run: RunCmd,

	/// Specify a rewards address for the miner
	#[arg(long, value_name = "REWARDS_ADDRESS")]
	pub rewards_address: Option<String>,
}

#[derive(Debug, clap::Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Subcommand {
	/// Key management cli utilities
	#[command(subcommand)]
	Key(ResonanceKeySubcommand),

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
	#[command(subcommand)]
	Benchmark(frame_benchmarking_cli::BenchmarkCmd),

	/// Db meta columns information.
	ChainInfo(sc_cli::ChainInfoCmd),
}

#[derive(Debug, clap::Subcommand)]
pub enum ResonanceKeySubcommand{

	/// Standard key commands from sc_cli
	#[command(flatten)]
	Sc(sc_cli::KeySubcommand),
	/// Generate a resonance address
	Resonance {
		/// Type of the key
		#[arg(long, value_name = "SCHEME", value_enum, ignore_case = true)]
		scheme: Option<ResonanceAddressType>,

		#[arg(long, value_name = "seed")]
		seed: Option<String>,
	},
}
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum ResonanceAddressType {
	Wormhole,
	Standard,
}

