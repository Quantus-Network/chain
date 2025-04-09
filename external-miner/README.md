# External Miner Service for Resonance Network

This crate provides an external mining service that can be used with a Resonance Network node. It exposes an HTTP API for managing mining jobs.

## Building

To build the external miner service, navigate to the `external-miner` directory within the repository and use Cargo:

```bash
cd external-miner
cargo build --release
```

This will compile the binary and place it in the `target/release/` directory.

## Configuration

The HTTP server port can be configured using the `MINER_PORT` environment variable. If this variable is not set, the service will default to port **9833**.

Example:

```bash
# Run on the default port 9833
export MINER_PORT=9833 

# Run on a custom port
export MINER_PORT=8000 
```

## Running

After building the service, you can run it directly from the command line:

```bash
# Ensure the MINER_PORT environment variable is set (optional)
# export MINER_PORT=9833

# Run the compiled binary
./target/release/external-miner
```

The service will start and log messages to the console, indicating the port it's listening on.

Example output:
```
INFO  external_miner > Starting external miner service...
INFO  external_miner > Server starting on 0.0.0.0:9833 
```

## API Endpoints

*   `POST /mine`: Submits a new mining job. Expects a JSON body with `MiningRequest` format.
*   `GET /result/{job_id}`: Retrieves the status and result of a specific mining job.
*   `POST /cancel/{job_id}`: Cancels an ongoing mining job. 