#!/bin/bash

# Starts solana-test-validator with all ZK Compression programs, prover, photon
# indexer, cp-amm accounts, and a lookup table. Also deploys the
# # cp-swap program.

./../light-protocol/cli/test_bin/run test-validator --validator-args "--clone DNXgeM9EiiaAbaWvwjHj9fQQLAX5ZsfHyvmYUNRAdNC8 \
     --clone D4FPEruKEHrG5TenZ2mpDGEfu1iUvTiqBxvpU8HLBvC2 \
     --account 9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ ./scripts/lut.json \
     --url https://api.mainnet-beta.solana.com \
     --upgradeable-program CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C target/deploy/raydium_cp_swap.so ~/.config/solana/id.json \
     --log-messages-bytes-limit 4194304"

# PHOTON_PID=$(pgrep -f photon)
# PROVER_PID=$(pgrep -f prover)
# VALIDATOR_PID=$(pgrep -f solana-test-validator)

solana airdrop 1000 ~/.config/solana/id.json --url "localhost"

echo "Running anchor test..."
NODE_OPTIONS="--enable-source-maps" anchor test --skip-local-validator --skip-deploy