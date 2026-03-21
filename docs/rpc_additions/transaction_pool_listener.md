# POS Integration: Transaction Listener

Real-time payment detection for mobile point-of-sale apps using the `txWatch` RPC subscription.

## Overview

The `txWatch` subscription notifies your app the moment a transfer enters the transaction pool -- typically within seconds of the customer hitting "send". Transactions in the pool have already passed signature validation, so a notification is a strong signal that the payment is legitimate.

> **Important:** notifications reflect transaction pool visibility, not block inclusion or finality. In rare cases a transaction can be dropped (e.g. the sender's account is drained by a competing tx). For high-value payments, confirm finality via `chain_subscribeFinalizedHeads` after receiving the pool notification.

## RPC Methods

| Method | Direction | Description |
|--------|-----------|-------------|
| `txWatch_watchAddress(address)` | request | Subscribe to transfers targeting `address` (SS58, prefix 189) |
| `txWatch_unwatchAddress(subscriptionId)` | request | Unsubscribe |
| `txWatch_transfer` | notification | Pushed when a matching transfer enters the pool |

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
| `tx_hash` | string | Transaction hash (hex, 0x-prefixed) |
| `from` | string | Sender SS58 address. Empty string if unsigned or non-standard addressing. |
| `amount` | string | Transfer amount in planck (1 UNIT = 10^12 planck) |
| `asset_id` | number \| null | `null` for native QTU transfers. Integer asset ID for fungible asset transfers. |

## Supported Transfer Types

- `Balances::transfer_keep_alive` / `transfer_allow_death` (native QTU)
- `Assets::transfer` / `transfer_keep_alive` (fungible assets)
- All of the above inside `Utility::batch` / `batch_all` / `force_batch` (nested up to 4 levels)

## Error Codes

| Code | Meaning |
|------|---------|
| 5010 | Too many concurrent subscriptions (max 32 per node) |
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
    console.log(`Payment received: ${amountQTU} QTU from ${tx.from}`);
    // Update your UI, mark invoice as paid, etc.
  },
  (err) => console.error("Watcher error:", err),
);

// When the payment screen closes:
watcher.unsubscribe();
```

## Typical POS Flow

1. **Generate invoice** -- display a QR code with the merchant's address and expected amount.
2. **Open subscription** -- call `txWatch_watchAddress` with the merchant address.
3. **Wait for notification** -- when `txWatch_transfer` fires, compare `amount` to the expected value.
4. **Show confirmation** -- display success to the customer. For added safety, you can also listen for block finality.
5. **Unsubscribe** -- call `txWatch_unwatchAddress` to free the subscription slot.

## Tips

- **Amount is in planck.** 1 QTU = 1,000,000,000,000 planck. Divide by `1e12` for display.
- **One subscription per address is enough.** Multiple payments to the same address all arrive on the same subscription.
- **Reconnect on disconnect.** WebSocket connections can drop. Implement reconnect logic with backoff.
- **Max 32 subscriptions per node.** If you hit error 5010, you're at the limit. Unsubscribe from addresses you no longer need.
- **Batch payments.** If a customer pays via a `Utility::batch` call containing multiple transfers, each matching transfer produces a separate notification within the same `tx_hash`.
