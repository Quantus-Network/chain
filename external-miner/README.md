# External Miner Service for Quantus Network

This crate provides an external mining service that can be used with a Quantus Network node. It exposes an HTTP API for
managing mining jobs.

## Building

To build the external miner service, navigate to the `external-miner` directory within the repository and use Cargo:

```bash
cd external-miner
cargo build --release
```

This will compile the binary and place it in the `target/release/` directory.

## Configuration

The service can be configured using command-line arguments or environment variables.

| Argument          | Environment Variable | Description                                | Default       |
|-------------------|----------------------|--------------------------------------------|---------------|
| `--port <PORT>`   | `MINER_PORT`         | The port for the HTTP server to listen on. | `9833`        |
| `--num-cores <N>` | `MINER_CORES`        | The number of CPU cores to use for mining. | All available |

Example:

```bash
# Run on the default port 9833 with all available cores
../target/release/external-miner

# Run on a custom port with 4 cores
../target/release/external-miner --port 8000 --num-cores 4

# Equivalent using environment variables
export MINER_PORT=8000
export MINER_CORES=4
../target/release/external-miner
```

## Running

After building the service, you can run it directly from the command line:

```bash
# Run with default settings
RUST_LOG=info ../target/release/external-miner

# Run with a specific port and 2 cores
RUST_LOG=info ../target/release/external-miner --port 12345 --num-cores 2

# Run in debug mode
RUST_LOG=info,miner=debug ../target/release/external-miner --num-cores 4

```

The service will start and log messages to the console, indicating the port it's listening on and the number of cores
it's using.

Example output:

```
INFO  external_miner > Starting external miner service...
INFO  external_miner > Using all available cores: 8
INFO  external_miner > Server starting on 0.0.0.0:9833 
```

## API Specification

The detailed API specification is defined using OpenAPI 3.0 and can be found in the `api/openapi.yaml` file.

This specification details all endpoints, request/response formats, and expected status codes.
You can use tools like [Swagger Editor](https://editor.swagger.io/)
or [Swagger UI](https://swagger.io/tools/swagger-ui/) to view and interact with the API definition.

## API Endpoints (Summary)

* `POST /mine`: Submits a new mining job.
* `GET /result/{job_id}`: Retrieves the status and result of a specific mining job.
* `POST /cancel/{job_id}`: Cancels an ongoing mining job. 