#!/bin/bash
#
# gh workflow run create-release.yml -- Triggers the GitHub Actions release workflow.
#
# USAGE:
#   gh workflow run create-release.yml [FLAGS]
#
# FLAGS:
#   -f version_type=<TYPE>        Bump type: patch, minor, major, custom. Default: patch
#   -f custom_version=<VERSION>   Custom version (e.g., v1.2.3). Use if version_type=custom.
#   -f is_prerelease=<BOOL>       Is it a pre-release? true, false. Default: false
#   -f prerelease_identifier=<ID> Pre-release ID (e.g., rc, beta). Default: rc
#   -f draft_release=<BOOL>       Create as draft? true, false. Default: true
#   -f fast_test_create_release_job=<BOOL> Skip build, use dummy artifacts? true, false. Default: false
#
# EXAMPLES:

# --- Example 1: Trigger a 'patch' release, run in fast-test mode, create as draft ---
echo "Triggering 'patch' release (fast-test mode, draft):"
gh workflow run create-release.yml \
  -f version_type=patch \
  -f is_prerelease=false \
  -f draft_release=true \
  -f fast_test_create_release_job=false

# Wait a moment for the workflow to be registered
sleep 5

echo ""
echo "Watching the latest 'create-release.yml' workflow run..."
gh run watch $(gh run list --workflow=create-release.yml --limit 1 --json databaseId --jq '.[0].databaseId') --exit-status --interval 10

echo ""
echo "-----------------------------------------------------"
echo ""

# --- Example 2: Trigger a 'minor' pre-release (e.g., for a release candidate), full build, publish directly ---
# echo "Triggering 'minor' pre-release (full build, non-draft):"
# gh workflow run create-release.yml \
#   -f version_type=minor \
#   -f is_prerelease=true \
#   -f prerelease_identifier=rc \
#   -f draft_release=false \
#   -f fast_test_create_release_job=false
#
# # Wait a moment for the workflow to be registered
# sleep 5
#
# echo ""
# echo "Watching the latest 'create-release.yml' workflow run..."
# gh run watch $(gh run list --workflow=create-release.yml --limit 1 --json databaseId --jq '.[0].databaseId') --exit-status --interval 10

