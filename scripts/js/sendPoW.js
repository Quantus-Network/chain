const { ApiPromise, WsProvider, Keyring } = require('@polkadot/api');

async function main() {
  // Explicit type definitions for PoW chain
  const types = {
    Extrinsic: {
      version: 'u8',
      signature: 'Option<(Address, Signature, ExtrinsicEra, Compact<Index>, Compact<Weight>)>',
      call: 'Call'
    },
    ExtrinsicEra: {
      _enum: ['Immortal', 'Mortal']
    }
  };

  const api = await ApiPromise.create({
    provider: new WsProvider('ws://127.0.0.1:9944'),
    types
  });

  const keyring = new Keyring({ type: 'sr25519' });
  const alice = keyring.addFromUri('//Alice');

  // Use immortal transactions (common in PoW chains)
  const tx = api.tx.balances.transferKeepAlive(
    '5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty',
    123456789
  );

  // Sign and send without era specification
  const unsub = await tx.signAndSend(alice, ({ events = [], status }) => {
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
