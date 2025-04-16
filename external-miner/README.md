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

The HTTP server port can be configured using command-line arguments or an environment variable. The order of precedence is:

1.  Command-line argument: `-p <PORT>` or `--port <PORT>`
2.  Environment variable: `MINER_PORT=<PORT>`
3.  Default value: **9833**

Example:

```bash
# Run on the default port 9833
../target/release/external-miner

# Run on a custom port
../target/release/external-miner --port 8000

# Equivalent using the environment variable
export MINER_PORT=8000
../target/release/external-miner
```

## Running

After building the service, you can run it directly from the command line:

```bash
# Run with default port (9833)
RUST_LOG=info ../target/release/external-miner

# Run with a specific port
RUST_LOG=info ../target/release/external-miner --port 12345
```

The service will start and log messages to the console, indicating the port it's listening on.

Example output:
```
INFO  external_miner > Starting external miner service...
INFO  external_miner > Server starting on 0.0.0.0:9833 
```

## API Specification

The detailed API specification is defined using OpenAPI 3.0 and can be found in the `api/openapi.yaml` file.

This specification details all endpoints, request/response formats, and expected status codes.
You can use tools like [Swagger Editor](https://editor.swagger.io/) or [Swagger UI](https://swagger.io/tools/swagger-ui/) to view and interact with the API definition.

## API Endpoints (Summary)

*   `POST /mine`: Submits a new mining job.
*   `GET /result/{job_id}`: Retrieves the status and result of a specific mining job.
*   `POST /cancel/{job_id}`: Cancels an ongoing mining job. 