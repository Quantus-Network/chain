const { ApiPromise, WsProvider, Keyring } = require('@polkadot/api');

async function main() {


  // 1) Connect to local node
  const wsProvider = new WsProvider('ws://127.0.0.1:9944');
  const api = await ApiPromise.create({ provider: wsProvider });
  await api.isReady;

  // 2) Print available pallets & calls

  const metadata = await api.rpc.state.getMetadata();
  console.log(metadata.toHuman());
  
  console.log('Available pallets:', Object.keys(api.tx));
  console.log('Balances pallet calls:', Object.keys(api.tx.balances || {}));

  const rpcMethods = await api.rpc.rpc.methods();
  console.log("Available RPC methods:", rpcMethods.toHuman());

  // console.log("checking storage system");
  // const account = await api.query.system.account("5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty");
  // console.log("storage OK");
  // console.log("ACCOUNT: ", account.toHuman());
  // console.log("storage OK2");

  // 3) Create a Keyring and add Alice
  const keyring = new Keyring({ type: 'sr25519' });
  const alice = keyring.addFromUri('//Alice', { name: 'Alice default' });

  const rawStorage = await api.rpc.state.getStorage(
    api.createType('StorageKey', api.query.system.account.key(alice.address))
  );
  console.log("Raw Storage:", rawStorage.toHex());

  // 4) Prepare transaction using `transferKeepAlive`
  const to = '5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty'; // Bob
  const amount = 123456789;

  // Fetch latest nonce for Alice
  const { nonce } = await api.query.system.account(alice.address);

  // Get current block hash for era information
  const currentBlockHash = await api.rpc.chain.getBlockHash();
  const currentBlock = await api.rpc.chain.getBlock(currentBlockHash);
  const currentBlockNumber = currentBlock.block.header.number.toNumber();

  // Define era: Valid for 64 blocks (~16 minutes on default Substrate chain)
  const mortalEra = api.createType('ExtrinsicEra', { current: currentBlockNumber, period: 64 });

  console.log(`Using nonce: ${nonce}, block number: ${currentBlockNumber}`);


  // Sign transaction with correct nonce and era
  const tx = api.tx.balances.transferKeepAlive(to, amount);
  const signedTx = await tx.signAsync(alice, { nonce, era: mortalEra });

  // Send transaction
  const txHash = await api.rpc.author.submitExtrinsic(signedTx);
  console.log(`\n✅ Tx sent with hash: ${txHash}\n`);

  // 5) Disconnect
  process.exit(0);
}

main().catch(console.error);