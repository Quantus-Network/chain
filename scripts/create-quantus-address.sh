#!/bin/bash
#
# create-quantus-address.sh: Generates a new Quantus rewards address and saves it.
#

set -euo pipefail # More robust error handling

CONFIG_DIR="$HOME/.quantus-node"
REWARDS_ADDRESS_FILE="$CONFIG_DIR/rewards_address.txt"
BINARY_NAME="quantus-node"

# --- Helper Functions ---
msg() {
  echo -e "\033[1;32mINFO\033[0m: $1"
}

important_msg() {
  echo -e "\033[1;33mIMPORTANT\033[0m: $1"
}

err() {
  echo -e "\033[1;31mERROR\033[0m: $1" >&2
  exit 1
}

# --- Sanity Check ---
if ! command -v "$BINARY_NAME" &>/dev/null; then
    err "'$BINARY_NAME' command not found. Please ensure it is installed and in your PATH."
fi

# --- Generate Key ---
msg "Generating a new Quantus address (Scheme: Standard)..."

# Capture the output of the key generation command
# The exact output format might vary slightly, adjust parsing if needed.
KEY_GENERATION_OUTPUT=$("$BINARY_NAME" key quantus --scheme Standard 2>&1) || {
    err "Failed to execute '$BINARY_NAME key quantus --scheme Standard'. Output:\n$KEY_GENERATION_OUTPUT"
}

# Parse the output for Secret Phrase (Mnemonic) and SS58 Address
# This assumes common Substrate output patterns.
SECRET_PHRASE=$(echo "$KEY_GENERATION_OUTPUT" | grep -i 'Secret phrase' | sed -e 's/Secret phrase[[:space:]]*//i' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
SS58_ADDRESS=$(echo "$KEY_GENERATION_OUTPUT" | grep -i 'SS58 Address' | sed -e 's/SS58 Address:[[:space:]]*//i' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')

if [ -z "$SECRET_PHRASE" ]; then
    err "Could not parse the Secret Phrase from the output. Raw output:\n$KEY_GENERATION_OUTPUT"
fi

if [ -z "$SS58_ADDRESS" ]; then
    err "Could not parse the SS58 Address from the output. Raw output:\n$KEY_GENERATION_OUTPUT"
fi

# --- Display Information and Save Address ---

msg "New Quantus Address Generated Successfully!"
echo "----------------------------------------------------------------------"
echo "SS58 Address:     $SS58_ADDRESS"
echo "Secret Phrase (Mnemonic): $SECRET_PHRASE"
echo "----------------------------------------------------------------------"

important_msg "WRITE DOWN YOUR SECRET PHRASE (MNEMONIC) AND STORE IT IN A SAFE PLACE."
important_msg "This phrase is the ONLY way to recover your account if you lose access."
important_msg "DO NOT share it with anyone."
echo ""

# Ensure config directory exists
mkdir -p "$CONFIG_DIR"

# Save the SS58 address to the file
echo "$SS58_ADDRESS" > "$REWARDS_ADDRESS_FILE"
if [ $? -eq 0 ]; then
    msg "The new SS58 Address has been saved to: $REWARDS_ADDRESS_FILE"
else
    err "Failed to save the SS58 Address to $REWARDS_ADDRESS_FILE"
fi

msg "You can now use this address in the start-quantus-node.sh script." 