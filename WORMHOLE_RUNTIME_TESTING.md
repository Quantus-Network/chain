# Wormhole Pallet Runtime Testing Guide

This guide provides step-by-step instructions for testing the wormhole pallet functionality with a running node.

## Quick Start (Automated)

Run the automated test script:

```bash
./scripts/test_wormhole_runtime.sh
```

This script will:
- Build the node if needed
- Start a development node
- Run comprehensive tests
- Clean up automatically

## Manual Testing Steps

### Step 1: Build and Start the Node

```bash
# Build the node (if not already built)
cargo build --release --package quantus-node

# Start development node
./target/release/quantus-node --dev
```

Or use the provided script:
```bash
./scripts/run_single_dev_node.sh
```

The node will be available at:
- RPC: `http://localhost:9944`
- WebSocket: `ws://localhost:9944`

### Step 2: Basic Health Checks

Test that the node is running properly:

```bash
# Check system health
curl -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"system_health","id":1}' http://localhost:9944

# Check chain version
curl -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"system_version","id":1}' http://localhost:9944

# Check chain name
curl -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"system_chain","id":1}' http://localhost:9944
```

Expected responses should contain `"result"` field without errors.

### Step 3: Test Account Generation

Generate a test account:

```bash
# Generate a standard account
./target/release/quantus-node key quantus

# Generate a wormhole account
./target/release/quantus-node key quantus --scheme wormhole
```

Save the generated address for testing.

### Step 4: Test Faucet (Optional)

If the faucet is enabled, test token requests:

```bash
# Replace <YOUR_ADDRESS> with generated address
curl -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"faucet_getAccountInfo","params":["<YOUR_ADDRESS>"],"id":1}' http://localhost:9944

curl -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"faucet_requestTokens","params":["<YOUR_ADDRESS>"],"id":1}' http://localhost:9944
```

### Step 5: Test Wormhole Verifier Availability

Check that the wormhole verifier is loaded correctly by checking the runtime metadata:

```bash
# Get runtime metadata (includes pallet information)
curl -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"state_getMetadata","id":1}' http://localhost:9944
```

Look for "Wormhole" in the response to confirm the pallet is loaded.

### Step 6: Monitor Node Logs

Watch the node logs for any wormhole-related messages:

```bash
# If running with RUST_LOG
RUST_LOG=info,pallet_wormhole=debug ./target/release/quantus-node --dev
```

You should see logs indicating:
- ✅ Wormhole verifier initialization
- ✅ Circuit data validation
- ❌ No panic messages or critical errors

### Step 7: Test Wormhole Proof Verification (Advanced)

**Note**: This requires a valid proof file. The test proof is located at `pallets/wormhole/proof_from_bins.hex`.

Create a simple extrinsic to test proof verification:

```bash
# This is a simplified example - actual implementation requires proper extrinsic encoding
# The automated script handles this complexity
```

For proper proof testing, use the automated script or Polkadot-JS Apps.

## Using Polkadot-JS Apps

1. Open [Polkadot-JS Apps](https://polkadot.js.org/apps/#/explorer?rpc=ws://localhost:9944)
2. Connect to your local node: `ws://localhost:9944`
3. Navigate to **Developer > Extrinsics**
4. Select the `wormhole` pallet
5. Choose `verifyWormholeProof` extrinsic
6. Test with valid/invalid proof data

## What to Look For

### ✅ Success Indicators:
- Node starts without panics
- RPC endpoints respond correctly
- Wormhole pallet appears in metadata
- Circuit verifier loads successfully
- Debug logs show "Wormhole verifier available"

### ❌ Failure Indicators:
- Node crashes on startup
- Panic messages in logs
- "Wormhole verifier not available" errors
- RPC calls fail with internal errors
- Missing wormhole pallet in metadata

## Common Issues and Solutions

### Issue: Node fails to start
**Solution**: Check that the circuit binaries exist:
```bash
ls -la pallets/wormhole/verifier.bin pallets/wormhole/common.bin
```

### Issue: Verifier not available
**Solution**: Check build logs for circuit validation errors:
```bash
cargo build --release --package quantus-node 2>&1 | grep -i wormhole
```

### Issue: RPC calls fail
**Solution**: Ensure node is fully synced and ready:
```bash
curl -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"system_health","id":1}' http://localhost:9944
```

## Testing Scenarios

### Scenario 1: Clean Startup Test
1. Clean build: `cargo clean && cargo build --release`
2. Start node: `./target/release/quantus-node --dev`
3. Verify no errors in logs
4. Test basic RPC calls

### Scenario 2: Circuit Integrity Test
1. Backup circuit files
2. Corrupt a circuit file: `echo "corrupted" > pallets/wormhole/verifier.bin`
3. Try to build: should fail with clear error message
4. Restore original files
5. Build should succeed

### Scenario 3: Runtime Integration Test
1. Start node
2. Submit various extrinsics
3. Verify wormhole pallet functions correctly
4. Check for memory leaks or performance issues

## Performance Monitoring

Monitor these metrics during testing:

- **Memory usage**: Should be stable, no significant leaks
- **CPU usage**: Should be reasonable for a development node
- **Response times**: RPC calls should respond quickly (<1s)
- **Log volume**: No excessive debug/error messages

## Cleanup

After testing:

```bash
# Stop the node (Ctrl+C)
# Clean up test data
rm -rf /tmp/wormhole-test-node
pkill -f "quantus-node"
```

## Advanced Testing

For more comprehensive testing:

1. **Multi-node testing**: Use `./scripts/run_local_nodes.sh`
2. **Load testing**: Submit multiple proof verifications
3. **Edge case testing**: Test with malformed proofs
4. **Upgrade testing**: Test runtime upgrades with new circuit data

## Troubleshooting

If you encounter issues:

1. Check the automated test script output
2. Review node logs in `/tmp/node.log`
3. Verify circuit binary integrity
4. Test with a clean build
5. Check system resources (disk space, memory)

## Next Steps

After successful runtime testing:

1. Test on a testnet environment
2. Perform stress testing
3. Test runtime upgrades
4. Validate with real proof data
5. Monitor long-running stability 