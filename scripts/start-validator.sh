#!/usr/bin/env bash
set -e

echo "Stopping any existing validator..."
light test-validator --stop 2>/dev/null || true
sleep 2

# Clean up old ledger to ensure fresh state
echo "Cleaning old ledger..."
rm -rf test-ledger

# Create pool fee receiver account file
PAYER_PUBKEY="ALA2cnz41Wa2v2EYUdkYHsg7VnKsbH1j7secM5aiP8k"
FEE_RECEIVER="DNXgeM9EiiaAbaWvwjHj9fQQLAX5ZsfHyvmYUNRAdNC8"

# Token account data (wSOL): mint(32) + owner(32) + amount(8) + delegate(36) + state(1) + ...
# mint = So11111111111111111111111111111111111111112 (wSOL)
FEE_FILE=$(mktemp)
cat > "$FEE_FILE" << 'JSONEOF'
{
  "pubkey": "DNXgeM9EiiaAbaWvwjHj9fQQLAX5ZsfHyvmYUNRAdNC8",
  "account": {
    "lamports": 2039280,
    "data": ["BpuIV/6rgYT7aH9jRhjANdrEOdwa6ztVmKDwAAAAAAECY+L7WEJcIRnY07lwy9TuaZBIebD9aqhznpq8Pv+mUQDKmjsAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQEAAADwHR8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "base64"],
    "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "executable": false,
    "rentEpoch": 0
  }
}
JSONEOF

echo "Starting validator with cp-swap program..."
light test-validator \
    --limit-ledger-size 50000000 \
    --upgradeable-program CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C target/deploy/raydium_cp_swap.so "$PAYER_PUBKEY" \
    --validator-args "--account $FEE_RECEIVER $FEE_FILE" &

# Wait for validator to be ready
echo "Waiting for validator to start..."
for i in $(seq 1 120); do
    if curl -s -X POST http://localhost:8899 -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' | grep -q '"ok"'; then
        echo "Validator is ready!"
        rm -f "$FEE_FILE"
        exit 0
    fi
    sleep 1
done
echo "Validator failed to start within 120 seconds"
rm -f "$FEE_FILE"
exit 1
