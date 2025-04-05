// merkle-generator.js
const { ApiPromise, WsProvider } = require('@polkadot/api');
const { u8aToHex, stringToU8a } = require('@polkadot/util');
const { blake2AsU8a } = require('@polkadot/util-crypto');

async function main() {
    // Connect to local node to get types
    console.log('Connecting to node...');
    const wsProvider = new WsProvider('ws://127.0.0.1:9944');
    const api = await ApiPromise.create({ 
        provider: wsProvider,
        types: {
            MerkleRoot: '[u8; 32]'
        }
    });
    await api.isReady;
    console.log('API is ready');

    // Define accounts and amounts
    const accounts = [
        { address: '5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty', amount: '1000000000000' }, // Bob
        { address: '5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y', amount: '2000000000000' }  // Charlie
    ];

    console.log('\nGenerating Merkle tree for accounts:');
    accounts.forEach(acc => {
        console.log(`- ${acc.address}: ${acc.amount}`);
    });

    // Generate leaves
    const leaves = accounts.map(account => {
        const accountBytes = stringToU8a(account.address);
        const amountBytes = stringToU8a(account.amount);
        const combined = new Uint8Array([...accountBytes, ...amountBytes]);
        const leaf = blake2AsU8a(combined);
        return {
            account: account.address,
            amount: account.amount,
            leaf: leaf,
            leafHex: u8aToHex(leaf)
        };
    });

    // For a simple tree with 2 accounts
    let merkleRoot;
    let proofs = {};

    if (leaves.length === 1) {
        // If only one leaf, it becomes the root
        merkleRoot = leaves[0].leaf;
        proofs[leaves[0].account] = [];
    } else if (leaves.length === 2) {
        // For two leaves, hash them together
        const combinedLeaves = new Uint8Array([...leaves[0].leaf, ...leaves[1].leaf]);
        merkleRoot = blake2AsU8a(combinedLeaves);
        
        // Proofs for each account
        proofs[leaves[0].account] = [leaves[1].leafHex];
        proofs[leaves[1].account] = [leaves[0].leafHex];
    } else {
        // For more complex trees, implement a proper Merkle tree algorithm
        // This is a simplification - for production, use a proper Merkle tree library
        console.log('Multiple accounts detected, building Merkle tree...');
        const { root, proofMap } = buildMerkleTree(leaves);
        merkleRoot = root;
        proofs = proofMap;
    }

    // Output results
    console.log('\n==== MERKLE ROOT ====');
    console.log(u8aToHex(merkleRoot));
    console.log('\n==== ACCOUNT PROOFS ====');
    for (const account of accounts) {
        console.log(`\nAccount: ${account.address}`);
        console.log(`Amount: ${account.amount}`);
        console.log(`Proof: ${JSON.stringify(proofs[account.address])}`);
    }

    console.log('\n==== INSTRUCTIONS ====');
    console.log('1. Go to Polkadot.js Apps: https://polkadot.js.org/apps/');
    console.log('2. Connect to your local node: 127.0.0.1:9944');
    console.log('3. Go to Developer -> Extrinsics');
    console.log('4. Select merkleAirdrop.createAirdrop()');
    console.log('5. Enter the Merkle root shown above');
    console.log('6. Submit the transaction');
    console.log('7. Then fund the airdrop with merkleAirdrop.fundAirdrop()');
    console.log('8. To claim, use merkleAirdrop.claim() with the appropriate proof');

    // Cleanup
    await api.disconnect();
}

// Helper function to build a Merkle tree for more than 2 leaves
function buildMerkleTree(leaves) {
    // For this example, we'll implement a simple binary Merkle tree
    // For production, use a proper Merkle tree library
    
    let currentLevel = leaves.map(item => item.leaf);
    let proofMap = {};
    
    // Initialize proof maps for each leaf
    leaves.forEach(leaf => {
        proofMap[leaf.account] = [];
    });
    
    // Build the tree level by level
    while (currentLevel.length > 1) {
        const nextLevel = [];
        
        // Process pairs of nodes
        for (let i = 0; i < currentLevel.length; i += 2) {
            if (i + 1 < currentLevel.length) {
                // Hash pair of nodes
                const combined = new Uint8Array([...currentLevel[i], ...currentLevel[i + 1]]);
                const parent = blake2AsU8a(combined);
                nextLevel.push(parent);
                
                // Update proofs
                for (const leaf of leaves) {
                    const leafIndex = currentLevel.findIndex(
                        node => u8aToHex(node) === u8aToHex(leaf.leaf)
                    );
                    
                    if (leafIndex === i) {
                        // Add right sibling to proof
                        proofMap[leaf.account].push(u8aToHex(currentLevel[i + 1]));
                    } else if (leafIndex === i + 1) {
                        // Add left sibling to proof
                        proofMap[leaf.account].push(u8aToHex(currentLevel[i]));
                    }
                }
            } else {
                // Odd number of nodes, promote the last one
                nextLevel.push(currentLevel[i]);
            }
        }
        
        currentLevel = nextLevel;
    }
    
    // The root is the only node left at the top level
    return { root: currentLevel[0], proofMap };
}

main().catch(console.error);