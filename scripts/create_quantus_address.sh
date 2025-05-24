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

# Capture the full output of the key generation command
# quantus-node key quantus will print details including:
# Secret phrase: <mnemonic>
# Address: <ss58_address>
# Seed (hex): <hex_seed_value>
# ... and other details like Pub key, Secret key

KEY_GENERATION_OUTPUT=$("$BINARY_NAME" key quantus 2>&1) || {
    err "Failed to execute '$BINARY_NAME key quantus'. Output:\n$KEY_GENERATION_OUTPUT"
}

# We want to display the Secret Phrase, Address and Seed (hex) to the user from this script,
# and save the address.

# Parse the output for Secret Phrase (Mnemonic), Address, and Seed (hex)
SECRET_PHRASE=$(echo "$KEY_GENERATION_OUTPUT" | grep -i '^Secret phrase:' | sed -e 's/Secret phrase:[[:space:]]*//i' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
SS58_ADDRESS=$(echo "$KEY_GENERATION_OUTPUT" | grep -i '^Address:' | sed -e 's/Address:[[:space:]]*//i' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
HEX_SEED=$(echo "$KEY_GENERATION_OUTPUT" | grep -i '^Seed (hex):' | sed -e 's/Seed (hex):[[:space:]]*//i' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')

if [ -z "$SECRET_PHRASE" ] && [ -z "$HEX_SEED" ]; then # If neither phrase nor seed is found, something is wrong with parsing or output
    # This case should ideally not happen if quantus-node key quantus always outputs at least a seed.
    echo "Could not automatically parse the Secret Phrase or Seed. This might happen if the output format changed significantly."
    echo "Please check the full output below for your account details."
    echo "-------------------- Full Key Generation Output --------------------"
    echo "$KEY_GENERATION_OUTPUT"
    echo "----------------------------------------------------------------------"
fi

if [ -z "$SS58_ADDRESS" ]; then
    err "Could not parse the Address from the output. Raw output:\n$KEY_GENERATION_OUTPUT"
fi

# --- Display Information and Save Address ---

msg "New Quantus Account Details:"
echo "----------------------------------------------------------------------"
echo "Address: $SS58_ADDRESS"
if [ -n "$SECRET_PHRASE" ]; then
  echo "Secret Words: $SECRET_PHRASE" 
  echo "Write down your secret words and store them in a safe place."
elif [ -n "$HEX_SEED" ]; then # If no phrase, but we have a seed (e.g. user ran with --seed or future version behavior)
  echo "A secret phrase was not generated (e.g., a seed was likely provided or used internally)."
  echo "Ensure you have securely stored your seed or other recovery method."
fi

if [ -n "$HEX_SEED" ]; then
    echo "Seed:  $HEX_SEED"
    echo "[Seed can be used in place of secret words]"
fi
echo "----------------------------------------------------------------------"
echo ""

# Ensure config directory exists
mkdir -p "$CONFIG_DIR"

# Save the SS58 address to the file
echo "$SS58_ADDRESS" > "$REWARDS_ADDRESS_FILE"
if [ $? -eq 0 ]; then
    msg "The new Address has been saved to: $REWARDS_ADDRESS_FILE"
else
    err "Failed to save the Address to $REWARDS_ADDRESS_FILE"
fi

msg "You can now use this address in the start_quantus_node.sh script or when prompted." 