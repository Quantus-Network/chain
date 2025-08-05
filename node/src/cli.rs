use clap::{Args, Parser};
use sc_cli::{Error, GenerateCmd, InsertKeyCmd, InspectKeyCmd, RunCmd, SubstrateCli};
use sc_network::Keypair;
use sc_service::BasePath;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::{fs, io};

#[derive(Debug, clap::Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub subcommand: Option<Subcommand>,

    #[clap(flatten)]
    pub run: RunCmd,

    /// Specify a rewards address for the miner
    #[arg(long, value_name = "REWARDS_ADDRESS")]
    pub rewards_address: Option<String>,

    /// Specify the URL of an external QPoW miner service
    #[arg(long, value_name = "EXTERNAL_MINER_URL")]
    pub external_miner_url: Option<String>,

    /// Enable peer sharing via RPC endpoint
    #[arg(long)]
    pub enable_peer_sharing: bool,
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
    // #[command(subcommand)]
    // Benchmark(frame_benchmarking_cli::BenchmarkCmd),

    /// Db meta columns information.
    ChainInfo(sc_cli::ChainInfoCmd),
}

#[derive(Debug, Args, Clone)]
pub struct GenerateKeyCmdCommon {
    /// Name of file to save secret key to.
    /// If not given, the secret key is printed to stdout.
    #[arg(long)]
    file: Option<PathBuf>,

    /// The output is in raw binary format.
    /// If not given, the output is written as an hex encoded string.
    #[arg(long)]
    bin: bool,
}

/// The `generate-node-key` command
#[derive(Debug, Clone, Parser)]
#[command(
    name = "generate-node-key",
    about = "Generate a random node key, write it to a file or stdout \
		 	and write the corresponding peer-id to stderr"
)]
pub struct GenerateNodeKeyCmd {
    #[clap(flatten)]
    pub common: GenerateKeyCmdCommon,
    /// Specify the chain specification.
    ///
    /// It can be any of the predefined chains like dev, local, staging, polkadot, kusama.
    #[arg(long, value_name = "CHAIN_SPEC")]
    pub chain: Option<String>,
    /// A directory where the key should be saved. If a key already
    /// exists in the directory, it won't be overwritten.
    #[arg(long, conflicts_with_all = ["file", "default_base_path"])]
    base_path: Option<PathBuf>,

    /// Save the key in the default directory. If a key already
    /// exists in the directory, it won't be overwritten.
    #[arg(long, conflicts_with_all = ["base_path", "file"])]
    default_base_path: bool,
}

impl GenerateNodeKeyCmd {
    /// Run the command
    pub fn run(&self, chain_spec_id: &str, executable_name: &String) -> Result<(), Error> {
        // NOTE: we ignore the --bin arg because we use protobufs for entire keys now
        generate_key_in_file(
            &self.common.file,
            Some(chain_spec_id),
            &self.base_path,
            self.default_base_path,
            Some(executable_name),
            None,
        )
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum KeySubcommand {
    /// Generate a random node key, write it to a file or stdout and write the
    /// corresponding peer-id to stderr
    GenerateNodeKey(GenerateNodeKeyCmd),

    /// Generate a random account
    Generate(GenerateCmd),

    /// Gets a public key and a SS58 address from the provided Secret URI
    Inspect(InspectKeyCmd),

    /// Load a node key from a file or stdin and print the corresponding peer-id
    InspectNodeKey(InspectNodeKeyCmd),

    /// Insert a key to the keystore of a node.
    Insert(InsertKeyCmd),
}

/// The `inspect-node-key` command
#[derive(Debug, Parser)]
#[command(
    name = "inspect-node-key",
    about = "Load a node key from a file or stdin and print the corresponding peer-id."
)]
pub struct InspectNodeKeyCmd {
    /// Name of file to read the secret key from.
    /// If not given, the secret key is read from stdin (up to EOF).
    #[arg(long)]
    file: Option<PathBuf>,

    /// The input is in raw binary format.
    /// If not given, the input is read as an hex encoded string.
    #[arg(long)]
    bin: bool,

    /// This argument is deprecated and has no effect for this command.
    #[deprecated(note = "Network identifier is not used for node-key inspection")]
    #[arg(
        short = 'n',
        long = "network",
        value_name = "NETWORK",
        ignore_case = true
    )]
    pub network_scheme: Option<String>,
}

