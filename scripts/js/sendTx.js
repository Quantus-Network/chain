// sendTx.js
const { ApiPromise, WsProvider, Keyring } = require('@polkadot/api');

async function main() {
  // 1) Connect to local node
  const wsProvider = new WsProvider('ws://127.0.0.1:9944');
  const api = await ApiPromise.create({ provider: wsProvider });
  await api.isReady;

  // 2) Print available pallets & calls
  console.log('Available pallets:', Object.keys(api.tx));
  console.log('Balances pallet calls:', Object.keys(api.tx.balances || {}));

  // 3) Create a Keyring and add Alice
  const keyring = new Keyring({ type: 'sr25519' });
  const alice = keyring.addFromUri('//Alice', { name: 'Alice default' });

  // 4) Prepare transaction using `transferKeepAlive`
  const to = '5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty'; // Bob
  const amount = 123456789;

  // Use `transferKeepAlive` instead of `transfer`
  const txHash = await api.tx.balances
    .transferKeepAlive(to, amount)
    .signAndSend(alice);

  console.log(`\n✅ Tx sent with hash: ${txHash}\n`);

  // 5) Disconnect
  process.exit(0);
}

main().catch(console.error);