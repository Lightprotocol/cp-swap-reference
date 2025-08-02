#!/bin/bash

# Start solana-test-validator with Light Protocol programs in the background
echo "Starting test validator with Light Protocol programs..."

./../light-protocol/cli/test_bin/run test-validator --validator-args "--clone DNXgeM9EiiaAbaWvwjHj9fQQLAX5ZsfHyvmYUNRAdNC8 \
     --clone D4FPEruKEHrG5TenZ2mpDGEfu1iUvTiqBxvpU8HLBvC2 \
     --clone 9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ \
     --url https://api.mainnet-beta.solana.com \
     --upgradeable-program CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C target/deploy/raydium_cp_swap.so ~/.config/solana/id.json" 

VALIDATOR_PID=$!
echo "Test validator started with PID: $VALIDATOR_PID"

# Wait for validator to start
sleep 3

solana airdrop 1000 "CLEuMG7pzJX9xAuKCFzBP154uiG1GaNo4Fq7x6KAcAfG" --url "localhost"

# Run tests
echo "Running anchor tests..."
anchor test --skip-local-validator --skip-deploy

# Clean up
echo "Stopping test validator..."
kill $VALIDATOR_PID