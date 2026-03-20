#!/usr/bin/env node
// Test script for txWatch subscription.
// Usage: node scripts/testing/test_txwatch.mjs
//
// Requires a dev node running on ws://127.0.0.1:9944
//
// This script:
//   1. Subscribes to watch Bob's address for incoming transfers
//   2. Verifies the subscription is active and RPC methods are registered
//   3. Waits for a transfer (run scripts/testing/submit_transfer.sh in another terminal)

const WS_URL = "ws://127.0.0.1:9944";

const BOB = "qzmqrbfz2fKChALV61CvvtTzdk8SenzoadhLGbSiBEtUkYSvg";

let idCounter = 1;

function jsonRpc(method, params = []) {
  return JSON.stringify({ jsonrpc: "2.0", id: idCounter++, method, params });
}

function connect(url) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    ws.onopen = () => resolve(ws);
    ws.onerror = (e) => reject(e);
  });
}

function call(ws, method, params = []) {
  return new Promise((resolve, reject) => {
    const id = idCounter;
    const msg = jsonRpc(method, params);
    const handler = (event) => {
      const data = JSON.parse(event.data);
      if (data.id === id) {
        ws.removeEventListener("message", handler);
        if (data.error) reject(data.error);
        else resolve(data.result);
      }
    };
    ws.addEventListener("message", handler);
    ws.send(msg);
  });
}

function waitForNotification(ws, subscriptionId, timeoutMs = 120_000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      ws.removeEventListener("message", handler);
      reject(new Error(`Timed out (${timeoutMs}ms)`));
    }, timeoutMs);

    const handler = (event) => {
      const data = JSON.parse(event.data);
      if (
        data.method === "txWatch_transfer" &&
        data.params?.subscription === subscriptionId
      ) {
        clearTimeout(timer);
        ws.removeEventListener("message", handler);
        resolve(data.params.result);
      }
    };
    ws.addEventListener("message", handler);
  });
}

async function main() {
  console.log("Connecting to", WS_URL);
  const sock = await connect(WS_URL);
  console.log("Connected!\n");

  // Test 1: Subscribe
  console.log("1. Subscribing to watch:", BOB);
  const subId = await call(sock, "txWatch_watchAddress", [BOB]);
  console.log("   Subscription ID:", subId);

  // Test 2: Verify invalid address is rejected
  console.log("\n2. Testing invalid address rejection...");
  try {
    await call(sock, "txWatch_watchAddress", ["not-an-address"]);
    console.log("   FAIL: should have rejected");
  } catch (e) {
    console.log("   OK: rejected with:", e.message);
  }

  // Test 3: Unsubscribe works
  console.log("\n3. Testing unsubscribe...");
  const unsub = await call(sock, "txWatch_unwatchAddress", [subId]);
  console.log("   Unsubscribed:", unsub);

  // Test 4: Re-subscribe and wait for a real transfer
  console.log("\n4. Re-subscribing for live transfer detection...");
  const subId2 = await call(sock, "txWatch_watchAddress", [BOB]);
  console.log("   Subscription ID:", subId2);
  console.log("   Watching for transfers to Bob...");
  console.log("   (Submit a transfer to Bob from another tool, or timeout in 120s)\n");

  try {
    const notification = await waitForNotification(sock, subId2);
    console.log("===========================================");
    console.log("  TRANSFER DETECTED IN TX POOL!");
    console.log("===========================================");
    console.log("  tx_hash: ", notification.tx_hash);
    console.log("  from:    ", notification.from);
    console.log("  amount:  ", notification.amount);
    console.log("  asset_id:", notification.asset_id ?? "native");
    console.log("===========================================");
  } catch (e) {
    console.log("  Timeout:", e.message);
  }

  await call(sock, "txWatch_unwatchAddress", [subId2]);
  sock.close();
  console.log("\nDone.");
}

main().catch((e) => {
  console.error("Error:", e);
  process.exit(1);
});
