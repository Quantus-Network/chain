#!/usr/bin/env bash
# rustfmt.toml uses unstable options, so formatting needs a nightly rustfmt, while
# rust-toolchain.toml pins the stable compiler used for builds. rustfmt output
# drifts between nightlies, so the nightly is pinned in ../rustfmt-toolchain and
# every fmt invocation (CI and local scripts) goes through this wrapper to keep
# them identical. To bump: change the date in rustfmt-toolchain, run this with
# --all, commit the reformat.
set -euo pipefail
toolchain=$(<"$(dirname "${BASH_SOURCE[0]}")/../rustfmt-toolchain")
rustup toolchain list | grep -q "^${toolchain}-" ||
	rustup toolchain install "$toolchain" --profile minimal --component rustfmt
exec cargo "+$toolchain" fmt "$@"
