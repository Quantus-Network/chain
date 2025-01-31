const { ApiPromise, WsProvider, Keyring } = require('@polkadot/api');

async function main() {
  const provider = new WsProvider('ws://127.0.0.1:9944');

  // No need to manually specify types, Polkadot.js can infer them
  const api = await ApiPromise.create({ provider });

  const keyring = new Keyring({ type: 'sr25519' });
  const alice = keyring.addFromUri('//Alice');

  // Create the transaction
  const tx = api.tx.balances.transferKeepAlive(
    '5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty',
    123456789
  );

  // Fetch nonce
  const { nonce } = await api.query.system.account(alice.address);

  // Sign and send explicitly as an Immortal transaction
  const unsub = await tx.signAndSend(alice, { nonce, era: api.createType('ExtrinsicEra', 0) }, ({ events = [], status }) => {
    console.log(`Transaction status: ${status.type}`);
    
    if (status.isInBlock) {
      console.log(`Included in block ${status.asInBlock}`);
      events.forEach(({ event }) => {
        console.log(`\t${event.section}.${event.method}:: ${event.data}`);
      });
      unsub();
      process.exit(0);
    }
  });
}

main().catch(console.error);