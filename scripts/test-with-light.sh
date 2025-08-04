#!/bin/bash

# Start solana-test-validator with ZK Compression programs, prover, photon
# indexer, cp-amm accounts, and a lookup table. Also deploys the cp-swap
# program.
./../light-protocol/cli/test_bin/run test-validator --validator-args "--clone DNXgeM9EiiaAbaWvwjHj9fQQLAX5ZsfHyvmYUNRAdNC8 \
     --clone D4FPEruKEHrG5TenZ2mpDGEfu1iUvTiqBxvpU8HLBvC2 \
     --clone 9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ \
     --url https://api.mainnet-beta.solana.com \
     --upgradeable-program CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C target/deploy/raydium_cp_swap.so ~/.config/solana/id.json" 

PHOTON_PID=$(pgrep -f photon)
PROVER_PID=$(pgrep -f prover)
VALIDATOR_PID=$(pgrep -f solana-test-validator)

solana airdrop 1000 ~/.config/solana/id.json --url "localhost"

echo "Running anchor test..."
anchor test --skip-local-validator --skip-deploy


# echo "Stopping test validator, photon, and prover..."
# kill $VALIDATOR_PID
# kill $PHOTON_PID
# kill $PROVER_PID
