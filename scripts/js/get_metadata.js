const { ApiPromise, WsProvider } = require('@polkadot/api');

async function fetchMetadata() {
  const provider = new WsProvider('ws://127.0.0.1:9944');

  // Create the API instance
  const api = await ApiPromise.create({ provider });

  // Wait until the API is ready
  await api.isReady;

  // Fetch and log metadata
  const metadata = await api.runtimeMetadata;
  console.log("📜 Runtime Metadata:", metadata.toHuman());

  return api;
}

// Call the function to fetch metadata
fetchMetadata().catch(console.error);