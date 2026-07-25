# POS Integration: Transaction Listener

Real-time payment detection for mobile point-of-sale apps using the `txWatch` RPC subscription.

## Overview

The `txWatch` subscription notifies your app the moment a transfer enters the transaction pool -- typically within seconds of the customer hitting "send". This is a **zero-confirmation mempool signal**: it is designed for instant UX feedback ("payment detected..."), not for payment confirmation.

## What a notification does and does not mean

A notification means a transaction **requesting** a transfer to your address entered the node's ready pool with a valid signature. It is decoded from the transaction's call data **before execution**, so it does **not** mean the transfer happened, or ever will. In particular:

- **The transaction may never be included in a block.** It can be dropped, replaced, or invalidated (e.g. the sender's balance is drained by a competing transaction).
- **The transfer can fail at execution** even after inclusion: insufficient balance, keep-alive violations, frozen or insufficient asset balances, etc. You will still have received a notification.
- **Batch semantics are not modeled.** Transfers found inside `Utility::batch` / `batch_all` / `force_batch` are each announced, but on-chain `batch` stops at the first failing item, `batch_all` rolls back the *entire* transaction if any item fails, and `force_batch` skips failed items. A batch that looks like several payments can execute none of them.
- **This is attacker-reachable.** Anyone can deliberately submit a transaction that triggers a convincing notification (any address, any amount) but never moves funds, paying at most a transaction fee -- e.g. a `batch_all` containing one real-looking transfer and one transfer that must fail.

> **Never release goods, mark an invoice paid, or credit a balance based on a `txWatch_transfer` notification alone.** Treat it as "customer initiated a payment". Before fulfilling, verify the transfer actually executed: wait for the transaction to be included in a finalized block (`chain_subscribeFinalizedHeads`) and check the resulting `Balances.Transfer` / `Assets.Transferred` event or the account balance. If you choose to accept zero-conf payments for speed, that is a deliberate risk decision -- only do so for amounts you can afford to lose.

## RPC Methods

| Method | Direction | Description |
|--------|-----------|-------------|
| `txWatch_watchAddress(address)` | request | Subscribe to transfers targeting `address` (SS58, prefix 189) |
| `txWatch_unwatchAddress(subscriptionId)` | request | Unsubscribe |
| `txWatch_transfer` | notification | Pushed when a matching transfer request enters the pool |

## Notification Shape

```json
{
  "tx_hash":  "0x9a3c...b7f1",
  "from":     "qzmXY...sender_address",
  "amount":   "5000000000000",
  "asset_id": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `tx_hash` | string | Transaction hash (hex, 0x-prefixed). Use it to verify inclusion and execution on-chain. |
| `from` | string | Sender SS58 address. Empty string if unsigned or non-standard addressing. |
| `amount` | string | **Requested** transfer amount in planck (1 UNIT = 10^12 planck). Not necessarily the amount that will move. |
| `asset_id` | number \| null | `null` for native QTU transfers. Integer asset ID for fungible asset transfers. |

## Supported Transfer Types

- `Balances::transfer_keep_alive` / `transfer_allow_death` (native QTU)
- `Assets::transfer` / `transfer_keep_alive` (fungible assets)
- All of the above inside `Utility::batch` / `batch_all` / `force_batch` (nested up to 4 levels; see the batch caveat above)

## Error Codes

| Code | Meaning |
|------|---------|
| 5011 | Invalid SS58 address |

## Integration Example (TypeScript / React Native)

```typescript
const MERCHANT_ADDRESS = "qzmYOUR_MERCHANT_ADDRESS_HERE";
const NODE_WS = "wss://rpc.quantus.network";

let idCounter = 1;

function connectAndWatch(
  address: string,
  onTransfer: (notification: TransferNotification) => void,
  onError?: (err: Error) => void,
) {
  const ws = new WebSocket(NODE_WS);
  let subscriptionId: string | null = null;

  ws.onopen = () => {
    ws.send(JSON.stringify({
      jsonrpc: "2.0",
      id: idCounter++,
      method: "txWatch_watchAddress",
      params: [address],
    }));
  };

  ws.onmessage = (event) => {
    const data = JSON.parse(event.data);

    // Subscribe response -- capture the subscription ID
    if (data.result && !subscriptionId) {
      subscriptionId = data.result;
      return;
    }

    // Transfer notification
    if (
      data.method === "txWatch_transfer" &&
      data.params?.subscription === subscriptionId
    ) {
      onTransfer(data.params.result);
    }

    // Error
    if (data.error) {
      onError?.(new Error(data.error.message));
    }
  };

  ws.onerror = () => onError?.(new Error("WebSocket connection failed"));

  return {
    unsubscribe: () => {
      if (subscriptionId) {
        ws.send(JSON.stringify({
          jsonrpc: "2.0",
          id: idCounter++,
          method: "txWatch_unwatchAddress",
          params: [subscriptionId],
        }));
      }
      ws.close();
    },
  };
}

interface TransferNotification {
  tx_hash: string;
  from: string;
  amount: string;
  asset_id: number | null;
}
```

### Usage

```typescript
const watcher = connectAndWatch(
  MERCHANT_ADDRESS,
  (tx) => {
    const amountQTU = Number(tx.amount) / 1e12;
    console.log(`Payment detected (pending): ${amountQTU} QTU from ${tx.from}`);
    // Show "payment detected, confirming..." in the UI.
    // Do NOT mark the invoice paid yet -- verify execution on-chain first.
  },
  (err) => console.error("Watcher error:", err),
);

// When the payment screen closes:
watcher.unsubscribe();
```

## Typical POS Flow

1. **Generate invoice** -- display a QR code with the merchant's address and expected amount.
2. **Open subscription** -- call `txWatch_watchAddress` with the merchant address.
3. **Wait for notification** -- when `txWatch_transfer` fires, compare `amount` to the expected value and show "payment detected, confirming..." to the customer.
4. **Verify execution** -- wait for the transaction to land in a finalized block (`chain_subscribeFinalizedHeads`) and confirm the transfer executed (check for the `Balances.Transfer` / `Assets.Transferred` event of `tx_hash`, or that the merchant balance increased). Only then mark the invoice paid and release goods.
5. **Unsubscribe** -- call `txWatch_unwatchAddress` to release the subscription.

## Tips

- **Amount is in planck.** 1 QTU = 1,000,000,000,000 planck. Divide by `1e12` for display.
- **One subscription per address is enough.** Multiple payments to the same address all arrive on the same subscription.
- **Reconnect on disconnect.** WebSocket connections can drop. Implement reconnect logic with backoff.
- **Subscription limits.** `txWatch` has no cap of its own; like the built-in Substrate subscriptions it is bounded by the node's server-level limits (`--rpc-max-connections`, default 100, and `--rpc-max-subscriptions-per-connection`, default 1024). There is no shared per-method pool, so one client cannot exhaust `txWatch` capacity for others. Still, unsubscribe from addresses you no longer need. Because the RPC is unauthenticated, operators exposing it publicly should front it with connection/rate limits (e.g. a reverse proxy) or restrict access, exactly as for the standard subscription endpoints.
- **Batch payments.** If a customer pays via a `Utility::batch` call containing multiple transfers, each matching transfer produces a separate notification within the same `tx_hash` -- remember these reflect the batch's call data, not its execution outcome.