impl InspectNodeKeyCmd {
    /// runs the command
    pub fn run(&self) -> Result<(), Error> {
        let mut file_data = match &self.file {
            Some(file) => fs::read(&file)?,
            None => {
                let mut buf = Vec::new();
                io::stdin().lock().read_to_end(&mut buf)?;
                buf
            }
        };

        let keypair =
            Keypair::from_protobuf_encoding(&mut file_data).map_err(|_| "Bad node key file")?;

        println!("{}", keypair.public().to_peer_id());

        Ok(())
    }
}

impl KeySubcommand {
    /// run the key subcommands
    pub fn run<C: SubstrateCli>(&self, cli: &C) -> Result<(), Error> {
        match self {
            KeySubcommand::GenerateNodeKey(cmd) => {
                let chain_spec = cli.load_spec(cmd.chain.as_deref().unwrap_or(""))?;
                cmd.run(chain_spec.id(), &C::executable_name())
            }
            KeySubcommand::Generate(cmd) => cmd.run(),
            KeySubcommand::Inspect(cmd) => cmd.run(),
            KeySubcommand::Insert(cmd) => cmd.run(cli),
            KeySubcommand::InspectNodeKey(cmd) => cmd.run(),
        }
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum QuantusKeySubcommand {
    /// Standard key commands from sc_cli
    #[command(flatten)]
    Sc(KeySubcommand),
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
    },
}

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum QuantusAddressType {
    Wormhole,
    Standard,
}

const NODE_KEY_DILITHIUM_FILE: &str = "secret_dilithium";
const DEFAULT_NETWORK_CONFIG_PATH: &str = "network";

/// Returns the value of `base_path` or the default_path if it is None
pub(crate) fn base_path_or_default(
    base_path: Option<BasePath>,
    executable_name: &String,
) -> BasePath {
    base_path.unwrap_or_else(|| BasePath::from_project("", "", executable_name))
}

/// Returns the default path for configuration  directory based on the chain_spec
pub(crate) fn build_config_dir(base_path: &BasePath, chain_spec_id: &str) -> PathBuf {
    base_path.config_dir(chain_spec_id)
}

/// Returns the default path for the network configuration inside the configuration dir
pub(crate) fn build_net_config_dir(config_dir: &PathBuf) -> PathBuf {
    config_dir.join(DEFAULT_NETWORK_CONFIG_PATH)
}

/// Returns the default path for the network directory starting from the provided base_path
/// or from the default base_path.
pub(crate) fn build_network_key_dir_or_default(
    base_path: Option<BasePath>,
    chain_spec_id: &str,
    executable_name: &String,
) -> PathBuf {
    let config_dir = build_config_dir(
        &base_path_or_default(base_path, executable_name),
        chain_spec_id,
    );
    build_net_config_dir(&config_dir)
}

/// This is copied from sc-cli and adapted to dilithium
pub(crate) fn generate_key_in_file(
    file: &Option<PathBuf>,
    chain_spec_id: Option<&str>,
    base_path: &Option<PathBuf>,
    default_base_path: bool,
    executable_name: Option<&String>,
    keypair: Option<&Keypair>,
) -> Result<(), Error> {
    let kp: Keypair;
    if let Some(k) = keypair {
        kp = k.clone();
    } else {
        kp = Keypair::generate_dilithium();
    }
    let file_data = kp.to_protobuf_encoding().unwrap();

    match (file, base_path, default_base_path) {
        (Some(file), None, false) => fs::write(file, file_data)?,
        (None, Some(_), false) | (None, None, true) => {
            let network_path = build_network_key_dir_or_default(
                base_path.clone().map(BasePath::new),
                chain_spec_id.unwrap_or_default(),
                executable_name.ok_or(Error::Input("Executable name not provided".into()))?,
            );

            fs::create_dir_all(network_path.as_path())?;

            let key_path = network_path.join(NODE_KEY_DILITHIUM_FILE);
            if key_path.exists() {
                eprintln!("Skip generation, a key already exists in {:?}", key_path);
                return Err(Error::KeyAlreadyExistsInPath(key_path));
            } else {
                eprintln!("Generating key in {:?}", key_path);
                fs::write(key_path, file_data)?
            }
        }
        (None, None, false) => io::stdout().lock().write_all(&file_data)?,
        (_, _, _) => {
            // This should not happen, arguments are marked as mutually exclusive.
            return Err(Error::Input("Mutually exclusive arguments provided".into()));
        }
    }

    eprintln!("{}", kp.public().to_peer_id());

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cli::{
        generate_key_in_file, GenerateNodeKeyCmd, InspectNodeKeyCmd, NODE_KEY_DILITHIUM_FILE,
    };
    use clap::Parser;
    use sc_cli::Error;
    use sc_network::Keypair;
    use std::fs;
    use tempfile;

    #[test]
    fn test_generate_key_in_file_explicit_path() {
        // Setup: Create a temporary directory and file path.
        let temp_dir = tempfile::TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.key");
        let original_keypair = Keypair::generate_dilithium();
        // Generate and write keypair.
        let result = generate_key_in_file(
            &Some(file_path.clone()),
            None,
            &None,
            false,
            Some(&"test-exec".to_string()),
            Some(&original_keypair),
        );
        assert!(result.is_ok(), "Failed to generate key: {:?}", result);

        // Read the file contents.
        let file_data = fs::read(&file_path).expect("Failed to read file");

        // Deserialize the keypair.
        let deserialized_keypair =
            Keypair::from_protobuf_encoding(&file_data).expect("Failed to deserialize keypair");

        // Verify the deserialized keypair matches the original.
        assert_eq!(
            deserialized_keypair.public().to_peer_id(),
            original_keypair.public().to_peer_id(),
            "Deserialized public key does not match original"
        );
    }

    #[test]
    fn test_generate_key_in_file_default_path() {
        // Setup: Create a temporary directory as base path.
        let temp_dir = tempfile::TempDir::new().unwrap();
        let base_path = Some(temp_dir.path().to_path_buf());
        let chain_spec_id = "test-chain";
        let executable_name = "test-exec";
        let original_keypair = Keypair::generate_dilithium();

        // Generate and write keypair.
        let result = generate_key_in_file(
            &None,
            Some(chain_spec_id),
            &base_path,
            false,
            Some(&executable_name.to_string()),
            Some(&original_keypair),
        );
        assert!(result.is_ok(), "Failed to generate key: {:?}", result);

        // Construct the expected key file path.
        let expected_path = temp_dir
            .path()
            .join("chains")
            .join(chain_spec_id)
            .join("network")
            .join(NODE_KEY_DILITHIUM_FILE);

        // Read the file contents.
        let file_data = fs::read(&expected_path).expect("Failed to read file");

        // Deserialize the keypair.
        let deserialized_keypair =
            Keypair::from_protobuf_encoding(&file_data).expect("Failed to deserialize keypair");

        // Verify the deserialized keypair matches the original.
        assert_eq!(
            deserialized_keypair.public().to_peer_id(),
            original_keypair.public().to_peer_id(),
            "Deserialized public key does not match original"
        );
    }

    #[test]
    fn test_generate_key_in_file_key_already_exists() {
        // Setup: Create a temporary directory and pre-create the key file.
        let temp_dir = tempfile::TempDir::new().unwrap();
        let base_path = Some(temp_dir.path().to_path_buf());
        let chain_spec_id = "test-chain";
        let executable_name = "test-exec";

        // Create the network directory and key file.
        let network_path = temp_dir
            .path()
            .join("chains")
            .join(chain_spec_id)
            .join("network");
        fs::create_dir_all(&network_path).unwrap();
        let key_path = network_path.join(NODE_KEY_DILITHIUM_FILE);
        fs::write(&key_path, vec![0u8; 8]).unwrap(); // Write dummy data.

        // Attempt to generate key (should fail due to existing file).
        let result = generate_key_in_file(
            &None,
            Some(chain_spec_id),
            &base_path,
            false,
            Some(&executable_name.to_string()),
            None,
        );
        assert!(
            matches!(result, Err(Error::KeyAlreadyExistsInPath(_))),
            "Expected KeyAlreadyExistsInPath error, got: {:?}",
            result
        );
    }

    #[test]
    fn inspect_node_key() {
        let path = tempfile::tempdir()
            .unwrap()
            .keep()
            .join("node-id")
            .into_os_string();
        let path = path.to_str().unwrap();
        let cmd = GenerateNodeKeyCmd::parse_from(&["generate-node-key", "--file", path]);

        assert!(cmd.run("test", &String::from("test")).is_ok());

        let cmd = InspectNodeKeyCmd::parse_from(&["inspect-node-key", "--file", path]);
        assert!(cmd.run().is_ok());
    }
}
