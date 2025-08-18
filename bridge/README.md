## Hyperlane bridge for Quantus <-> Solana

The PoC bridge between Quantus and Solana works by using a proxy ETH server that listens to the Quantus chain and uses the existing hyperlane infrastructure to send/receive messages to/from Solana.


### How to run

1. Start the Quantus chain:
    ```bash
    cargo run --release -- --dev --tmp -lerror,runtime::revive::strace=trace,runtime::revive=debug
    ```
2. Start the proxy ETH server

    ```bash
    cargo run --release --bin eth-rpc -- --dev
    ```
3. Deploy the Hyperlane contract on Quantus:
    ```bash
    hyperlane registry init
    hyperlane core init
    hyperlane core deploy
    ```
