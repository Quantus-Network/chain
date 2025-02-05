#!/bin/bash

# Ensure script runs from its own directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Define the directory name
APP_DIR="apps"

echo "Run Polkadot JS web app - you can navigate to the site here"
echo "http://localhost:3000/#/explorer"
echo "Use the top left chain switcher to switch to local / custom chain, leave default values"

# Check if the repo has already been cloned
if [ -d "$APP_DIR" ]; then
    echo "Repository already cloned. Skipping setup..."
    cd "$APP_DIR"
else
    echo "Cloning repository..."
    git clone https://github.com/polkadot-js/apps.git "$APP_DIR"
    cd "$APP_DIR"
    echo "Installing dependencies..."
    yarn install
fi

echo "Running Polkadot JS web app => http://localhost:3000/"
yarn run start
