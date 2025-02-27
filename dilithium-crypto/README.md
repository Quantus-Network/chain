# Dilithium Crypto

This crate implements all necessary types and traits to enable Dilithium crypto in a substrate-based blockchain.

## How to change the signature scheme in Substrate 

_"A signed message is really just a blob of bytes"_ - Gavin Wood

### Substrate Architecture Background

Substrate has an extremely flexible architecture allowing for different valid crypto schemes being used in a chain simultaneously.
The schemes enabled by default are ECDSA, Ed25519 and Sr25519.

### How to think about the signature scheme for transactions (extrinsics)

We started by looking at the UncheckedExtrinsic struct. We noticed that any transaction submitted to the node is immediately decoded into an UncheckedExtrinsic. 

### Which types to swap out 

In Runtime/lib.rs, we swap out this statement 

```rust
pub type Signature = MultiSignature;
```

with this one 

```rust
pub type Signature = ResonanceSignatureScheme;
```

In order to make ResonanceSignatureScheme implement the Verify trait, and all the other required traits and derives, we also need to implement the following types:

- ResonanceSigner (equivalent to MultiSigner in Substrate)
- ResonancePublic (equivalent to Public in the Schnorr signature scheme sr25519)
- ResonanceSignature (equivalent to Signature in sr25519)
- ResonancePair (equivalent to Pair in sr25519)

These also need to implement many associated traits - see traits.rs.

Then we must implement the verify trait - and this is where the new signature is verified. 

Now all that's remaining is to see if an UncheckedExtrinsic created by the new signature scheme is accepted by the node. How to create and handle this is outlined in the integration tests in runtime/tests/integration.rs 

### Notes

While it is relatively straightforward to implement a new signature scheme using these instructions, we spent a lot of time (a LOT of time) figuring out exactly what the minimal implementation is. 

There is no other tutorial or set of instructions or sample implementation. 

### Remaining issues and unknowns

There are a few issues remaining in this crate:

#### Ingegration tests in this crate
The integration tests don't work inside this crate due to feature unifications / selecting the wrong features in the test config. 

Since we are running the tests correctly in runtime, using the runtime "cargo test" command, we know the tests are valid, and the outcome is as expected, e.g., signing, encoding, decoding, and verifying all work. 

But the Pair trait has a conditional implemenation of "sign" which we found challenging to integrate in this crate, either too much is turned on and the tests work here, but the code doesn't work in the runtime, or vice versa. We prefer code to run in the runtime. There is likely a way to run the test with the correct build settings to avoid this. 

This issue is not related to actual code execution, or the runtime configuration, but only related to build commands, crate build logic, and so on. 

#### A more minimal implementation

We can definitely remove the standard signature schemes, and a bit of trait implementation code with them. We will do that for mainnet. Meanwhile it's nice to have the standard schemes in the runtime for checking that we can make and submit transactions using the web interface or vanilla polkadot JS.

Once we have the client implemented, we won't need this anymore. 

There may also be other things that can be removed or minimized. It's a lot of effort to experiment with this. 

#### 










