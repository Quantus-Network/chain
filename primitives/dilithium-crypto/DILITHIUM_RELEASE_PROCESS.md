# Dilithium Crypto Release Process

This document describes the process for releasing the `qp-dilithium-crypto` crate to crates.io with separate versioning and tagging.

## Overview

The dilithium-crypto crate has its own independent release cycle with:
- Separate version tags: `dilithium-vX.Y.Z`
- Dedicated GitHub workflows
- Independent crates.io publishing
- Separate release proposals and PRs

## Prerequisites

### 1. Crates.io Setup
- Ensure you have a crates.io account
- Generate an API token with `publish-new` and `publish-update` permissions
- Add the token as `CARGO_REGISTRY_TOKEN` secret in GitHub repository settings

### 2. Repository Secrets
Make sure the following secrets are configured in GitHub:
- `CARGO_REGISTRY_TOKEN`: Your crates.io API token

## Release Process

### Step 1: Create Release Proposal

Use the GitHub Actions workflow to create a release proposal:

1. Go to **Actions** tab in GitHub
2. Select **"Dilithium - Release Proposal"** workflow
3. Click **"Run workflow"**
4. Fill in the parameters:
   - **Version**: The new version number (e.g., `0.1.1`)
   - **Draft**: Check if you want to create a draft release

This will:
- Create a new branch `release/dilithium-vX.Y.Z`
- Update the version in `primitives/dilithium-crypto/Cargo.toml`
- Update `Cargo.lock`
- Create a PR with the `dilithium-release-proposal` label

### Step 2: Review and Merge

1. Review the created PR
2. Ensure all tests pass
3. Verify the version number is correct
4. Merge the PR when ready

### Step 3: Automatic Release

When the PR is merged, the **"Dlilithium - Publish"** workflow automatically:

1. **Creates Tag**: Creates and pushes `dilithium-vX.Y.Z` tag
2. **Runs Tests**: Executes format checks, builds, and tests
3. **Creates GitHub Release**: Creates a GitHub release for the tag
4. **Publishes to crates.io**: Publishes the crate (only for non-draft releases)

## Workflow Files

### 1. `create-dilithium-release-proposal.yml`
- **Trigger**: Manual workflow dispatch
- **Purpose**: Creates release proposal PRs
- **Inputs**: Version number and draft flag

### 2. `dilithium-publish.yml`
- **Trigger**: PR merge with `dilithium-release-proposal` label
- **Purpose**: Handles the complete release process
- **Jobs**:
  - `create-dilithium-tag`: Creates and pushes the git tag
  - `format-checks`: Runs formatting and linting checks
  - `build-and-test-dilithium`: Builds and tests the crate
  - `create-dilithium-github-release`: Creates GitHub release
  - `publish-dilithium-to-crates`: Publishes to crates.io

## Package Metadata

The `qp-dilithium-crypto` package includes all required metadata for crates.io:

```toml
[package]
name = "qp-dilithium-crypto"
version = "0.1.0"
description = "Dilithium post-quantum cryptographic signatures implementation for Substrate"
license = "MIT"
repository = "https://github.com/Quantus-Network/chain"
homepage = "https://quantus.com"
documentation = "https://docs.rs/qp-dilithium-crypto"
authors = ["Quantus Network"]
keywords = ["cryptography", "dilithium", "post-quantum", "quantus-network", "substrate"]
categories = ["cryptography", "no-std"]
readme = "README.md"
```

## Version Management

- **Independent Versioning**: Dilithium-crypto versions are independent of the main chain versions
- **Semantic Versioning**: Follow semver (X.Y.Z) for dilithium releases
- **Tag Format**: Always use `dilithium-vX.Y.Z` format for tags

## Draft Releases

For testing or pre-releases:
1. Check the "Draft" option when creating the release proposal
2. This will create a draft GitHub release
3. Draft releases are **NOT** published to crates.io automatically
4. You can manually publish later by editing the GitHub release

## Troubleshooting

### Common Issues

1. **Missing Secrets**: Ensure `CARGO_REGISTRY_TOKEN` is set in repository secrets
2. **Version Conflicts**: Check if the version already exists on crates.io
3. **Build Failures**: Ensure all dependencies are available and tests pass
4. **Tag Conflicts**: Verify the tag doesn't already exist

### Manual Publishing

If automatic publishing fails, you can manually publish:

```bash
cd primitives/dilithium-crypto
cargo publish --token YOUR_TOKEN --allow-dirty
```

## Example Release

To release version `0.1.1`:

1. Run "Create Dilithium Release Proposal" workflow with version `0.1.1`
2. Review the created PR
3. Merge the PR
4. The release workflow will automatically:
   - Create tag `dilithium-v0.1.1`
   - Run tests and checks
   - Create GitHub release
   - Publish `qp-dilithium-crypto` v0.1.1 to crates.io

## Files Created/Modified

This release setup includes:

- **Workflows**:
  - `.github/workflows/dilithium-publish.yml`
  - `.github/workflows/dilithium-release-proposal.yml`

- **Package Files**:
  - `primitives/dilithium-crypto/Cargo.toml` (updated with metadata)
  - `primitives/dilithium-crypto/README.md`
  - `primitives/dilithium-crypto/LICENSE`

- **Documentation**:
  - `primitives/dilithium-crypto/DILITHIUM_RELEASE_PROCESS.md` (this file)
